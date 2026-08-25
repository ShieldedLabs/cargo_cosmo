# cargo_cosmo

`cargo cosmo build` -> one file that runs on x86-64 and arm64.


Build Rust programs into **Actually Portable Executables** — one file containing
both an x86-64 and an arm64 build, linked against Cosmopolitan Libc.

```
$ cd examples/rust-std
$ cargo cosmo build --release
cargo-cosmo: building x86_64
cargo-cosmo: building aarch64
cargo-cosmo: target/cosmo/release/stdhello.com  (1,439,666 bytes, x86_64+aarch64)

$ ./target/cosmo/release/stdhello.com
hello from a std Rust APE
heap  = works
thread sum 1..10 = 55
file  = file io works
catch_unwind caught = true
```

## What works

| | status |
|---|---|
| `no_std` Rust → cosmopolitan | works |
| **full `std`** — stdio, heap, threads, files, time | works |
| **`panic = "unwind"`**, `catch_unwind`, panic messages | works |
| x86-64 build | works, executed in CI script |
| arm64 build | builds; verified structurally, not executed (no emulator on this host) |
| **fat x86-64 + arm64 APE** | works, `pecheck`-clean |
| `io::Error` formatting | works — needs the `cosmo-compat` shim, see below |
| Runs on macOS | **yes** — measured on macOS 15.7.2 / arm64: files, dirs, threads, time, TCP and unwinding all work through the stage-2 shim, and a full GUI program runs from the same file. See below |
| Runs on Windows / the BSDs | shim covers them, still unrun — `examples/cross-os-probe` is the way to find out |

`tests/run-all.sh` builds and runs all of it: 13 checks, no mocks. It is
host-aware — on a Mac it swaps the Linux-only invariants for the ones that hold
there, and passes 13/13.

Notably, `panic = "unwind"` works. The prior art in this space documents cosmo as
having no unwinder; that is now stale. `libcosmo.a` defines the complete
`_Unwind_*` ABI and both linker scripts emit `.eh_frame_hdr` and
`.gcc_except_table`.

## crates/cosmo-compat

One shim, and it is not optional. Cosmopolitan declares
`char *__xpg_strerror_r(int, char *, size_t)` — the GNU contract — while Rust's
std links that same symbol expecting the POSIX XSI one, which returns `int`.
std reinterprets the returned pointer as an int, finds the sign bit set, and
panics with `strerror_r failure`. The practical effect is that formatting *any*
`io::Error` aborts the program, so ordinary error paths break long before
anything exotic does. `cosmo-compat` redefines the symbol with XSI semantics;
the linker prefers it because an archive member is only pulled in to satisfy a
still-undefined symbol.

Depend on it from any cosmo target:

```toml
[dependencies]
cosmo-compat = { path = "../../crates/cosmo-compat" }
```

```rust
extern crate cosmo_compat as _;
```

## Testing on macOS and Windows

`examples/cross-os-probe` is built for exactly this. Copy the fat APE to a Mac
or a Windows box and run it — no install, no runtime.

It is self-checking. Because `data/syscon-*.json` holds cosmo's value for every
constant on every OS, the probe knows what each one *should* become on whatever
host it lands on, and prints three columns: what cosmo resolved at load time,
what rustc baked in, and what the table predicted.

* `cosmo != rustc` is a live portability bug — a constant std is now passing wrongly.
* `cosmo != predict` would mean the extractor is wrong.

It then walks std stage by stage (heap, threads, files, dirs, time, tcp, panic),
flushing after each, so a hard crash still says where it died.

x86-64 cosmo supports every OS (`SUPPORT_VECTOR 255`); arm64 covers
Linux/macOS/FreeBSD. So an Apple Silicon Mac exercises the arm64 half and a
Windows PC the x86-64 half — the two halves this repo cannot execute on a Linux
x86-64 host.

### What the Mac said

Run on macOS 15.7.2 / arm64. cosmo reports `xnu (macOS)` while rustc still says
`os=linux`; of 47 probed constants **24 are identical, 22 diverge, 0 are
unexpected**, so the extractor predicted every XNU value correctly. With the
stage-2 shim in the path, std works there: stdout, heap, `env::args`, time,
threads, file write+read, dir listing, `panic` + `catch_unwind`, and a
`connect` to a closed port reports `ConnectionRefused` rather than a
mistranslated errno. `tests/run-all.sh` is 13/13 on that host.

Running it found two holes in the shim, both fixed:

* `clock_gettime` and `clock_nanosleep` were generated as errno-only
  passthroughs, but their first argument is a `clockid_t` and
  `CLOCK_MONOTONIC` is 1 on Linux, 8 on XNU. The first `Instant::now()` on a
  Mac died with EINVAL.
* The double-translation hazard is real on XNU. cosmo's own `open()` forwards
  to `openat()`, `--wrap` catches that internal call too, and the flags were
  translated twice — `File::create` failed with ENOENT. `__wrap_open` now goes
  straight to the leaf.

Two build settings also have to be avoided, and `cargo-cosmo` overrides them:
`codegen-units = 1` hangs `JoinHandle::join`, and `lto = "fat"` corrupts the
caller of `thread::spawn`. Related trap while debugging either: std's
stack-overflow handler is called with a NULL `siginfo_t` under cosmo, so it
faults inside the SIGSEGV handler and the process wedges instead of dying —
the hang appears nowhere near the fault.

A real GUI program now runs from one APE on macOS:
[softer_gui](https://github.com/ShieldedLabs/softer_gui) — window, software
renderer and vblank-exact pacing, from the same file that runs on Linux.

## The catch, stated plainly

The binary is a genuine fat APE, but it only **runs on Linux**.

Rust resolves OS constants at compile time (`libc::EINVAL` is a `const`), while
cosmopolitan resolves them at load time. The target spec here says `linux`, so
Linux's values get baked in, and on any other OS they are wrong.

This is measured, not assumed. `tools/extract-syscon.py` pulls cosmopolitan's
entire per-OS constant table straight out of `libcosmo.a`:

```
constants extracted: 1591      groups: 65

os          differs   absent   total  % differ
xnu             987      240    1591     77.1%
freebsd         993      234    1591     77.1%
openbsd         922      259    1591     74.2%
netbsd          952      256    1591     75.9%
windows         781      336    1591     70.2%
```

Roughly three quarters of every system constant changes value off Linux. Closing
that gap is the real port — see [docs/DESIGN.md](docs/DESIGN.md) for the
normalization approach and a work breakdown.

## Layout

```
targets/     generated target specs (gitignored: they embed an absolute
             linker path, so cargo-cosmo regenerates them on demand)
tools/
  gen-target-specs.py  derive specs from rustc's own musl specs
  cosmo-ld             linker shim: reconciles rustc's link line with cosmo's
  cargo-cosmo          build driver: both arches + apelink
  extract-syscon.py    pull cosmo's per-OS constant table out of libcosmo.a
  gen-probe.py         generate the cross-OS probe from that table
  check-fat.py         assert an APE carries both architectures
  check-regs.py        assert rustc never allocated cosmo's reserved x18/x28
data/        extracted constant tables (1591 constants x 6 OSes)
crates/
  cosmo-compat         libc shims where cosmo and std disagree
examples/    c-hello, rust-nostd, rust-std, syscon-probe, cross-os-probe
tests/       run-all.sh
crates/
  cosmo-build          build-dependency: the whole APE build from a build.rs
templates/   build.sh, build.rs -- drop-in bootstrap, no dependency on the crates
```

## Setup

```sh
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly

# ~440MB, not in git
mkdir -p toolchain && cd toolchain
curl -sSLo cosmocc.zip https://cosmo.zip/pub/cosmocc/cosmocc.zip
mkdir -p cosmocc && cd cosmocc && unzip -q ../cosmocc.zip

cd ../..

# cosmocc ships its tools as APEs, and cosmocc's gcc spawns `ld` and `as` with
# posix_spawnp, which cannot exec one. Without this the link step fails, rustc
# reports it as a warning, and cargo says "Finished" having built nothing.
./tools/assimilate-toolchain.sh

python3 tools/gen-target-specs.py
export PATH="$PWD/tools:$PATH"
./tests/run-all.sh
```

On WSL, note that running an APE by path hands it to Windows: binfmt_misc
registers WSLInterop for PE images and an APE is one, so `./foo.com` starts a
*Windows* process and any "Linux" test silently measures the wrong OS.
`tests/run-all.sh` and `cargo cosmo run` detect this and go through
`toolchain/cosmocc/bin/ape-$(uname -m).elf`; do the same by hand.

## Using it on your own project

Nothing has to be installed first. Whichever route you pick, the first build
fetches these tools, the pinned nightly (`rust-src` included) and cosmocc into
a shared cache outside the tree, so a second project costs no disk.
`COSMO_HOME=<dir>` moves the cache, `COSMO_REF=<tag|sha>` pins the tools.

One thing is common to every route: the shim has to be in the crate graph, or
the link fails with a hundred undefined `__wrap_*` references. Scoping it to
`cfg(cosmo)`, which `cargo-cosmo` sets, keeps ordinary builds untouched.

```toml
[target.'cfg(cosmo)'.dependencies]
cosmo-compat = "5"

[lints.rust]                                    # optional: quiet the warning
unexpected_cfgs = { level = "allow", check-cfg = ['cfg(cosmo)'] }
```

```rust
#[cfg(cosmo)]
extern crate cosmo_compat as _;
```

### As a feature flag

`crates/cosmo-build` makes the APE a build of its own. The build-dependency is
optional, so with the feature off it is never even compiled:

```toml
[features]
ape = ["dep:cosmo-build"]      # or: default = ["ape"] to always build one

[build-dependencies]
cosmo-build = { version = "5", optional = true }
```

```rust
// build.rs
fn main() {
   #[cfg(feature = "ape")]
   cosmo_build::apeify();
}
```

```
cargo build           ordinary host build
cargo build -F ape    host build + target/cosmo/<profile>/<name>.com
```

Drop the feature and the `optional` and every `cargo build` produces an APE --
that is the whole of an "always APE" project, no features table at all.

Two catches, both inherent to build scripts. Cargo captures their output, so
the two-architecture build is silent until it fails, and the nested build needs
its own target directory (~190MB) because the outer cargo holds an exclusive
lock on `target/`. And `cargo check`, `cargo test` and `cargo clippy` all run
build scripts, so an always-on project would build an APE on every check-on-save.
`apeify` therefore skips check and clippy builds -- see `is_metadata_only`,
which infers it because cargo exposes no flag -- and `COSMO_APE=0` turns it off
wholesale for an editor that guesses wrong. `cargo test` is indistinguishable
from `cargo build` in a build script's environment, so that one still builds.
Call `build_ape` instead of `apeify` to skip the policy entirely.

rust-analyzer is covered by that: both the command it runs to discover build
scripts and its check-on-save default are `cargo check` invocations, which the
detector sees. It is not covered if you have pointed
`rust-analyzer.cargo.buildScripts.overrideCommand` or `check.overrideCommand`
at something built on `cargo build` -- that is indistinguishable from a real
build by construction. For an always-on project, the belt-and-braces setting is

```json
"rust-analyzer.cargo.extraEnv": { "COSMO_APE": "0" }
```

which is a switch rather than a heuristic.

Rebuilds are incremental: each architecture keeps its own incremental cache in
the nested target dir, std is compiled once, and a one-line edit relinks both
halves and the APE in about a second. Every `cargo build` pays roughly that
much even when nothing changed, because the build script has to re-run to
notice.

The crate needs `rustup` and `/bin/sh` and nothing else: the toolchains are
fetched with `ureq` and unpacked with the `zip` crate rather than by shelling
out, so no python, git, curl or unzip has to exist on the build host. `/bin/sh`
is unavoidable -- cosmocc's own tools are APEs and the kernel cannot exec one
directly.

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

### A library crate

An APE is a program, and a library has none, so it names the one the APE is
built from. The build script cannot learn this from the command line -- cargo
hands an identical environment to `cargo build` and `cargo build --example
demo` -- so it is declared next to the crate's other target declarations, where
cargo ignores it:

```toml
[package.metadata.cosmo]
example  = "demo"              # or ["demo", "bench"]
bin      = "tool"              # or [...]
features = ["gui", "x11"]
args     = ["--no-default-features"]
```

`build.rs` stays `cosmo_build::apeify()`. Where the choice has to be computed
rather than declared, `apeify_with(&["--example", "demo"])` takes the same
arguments directly and the manifest table is not consulted.

### As a script

`templates/build.sh` is the same bootstrap as a shell script, for a project that
would rather not take a build-dependency -- and it lets you watch the build,
which the crate route cannot. It clones this repo and drives the Python tools in
`tools/`, so unlike the crate it does want git, curl, unzip and python3:

```sh
curl -sSLo build.sh https://raw.githubusercontent.com/ShieldedLabs/cargo_cosmo/main/templates/build.sh
chmod +x build.sh
./build.sh --release      # -> target/cosmo/release/<name>.com
```

`templates/build.rs` is the middle option: the feature-flag behaviour with the
guardrails vendored, for a project that wants `cargo build` to be the only
command but no dependency on this repo.

There is no `cargo install` step in any of them. The driver resolves its own
repo from its path, so `PATH="$COSMO_HOME/tools:$PATH"; cargo cosmo build`
works too, in any directory, once the cache exists.

## Design notes worth knowing

Three things were non-obvious enough to be worth recording:

**Thread-locals are off.** cosmocc compiles with `-mno-tls-direct-seg-refs`,
which rustc does not expose as a target feature. Rather than emit native ELF TLS
that cosmo's loader cannot service, the specs set `has-thread-local: false` so
std falls back to `pthread_key_create`, which cosmo does implement.

**arm64 reserves x18 and x28.** cosmo uses x18 as its thread pointer and reserves
x28. rustc allocating either as a scratch register is silent memory corruption,
not a link error, so the aarch64 spec sets `+reserve-x18,+reserve-x28` and
`tools/check-regs.py` asserts it on every build.

**Cosmo's own tools are APEs.** `apelink`, `pecheck` and the binutils cannot be
`exec`'d directly — no ELF magic, no shebang. A shell handles it because the APE
header is also valid `/bin/sh`; `posix_spawn` returns `ENOEXEC`. Both Python
drivers sniff the magic and route accordingly.
