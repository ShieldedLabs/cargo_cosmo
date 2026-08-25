# cosmo-build

`cargo build -F ape` → one file that runs on x86-64 and arm64.

Builds a Rust crate into a fat **Actually Portable Executable** from its
`build.rs`, linked against [Cosmopolitan Libc](https://justine.lol/cosmopolitan/).
Cargo has no concept of a multi-architecture artifact, so one `cargo build` can
never produce an APE: this runs the build once per architecture and fuses the
two ELFs with `apelink`, which is what cosmocc does for C.

```toml
[features]
ape = ["dep:cosmo-build"]      # or: default = ["ape"] to always build one

[build-dependencies]
cosmo-build = { version = "1", optional = true }

[target.'cfg(cosmo)'.dependencies]
cosmo-compat = "1"

[lints.rust]
unexpected_cfgs = { level = "allow", check-cfg = ['cfg(cosmo)'] }
```

```rust
// build.rs
fn main() {
   #[cfg(feature = "ape")]
   cosmo_build::apeify();
}

// src/main.rs
#[cfg(cosmo)]
extern crate cosmo_compat as _;
```

```
$ cargo build -F ape
$ ./target/cosmo/myprog.com
```

With the feature off, this crate is never fetched, compiled or run.

## What it needs

`rustup` and `/bin/sh`. Nothing else, and nothing installed up front — the first
build installs the pinned nightly with `rust-src` and downloads cosmocc (~440MB,
~1.4GB unpacked) into a cache shared by every project, so a second project costs
no disk. `/bin/sh` is unavoidable: cosmocc's own tools are APEs and the kernel
cannot `exec` one directly.

## What gets downloaded

Three things, once, then never again -- the cache is shared by every project on
the machine and nothing is fetched on a warm build.

| from | what | size | verified by |
|---|---|---|---|
| crates.io | `cosmo-build`, `cosmo-compat` and 45 transitive crates (`ureq`, `zip`, `sha2` and their trees) | ~5MB | cargo's registry checksums |
| static.rust-lang.org | the pinned nightly plus `rust-src`, via rustup, only if not already installed | ~1.6GB on disk | rustup's signed manifests |
| cosmo.zip | `cosmocc-4.0.2.zip` | 441,763,966 bytes (~1.4GB unpacked) | SHA-256 pinned in the crate |

`https://cosmo.zip/pub/cosmocc/cosmocc-4.0.2.zip` is the only URL this crate
itself will open, and it is pinned to a *versioned* release rather than the
moving `cosmocc.zip`, so a build in a year fetches the compiler these results
were measured against. Its SHA-256 is checked before anything is unpacked --
including from a mirror, since a mirror is only a different route to the same
bytes. A mismatch deletes the download and fails the build.

`COSMO_COSMOCC_URL` points the fetch at a mirror or an internal cache;
`COSMO_COSMOCC_SHA256` is how you run a different cosmocc deliberately.

## Environment

| | |
|---|---|
| `COSMO_HOME` | move the cache (default `$XDG_CACHE_HOME/cargo-cosmo`) |
| `COSMO_APE=0` | never build an APE — the setting for an editor |
| `COSMO_APE=1` | build one even when the heuristics say otherwise |
| `COSMO_COSMOCC_URL` | fetch cosmocc from a mirror |
| `COSMO_COSMOCC_SHA256` | expect a different cosmocc |
| `COSMO_RUSTFLAGS` | extra rustflags for the cosmo builds only |
| `COSMO_KEEP_PROFILE=1` | do not override `codegen-units`/`lto` |

## Caveats

* Cargo captures build-script output, so the two-architecture build is **silent
  until it fails**.
* The nested build needs its own target directory (~190MB) because the outer
  cargo holds an exclusive lock on `target/`.
* `cargo check` and `cargo clippy` are detected and skipped; `cargo test` is
  indistinguishable from `cargo build` in a build script's environment and will
  build an APE. Set `COSMO_APE=0` where that matters.
* The APE is a genuine fat binary but **runs on Linux today** — rustc bakes in
  Linux's OS constants while cosmopolitan resolves them at load time. The
  `cosmo-compat` shim translates 123 libc entry points at the boundary, and
  macOS/arm64 is measured working; the rest is untested. See the
  [repository](https://github.com/ShieldedLabs/cargo_cosmo).

MIT licensed.
