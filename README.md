# cargo_cosmo

`cargo cosmo build` -> one file that runs on x86-64 and arm64.


Build Rust programs into **Actually Portable Executables** — one file containing
both an x86-64 and an arm64 build, linked against Cosmopolitan Libc.

```
$ cd examples/rust-std
$ cargo cosmo build --release
cargo-cosmo: building x86_64
cargo-cosmo: building aarch64
cargo-cosmo: target/cosmo/stdhello.com  (1,439,666 bytes, x86_64+aarch64)

$ ./target/cosmo/stdhello.com
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
| Runs on non-Linux OSes | **shim built, unrun there** — std's libc calls are translated at the boundary (`crates/cosmo-compat`, see [docs/DESIGN.md](docs/DESIGN.md) "Stage 2 as built"); its logic is tested against the Windows and macOS constant tables on Linux, the binaries have not yet been run on either |

`tests/run-all.sh` builds and runs all of it: 11 checks, no mocks.

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
```

## Setup

```sh
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly

# ~440MB, not in git
mkdir -p toolchain && cd toolchain
curl -sSLo cosmocc.zip https://cosmo.zip/pub/cosmocc/cosmocc.zip
mkdir -p cosmocc && cd cosmocc && unzip -q ../cosmocc.zip

cd ../.. && python3 tools/gen-target-specs.py
export PATH="$PWD/tools:$PATH"
./tests/run-all.sh
```

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
