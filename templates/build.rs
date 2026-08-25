// Build this crate into a fat Actually Portable Executable as a side effect of
// `cargo build`, by re-entering cargo once per architecture and apelinking the
// two halves. Pair it with a copy of crates/cosmo-build/bootstrap.sh saved as
// build.sh next to it; this file is only the trigger and the guardrails.
//
// If you can depend on this repo, use the cosmo-build crate instead: the same
// logic, one line in build.rs, nothing vendored to drift.
//
//    cargo build                 ordinary host build, untouched
//    cargo build -F ape          host build + target/cosmo/<profile>/<name>.com
//
// Four things here are not optional; each one is a silent failure without it.
use std::{env, process::Command};

fn main() {
   // Cargo caches build-script runs, and any rerun-if-* directive REPLACES the
   // default "re-run when a file in the package changed". Declaring a path
   // that never exists is the only way to say "always run": without it a
   // deleted APE is never rebuilt, and cargo still reports Finished.
   println!("cargo::rerun-if-changed=.cosmo-always-rerun");
   println!("cargo::rustc-check-cfg=cfg(cosmo)");
   println!("cargo::rerun-if-env-changed=COSMO_APE");

   // cargo-cosmo re-enters cargo on this same package, which runs this script
   // again. Without the inner flag that recurses forever.
   if env::var_os("CARGO_FEATURE_APE").is_none() || env::var_os("COSMO_APE_INNER").is_some() {
      return;
   }

   // `cargo check` and `cargo clippy` run build scripts too, so without this an
   // editor's check-on-save hands you a two-architecture build per keystroke.
   // Cargo exposes no flag for it: clippy announces itself, and cargo puts the
   // artifact dir first on the dynamic search path only for a build that emits
   // one -- a check build starts at .../deps. Undocumented, and a heuristic
   // about wasted work rather than correctness. COSMO_APE=0 forces it off.
   let checking = env::var_os("CLIPPY_ARGS").is_some()
      || env::var("LD_LIBRARY_PATH").or_else(|_| env::var("DYLD_FALLBACK_LIBRARY_PATH"))
            .map(|p| p.split(':').next().unwrap_or("").ends_with("/deps"))
            .unwrap_or(false);
   let off = matches!(env::var("COSMO_APE").as_deref(), Ok("0") | Ok("false") | Ok("no"));
   if off || (checking && env::var_os("COSMO_APE").is_none()) {
      return;
   }

   let dir = env!("CARGO_MANIFEST_DIR");
   let profile = env::var("PROFILE").unwrap_or_default();

   let mut cmd = Command::new("sh");
   cmd.arg(format!("{dir}/build.sh"));
   if profile == "release" { cmd.arg("--release"); }

   let ok = cmd
      .env("COSMO_APE_INNER", "1")
      // The outer cargo holds an exclusive lock on target/. A nested cargo
      // against the same directory blocks on it until it is killed.
      .env("CARGO_TARGET_DIR", format!("{dir}/target/cosmo-inner"))
      // The outer cargo also pins its own toolchain into the environment.
      // Inherited, these override the nested `cargo +nightly` and the inner
      // build dies with "1 nightly option were parsed" from a stable rustc.
      .env_remove("RUSTC").env_remove("RUSTDOC").env_remove("RUSTC_WRAPPER")
      .env_remove("RUSTC_WORKSPACE_WRAPPER").env_remove("RUSTUP_TOOLCHAIN")
      .env_remove("CARGO").env_remove("RUSTFLAGS")
      .env_remove("CARGO_ENCODED_RUSTFLAGS").env_remove("CARGO_MAKEFLAGS")
      .status()
      .expect("could not run build.sh")
      .success();

   // Cargo captures build-script output and shows it only on failure, so the
   // whole cosmo build is silent until something breaks. Nothing to be done
   // about that from in here -- it is the cost of this route.
   assert!(ok, "cosmo build failed");
}
