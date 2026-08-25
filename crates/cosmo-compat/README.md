# cosmo-compat

Libc shims for the places where [Cosmopolitan Libc](https://justine.lol/cosmopolitan/)
and Rust's `std` disagree. Link it into a cosmo target and the fixes apply
themselves — each is a `#[no_mangle]` definition the linker prefers over the one
in `libcosmo.a`, because an archive member is only pulled in to satisfy a symbol
that is still undefined.

It is not optional. Cosmopolitan declares
`char *__xpg_strerror_r(int, char *, size_t)` — the GNU contract — while Rust's
std links that same symbol expecting the POSIX XSI one, which returns `int`. std
reinterprets the returned pointer as an int, finds the sign bit set, and panics
with `strerror_r failure`. The practical effect is that formatting *any*
`io::Error` aborts the program, so ordinary error paths break long before
anything exotic does.

The crate also carries the stage-2 normalization shim: cosmopolitan resolves OS
constants at load time while rustc bakes in Linux's values at compile time, so
123 libc entry points are `--wrap`ped and translated at the boundary.

## Use

Paired with [`cosmo-build`](https://crates.io/crates/cosmo-build), which sets
`cfg(cosmo)` and adds the `--wrap` flags:

```toml
[target.'cfg(cosmo)'.dependencies]
cosmo-compat = "3"
```

```rust
#[cfg(cosmo)]
extern crate cosmo_compat as _;
```

The `cosmo-compat` and `cosmo-build` versions must match: the wrap list the
linker is given lives in `cosmo-build`, and a symbol wrapped without a
translator is a link error.

See the [repository](https://github.com/ShieldedLabs/cargo_cosmo) for how the
constant tables are extracted and what is verified on which OS.

MIT licensed. Embeds constant tables extracted from Cosmopolitan Libc (ISC) —
see NOTICE in the repository.
