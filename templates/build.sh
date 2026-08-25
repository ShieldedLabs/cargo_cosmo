#!/bin/sh
# Build this crate into a fat Actually Portable Executable.
#
# Drop this one file into a Rust project and run it. Nothing has to be
# installed first: it fetches the cargo_cosmo tools, the pinned nightly and
# cosmopolitan's toolchain on demand, then hands off to `cargo cosmo`.
#
#    ./build.sh              debug APE   -> target/cosmo/debug/<name>.com
#    ./build.sh --release    release APE
#    ./build.sh run -- ...   build and run it
#
# Everything downloaded is shared between projects and cached outside the tree,
# so a second project costs no disk. Override with COSMO_HOME=<dir>, and pin
# the tools with COSMO_REF=<tag|sha> (default: main).

set -e

REPO_URL=${COSMO_REPO:-https://github.com/ShieldedLabs/cargo_cosmo.git}
REF=${COSMO_REF:-main}
HOME_DIR=${COSMO_HOME:-${XDG_CACHE_HOME:-$HOME/.cache}/cargo-cosmo}
COSMOCC_URL=${COSMOCC_URL:-https://cosmo.zip/pub/cosmocc/cosmocc.zip}

say() { echo "build.sh: $*" >&2; }
die() { say "$*"; exit 1; }

# 1. The tools. A shallow clone at a fixed ref -- not a submodule, so the
#    consuming project's history stays its own.
if [ ! -x "$HOME_DIR/tools/cargo-cosmo" ]; then
   say "fetching cargo_cosmo ($REF) into $HOME_DIR"
   command -v git >/dev/null || die "git not found"
   rm -rf "$HOME_DIR"
   git clone -q --depth 1 --branch "$REF" "$REPO_URL" "$HOME_DIR" 2>/dev/null ||
      { git clone -q "$REPO_URL" "$HOME_DIR" && git -C "$HOME_DIR" checkout -q "$REF"; } ||
      die "could not clone $REPO_URL"
fi

# 2. The nightly. Read the pin out of the tools rather than duplicating it here;
#    the consuming project keeps whatever toolchain it already uses, and only
#    the cosmo builds are forced onto this one.
CHANNEL=$(sed -n 's/^ *channel *= *"\(.*\)"/\1/p' "$HOME_DIR/rust-toolchain.toml")
[ -n "$CHANNEL" ] || die "no channel in $HOME_DIR/rust-toolchain.toml"
if ! rustup run "$CHANNEL" rustc -V >/dev/null 2>&1; then
   say "installing $CHANNEL (+ rust-src)"
   rustup toolchain install -q --profile minimal -c rust-src "$CHANNEL"
elif ! rustup component list --toolchain "$CHANNEL" --installed 2>/dev/null | grep -q '^rust-src'; then
   say "adding rust-src to $CHANNEL"
   rustup component add -q --toolchain "$CHANNEL" rust-src
fi
export CARGO_COSMO_TOOLCHAIN=$CHANNEL

# 3. Cosmopolitan's toolchain: ~440MB down, ~1.4GB unpacked, so it is fetched
#    once per cache and never per project.
CC_DIR="$HOME_DIR/toolchain/cosmocc"
if [ ! -f "$CC_DIR/bin/apelink" ]; then
   say "downloading cosmocc (~440MB, one time)"
   command -v unzip >/dev/null || die "unzip not found"
   mkdir -p "$CC_DIR"
   ZIP="$HOME_DIR/toolchain/cosmocc.zip"
   if command -v curl >/dev/null; then curl -fSL# -o "$ZIP" "$COSMOCC_URL"
   elif command -v wget >/dev/null; then wget -q --show-progress -O "$ZIP" "$COSMOCC_URL"
   else die "need curl or wget"; fi
   unzip -q -o "$ZIP" -d "$CC_DIR"
   rm -f "$ZIP"
   [ -f "$CC_DIR/bin/apelink" ] || die "cosmocc unpacked but bin/apelink is missing"
fi

# `./build.sh --release` should mean `build --release`; `./build.sh run ...`
# and `clean` pass straight through.
case "$1" in
   build|run|clean) ;;
   *) set -- build "$@" ;;
esac

exec "$HOME_DIR/tools/cargo-cosmo" "$@"
