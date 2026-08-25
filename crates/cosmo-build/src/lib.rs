//! Build a Rust crate into a fat **Actually Portable Executable** from its
//! `build.rs` -- one file that runs on x86-64 and arm64, linked against
//! Cosmopolitan Libc.
//!
//! Cargo has no concept of a multi-architecture artifact, so one `cargo build`
//! can never produce an APE. [`apeify`] runs the build once per architecture
//! and fuses the two ELFs with `apelink`, which is what cosmocc does for C.
//!
//! # Adding it to a project
//!
//! ```toml
//! [features]
//! ape = ["dep:cosmo-build"]      # or: default = ["ape"] to always build one
//!
//! [build-dependencies]
//! cosmo-build = { version = "2", optional = true }
//!
//! # The shim has to be in the crate graph or the link fails with a hundred
//! # undefined __wrap_* references. cfg(cosmo) is set only by this crate, so
//! # ordinary builds neither resolve nor compile it.
//! [target.'cfg(cosmo)'.dependencies]
//! cosmo-compat = "2"
//!
//! [lints.rust]
//! unexpected_cfgs = { level = "allow", check-cfg = ['cfg(cosmo)'] }
//! ```
//!
//! ```no_run
//! // build.rs
//! fn main() {
//!    #[cfg(feature = "ape")]
//!    cosmo_build::apeify();
//! }
//! ```
//!
//! ```text
//! // src/main.rs
//! #[cfg(cosmo)]
//! extern crate cosmo_compat as _;
//! ```
//!
//! Then `cargo build -F ape` leaves the APE in `target/cosmo/<name>.com`, and a
//! plain `cargo build` is an ordinary host build that never compiles this crate.
//!
//! # What it needs
//!
//! Nothing installed up front beyond `rustup` and a shell. The first build
//! installs the pinned nightly (with `rust-src`) and downloads cosmocc
//! (~440MB, ~1.4GB unpacked) into a cache shared by every project, so a second
//! project costs no disk. `/bin/sh` is required because cosmocc's own tools are
//! APEs and cannot be `exec`'d any other way.
//!
//! # What gets downloaded
//!
//! Once, into a cache shared by every project; a warm build touches the network
//! not at all.
//!
//! * crates.io: this crate, `cosmo-compat`, and 45 transitive crates (~5MB),
//!   checksummed by cargo.
//! * static.rust-lang.org: the pinned nightly and `rust-src` via rustup, only
//!   if they are not already installed.
//! * cosmo.zip: `cosmocc-4.0.2.zip`, 441,763,966 bytes, ~1.4GB unpacked. This
//!   is the only URL the crate itself opens. It is pinned to a versioned
//!   release rather than the moving `cosmocc.zip`, and its SHA-256 is checked
//!   before anything is unpacked -- from a mirror too. A mismatch deletes the
//!   download and fails the build.
//!
//! # Environment
//!
//! | | |
//! |---|---|
//! | `COSMO_HOME` | move the cache (default `$XDG_CACHE_HOME/cargo-cosmo`) |
//! | `COSMO_APE=0` | never build an APE -- the setting for an editor |
//! | `COSMO_APE=1` | build one even when the heuristics say otherwise |
//! | `COSMO_RUSTFLAGS` | extra rustflags for the cosmo builds only |
//! | `COSMO_KEEP_PROFILE=1` | do not override `codegen-units`/`lto` |
//!
//! # Caveats
//!
//! Cargo captures build-script output, so the two-architecture build is silent
//! until it fails. The nested build needs its own target directory (~190MB)
//! because the outer cargo holds an exclusive lock on `target/`. And the APE
//! runs on Linux today: rustc bakes in Linux's OS constants while cosmopolitan
//! resolves them at load time, which the `cosmo-compat` shim translates at the
//! libc boundary -- see the repository for how far that goes.

use std::env;
use std::path::PathBuf;

mod cache;
mod driver;
mod pairing;
mod toolchain;

/// True when cargo is only type-checking, so no artifact will be produced.
///
/// `cargo check`, `cargo clippy` and rust-analyzer's check-on-save all run
/// build scripts, and an unconditional `cosmo-build` would hand each of them a
/// two-architecture build-std build -- once per save, in an editor. Cargo
/// exposes no flag for this, so it is inferred: clippy announces itself, and
/// cargo puts the artifact directory (`target/<profile>`) first on the dynamic
/// search path only for a build that will emit one; a check build starts at
/// `.../deps`. Verified by diffing the build-script environment of `build`,
/// `check`, `test` and `clippy` -- `LD_LIBRARY_PATH` is the only key that
/// differs between build and check, so this is undocumented cargo behaviour and
/// could change. It guards wasted work, never correctness: guess wrong and you
/// get a spurious APE, or none from a `cargo check` that would not have linked
/// anything anyway.
///
/// `cargo test` is byte-identical to `cargo build` here and cannot be
/// distinguished; set `COSMO_APE=0` if that matters.
pub fn is_metadata_only() -> bool {
   if env::var_os("CLIPPY_ARGS").is_some() {
      return true;
   }
   for key in ["LD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH", "DYLD_LIBRARY_PATH"] {
      if let Ok(path) = env::var(key) {
         return path.split(':').next().unwrap_or("").ends_with("/deps");
      }
   }
   false
}

/// Whether [`apeify`] will do anything: `COSMO_APE` wins if set, otherwise any
/// build that actually links.
pub fn wanted() -> bool {
   match env::var("COSMO_APE").as_deref() {
      Ok("0") | Ok("false") | Ok("no") => false,
      Ok(_) => true,
      Err(_) => !is_metadata_only(),
   }
}

/// Build this crate into `target/cosmo/<name>.com` unless this is a check-only
/// pass or the build script is re-entering itself. Panics with the failure if
/// the build breaks.
///
/// This is the one call a `build.rs` needs. Use [`build_ape`] to skip the
/// policy and always build.
pub fn apeify() {
   apeify_with(&[])
}

/// [`apeify`] with extra arguments handed to each `cargo build`.
///
/// A library crate has no bins, so nothing is produced unless a target is
/// named: `apeify_with(&["--example", "demo"])`. Anything `cargo build`
/// accepts works -- `--bin`, `--features`, `--no-default-features`.
///
/// ```no_run
/// // build.rs of a library whose demo lives in examples/demo.rs
/// fn main() {
///    #[cfg(feature = "ape")]
///    cosmo_build::apeify_with(&["--example", "demo"]);
/// }
/// ```
pub fn apeify_with(args: &[&str]) {
   // Cargo caches build-script runs, and any rerun-if-* directive REPLACES the
   // default "re-run when a package file changed". Declaring a path that never
   // exists is the only way to say "always run": without it a deleted APE is
   // never rebuilt and cargo still reports Finished.
   println!("cargo::rerun-if-changed=.cosmo-always-rerun");
   println!("cargo::rerun-if-env-changed=COSMO_APE");
   println!("cargo::rustc-check-cfg=cfg(cosmo)");

   // The nested build runs this script again; that pass must fall straight
   // through or the builds nest forever.
   if env::var_os("COSMO_APE_INNER").is_some() || !wanted() {
      return;
   }
   if let Err(e) = try_build_ape(args) {
      panic!("cosmo-build: {e}");
   }
}

/// Build the APE whatever cargo is doing, ignoring the [`wanted`] policy but
/// still declining to recurse. Panics on failure.
pub fn build_ape() {
   build_ape_with(&[])
}

/// [`build_ape`] with extra arguments handed to each `cargo build`.
pub fn build_ape_with(args: &[&str]) {
   if env::var_os("COSMO_APE_INNER").is_some() {
      return;
   }
   if let Err(e) = try_build_ape(args) {
      panic!("cosmo-build: {e}");
   }
}

/// The whole build, with the failure returned rather than raised.
///
/// Worth calling directly from a `build.rs` that wants to degrade instead of
/// break -- warn and carry on with the host build, say.
pub fn try_build_ape(args: &[&str]) -> Result<Vec<PathBuf>, String> {
   let manifest = PathBuf::from(
      env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is not set")?,
   );
   let release = env::var("PROFILE").as_deref() == Ok("release");

   pairing::check(&manifest)?;

   let cache = cache::Cache::locate()?;
   toolchain::ensure_rust()?;
   toolchain::ensure_cosmocc(&cache)?;
   driver::build(&cache, &manifest, release, args)
}

#[cfg(test)]
mod tests {
   /// The wrap list is inlined into the linker shim at asset-generation time,
   /// and the translators it names live in cosmo-compat. A symbol wrapped here
   /// with no translator there is a link error in every consuming project, so
   /// the two must be published in lockstep. Only runs from a repo checkout;
   /// the sibling crate is not in the published package.
   #[test]
   fn wrap_list_matches_cosmo_compat() {
      let Ok(list) = std::fs::read_to_string("../cosmo-compat/wrap.txt") else {
         return;
      };
      let ld = include_str!("../assets/cosmo-ld.sh");
      for sym in list.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
         assert!(
            ld.contains(&format!("--wrap={}", sym.trim())),
            "cosmo-compat wraps {sym:?} but the embedded linker shim does not; \
             regenerate crates/cosmo-build/assets"
         );
      }
   }
}
