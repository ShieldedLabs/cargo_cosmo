# Publishing to crates.io

Two crates go up, and they must go up together: `cosmo-build` embeds the list of
symbols the linker is told to `--wrap`, and `cosmo-compat` defines the
translators for exactly that list. A version of one paired with a different
version of the other is a link error in every consuming project.
`cosmo-build`'s test suite asserts the two agree; run it from a checkout.

## Before the first publish

* `cargo login` with an account that owns nothing yet -- both names were free as
  of this writing, and neither crate has been reserved.
* Decide whether to also take `cargo-cosmo` on crates.io. It is unclaimed, and
  the Rust driver now lives inside `cosmo-build`, so a real `cargo install
  cargo-cosmo` binary is a small step from here.

## Every publish

```sh
# 1. the embedded assets must match the tools they derive from
python3 tools/gen-assets.py

# 2. everything green, on a host that can run the APEs
./tests/run-all.sh
cargo test --manifest-path crates/cosmo-build/Cargo.toml

# 3. dry runs -- these do a real build of the packaged tarball
cargo package --manifest-path crates/cosmo-compat/Cargo.toml
cargo package --manifest-path crates/cosmo-build/Cargo.toml

# 4. compat first: cosmo-build's docs reference it, and a consuming project
#    resolves both from one lockfile
cargo publish --manifest-path crates/cosmo-compat/Cargo.toml
cargo publish --manifest-path crates/cosmo-build/Cargo.toml

# 5. tag, so the version is recoverable from the repo
git tag -a v4.0.0 -m 'cosmo-build/cosmo-compat 4.0.0' && git push --tags
```

## What docs.rs will show

Both crates build for the host, which is what docs.rs uses -- the interesting
targets are custom JSON specs it cannot build, so the pages document the API and
the setup rather than the cosmo-side behaviour. `cosmo-build`'s crate-level docs
carry the full manifest snippet a user needs, because that is the page they will
land on from crates.io.

## Versioning: majors only

Both crates carry the same version and only the major moves. `1.0.0`, then
`2.0.0`, then `3.0.0`. There are no minor or patch releases, and there is
never a version of one crate without the matching version of the other.

This is not the usual semver discipline, and it buys something specific here.
`cosmo-build` embeds the list of symbols the linker is told to `--wrap`;
`cosmo-compat` defines the `__wrap_*` translators for exactly that list. They
are one artifact split across two crates because cargo needs one on the host
and the other on the cosmo target. A user who ends up with mismatched halves
gets a hundred undefined symbols and no hint why. With majors only:

* a project writes `cosmo-build = "3"` and `cosmo-compat = "3"`, and there is
  exactly one version each of those can resolve to;
* mismatch is visible in the manifest, as two numbers that differ;
* nothing can drift underneath a project without the number changing.

The cost is that every release is a breaking release and users edit two numbers
to take it. For a crate that vendors a pinned compiler and a pinned nightly,
that is honest: any release that moves either of those *is* breaking.

`cosmo_build::apeify` checks the pairing before it builds anything. It reads
the resolved `Cargo.lock`, compares majors, and fails in a fifth of a second
with what to change -- rather than after a two-minute build, in linker output.
It catches the missing-dependency case with the same message, which is the
error a new user is most likely to hit.

## When cosmocc moves

The toolchain URL is pinned to a version *and* a SHA-256 in
`crates/cosmo-build/src/toolchain.rs`. Moving it:

```sh
curl -sO https://cosmo.zip/pub/cosmocc/cosmocc-<new>.zip
sha256sum cosmocc-<new>.zip          # -> COSMOCC_SHA256
./tests/run-all.sh                   # against the new toolchain, on a host that runs APEs
```

Then bump `COSMOCC_VERSION`, `COSMOCC_SHA256` and the major of both crates in
the same commit. A toolchain change is a breaking change: it is a different
compiler producing the user's binaries.
