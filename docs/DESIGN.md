# Porting Rust to Cosmopolitan Libc

## The problem in one paragraph

Rust binds OS-dependent constants at *compile* time. `libc::EINVAL` is a `const i32`
chosen by `target_os`. Cosmopolitan binds them at *load* time: it declares
`extern const int EINVAL;` and fills the value in once the running kernel is known.
These two models are directly opposed, and the collision point is pattern matching —
`match err { libc::EINVAL => ... }` requires a compile-time constant, so a late-bound
`extern` symbol cannot be used there at all.

C hit the same wall. Cosmopolitan's answer was a ~2kLOC GCC patch that rewrites
`switch (x) { case EINVAL: }` into an if-chain. Rust cannot copy that: const patterns
are load-bearing for exhaustiveness checking, match lowering, and const eval. Patching
rustc means forking rustc forever.

## The two stages

This repo separates what is *achievable now* from what requires the deep work, because
conflating them is how this project would die.

### Stage 1 — Linux-only APE (mechanical, achievable)

Take the existing Rust `linux-musl` machinery, point the linker at `cosmocc`, and let
`std` link against `cosmopolitan.a` instead of musl. The syscalls resolve, threads work,
files work.

The binary is a genuine APE — a real fat x86_64+aarch64 polyglot — but the OS constants
baked into it are Linux's, so it only *runs* on Linux. You get one file for two
architectures, not one file for every OS.

This is what `cargo-cosmo` in this repo builds today. It is honestly useful (one artifact
for arm64 and x64 Linux) and it is the foundation the next stage stands on.

### Stage 2 — Normalization (the actual port)

The insight that makes the real port tractable: **do not propagate cosmo's late-bound
constants into Rust. Normalize instead.**

Declare the Rust cosmo target to have *fixed canonical* constant values — adopt Linux's
numbering — and translate at the libc boundary:

    canonical value --> [shim] --> host-native value    (arguments in)
    host-native value --> [shim] --> canonical value    (returns out)

This restores compile-time constants. `match` works. Exhaustiveness works. The entire
crates.io ecosystem compiles unmodified, because from Rust's point of view the constants
are as static as they have always been.

Two facts make this much cheaper than it sounds:

1. Cosmopolitan **already normalizes struct layouts** — it defines its own `struct stat`,
   `struct sockaddr` and converts internally per host. The shim therefore only has to
   translate *scalars*: errno, signal numbers, `O_*`, `PROT_*`/`MAP_*`, socket options,
   and a handful of ioctls.

2. Those tables are machine-readable in cosmopolitan's own `libc/sysv/consts.sh`. The
   shim is largely **generated**, not hand-written.

The price, stated plainly: anything that bypasses cosmo libc breaks. `rustix`'s
`linux_raw` backend must be forced off. Any crate issuing raw syscalls, or reading
`/proc`, is out.

## How big is the problem, measured

`tools/extract-syscon.py` reads the whole table out of `libcosmo.a` and diffs
each OS against Linux:

    constants extracted: 1591      groups: 65

    os          differs   absent   total  % differ
    xnu             987      240    1591     77.1%
    freebsd         993      234    1591     77.1%
    openbsd         922      259    1591     74.2%
    netbsd          952      256    1591     75.9%
    windows         781      336    1591     70.2%

    worst groups (non-Linux pooled):
      nr        332/332   errno    109/110   termios  94/109
      misc       93/103   ioctl     63/63    ptrace   50/50

Two things fall out of this. First, ~75% divergence confirms that a Rust binary
carrying Linux constants cannot work on another OS -- this is not a long-tail
problem that could be patched case by case.

Second, the single worst group is `nr` (332 of 332 diverge) -- raw syscall
numbers. Those are exactly what Rust never touches, because std and `libc` go
through cosmo's function-call interface rather than issuing syscalls directly.
The worst part of the table is free, which is the same fact that makes the
"ban raw-syscall backends" constraint cheap to accept.

The shim therefore has to cover roughly 1250 non-`nr` constants across 64
groups, with the values already machine-extracted in `data/syscon-*.json`.

## The primitive, verified

`examples/syscon-probe` proves the mechanism from the Rust side: cosmo's
constants are ordinary linkable symbols living in BSS, filled in by its runtime
before `main`, so Rust can simply declare and read them:

    extern "C" { static EINVAL: core::ffi::c_int; }

On Linux every probed constant matches the value rustc baked in, which is both
the expected result and a check that the target spec is not lying about its OS.
That read is the hinge the whole translation layer turns on.

## Why this is a different bet than C made

Justine chose host-native values for C because C code passes constants straight through
to the host and the GCC patch made `switch` work. Rust cannot have the patch, but Rust
also has something C does not: nearly all Rust code reaches the OS through `libc` or
`rustix` rather than hand-rolled syscalls. That chokepoint is exactly what makes a
translation shim viable. The property that blocks one solution enables the other.

## Work breakdown, and what is already done

| # | Item | Status |
|---|------|--------|
| 1 | Target specs `{x86_64,aarch64}-unknown-cosmo` | **done** — generated in `targets/`. Out-of-tree JSON specs; upstreaming as tier-3 would start from a `#![no_core]` spec to break the rustc<->libc<->std cycle |
| 2 | Normalization shim | **built, unrun off-Linux** — `crates/cosmo-compat` wraps std's 123 libc imports via `ld --wrap` (list in `wrap.txt`, added by `cosmo-ld`); `tools/gen-shim.py` generates the tables from `data/syscon-*.json`. Translation logic is unit-tested on Linux against the table's Windows and XNU columns; the runtime path is verified on Linux only, where it is the identity. See "Stage 2 as built" below |
| 3 | `std::sys::pal::cosmo` | **not started** — fork of `pal/unix` with the cfg forest collapsed to cosmo's everywhere-POSIX subset: `poll` not `epoll`/`kqueue`, pthreads, no `/proc`, no `statx` |
| 4 | Unwinding | **done, and easier than expected** — `libcosmo.a` defines the full `_Unwind_*` ABI and both linker scripts emit `.eh_frame_hdr`/`.gcc_except_table`, so `panic=unwind` and `catch_unwind` work today. The "cosmo has no unwinder" limitation in the older prior art is stale |
| 5 | `cargo-cosmo` | **done** — builds both arches and runs `apelink`; cargo itself has no multi-arch output concept |
| 6 | Ecosystem cfg arms | **not started** — `getrandom`, `mio`, `socket2`, `parking_lot`. `getrandom` and `mio` are what gate tokio |
| 7 | Cross-OS test matrix | **not started** — booting artifacts on Linux/macOS/Windows/FreeBSD. Nothing in Rust CI does this; without it the target silently rots |

Items 1, 4 and 5 are what "one fat APE per crate, Linux-only" needed. Items 2 and 3
are what "runs on every OS" needs, and they are the months of work. Tokio-class
async is beyond even that.

## Stage 2 as built

Not a `libc` crate fork. std keeps calling `open`, `socket`, `poll` … by name with
Linux-numbered arguments; `cosmo-ld` links every program with `-Wl,--wrap=NAME`
for each name in `crates/cosmo-compat/wrap.txt`, so those calls land in
`__wrap_NAME` in `cosmo-compat/src/shim.rs`, which translates arguments to the
host's numbering, calls `__real_NAME` (cosmo's), and translates errno, `revents`,
`sa_family`, wait statuses and option results back. The host values are cosmo's
own load-time symbols (`extern const int EAGAIN` etc.), read at run time — the
binary carries no per-OS table, and the JSON columns exist only so the logic can
be tested on a Linux host (`cargo test` in `crates/cosmo-compat`, Windows column
by default, `COSMO_TEST_HOST=xnu` for macOS).

What the table says about Windows specifically: cosmo keeps Linux numbering there
for `O_*`, `MAP_*`, `SIG*`, `F_*`, `AT_*`, `CLOCK_*`, `SOCK_*`. What diverges is
errno (WinSock/Win32 numbers), the socket option layer (`AF_INET6`, `SOL_SOCKET`,
`SO_*`, `MSG_NOSIGNAL`, `IPV6_*`, `IP_*`), `POLL*`, `FIONBIO`. So on Windows most
groups are the identity and the shim's real work is errno and sockets. XNU
diverges far more widely (signals, `O_*`, `AT_FDCWD`, `CLOCK_*`) and the same
generated tables cover it.

Two properties the design depends on, both deliberate:

* **Every wrapper is heap-free and lock-free, and is the identity until cosmo's
  tables are filled.** `--wrap` also captures cosmopolitan's own startup calls
  (its first `open`/`mmap`), before std's runtime exists. A first version used
  `OnceLock` and segfaulted there.
* **Wrapped functions taking only integer/pointer arguments are wrapped by a
  generated six-register passthrough** rather than a typed signature each: SysV
  and AAPCS pass those in the first integer registers regardless of declared
  type, so an untyped forwarder is exact. Functions whose arguments carry
  constants are written by hand.

Known hazard: because `--wrap` redirects cosmo-internal calls too, a cosmo
routine calling public `open(O_CLOEXEC)` with a HOST value gets re-translated.
Harmless on Windows (identity for those groups), real on XNU. The fix if it
bites is to wrap only std's objects (`objcopy --redefine-sym` on the rlibs).

Not translated: `struct stat` layout (cosmo's is the x86-64 Linux/musl layout on
both arches, so the aarch64 half's std, built for musl-aarch64's generic layout,
may read `st_*` fields wrongly — unverified), `dirent`, `ioctl` argument
structures, and anything std reaches through `syscall(2)` directly (it only
does so for Linux-specific features it already falls back from).

## What cosmo does and does not give you

Cosmopolitan gives you **libc portability**, not **platform portability**. There is no
windowing abstraction, no GUI layer, no graphics. A portable GUI app still needs
Win32/Cocoa/X11 backends behind `cfg(target_os)` at the application layer, and at that
point the single-binary property has bought you very little.

Good fits: CLI tools, servers, compilers, anything whose entire interface is stdio,
files, and sockets.
