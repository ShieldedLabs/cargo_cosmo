#!/bin/sh
# End-to-end check: build every example for both architectures, fuse each into
# a fat APE, and run the result. No mocks and no unit tests -- if this passes,
# real binaries were built and really executed.
#
#   tests/run-all.sh
#
# Note on coverage: the x86-64 half is executed here. The arm64 half is checked
# structurally (it is present, and rustc respected cosmo's reserved registers)
# but not run, because this is an x86-64 host with no emulator installed.

REPO=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PATH="$REPO/tools:$REPO/toolchain/cosmocc/bin:$PATH"
export PATH

# Specs embed an absolute linker path, so they are generated, never committed.
python3 "$REPO/tools/gen-target-specs.py" >/dev/null

pass=0
fail=0

check() {
   name=$1; got=$2; want=$3
   if printf '%s' "$got" | grep -q "$want"; then
      echo "    ok: $name"
      pass=$((pass + 1))
   else
      echo "    FAIL: $name (expected to find '$want')"
      fail=$((fail + 1))
   fi
}

ok_if() {
   if [ "$1" -eq 0 ]; then
      echo "    ok: $2"; pass=$((pass + 1))
   else
      echo "    FAIL: $3"; fail=$((fail + 1))
   fi
}

banner() { echo; echo "=== $1 ==="; }

# ---------------------------------------------------------------- C baseline
banner "C baseline (proves the toolchain itself)"
cd "$REPO/examples/c-hello" || exit 1
cosmocc -O2 -o hello.com hello.c || exit 1
check "c hello runs" "$(./hello.com)" "hello from a C APE"

# ------------------------------------------------------------------- no_std
banner "no_std Rust -> cosmopolitan"
cd "$REPO/examples/rust-nostd" || exit 1
cargo build --release -Zjson-target-spec \
   -Zbuild-std=core,compiler_builtins \
   -Zbuild-std-features=compiler-builtins-mem \
   --target "$REPO/targets/x86_64-unknown-cosmo.json" >/dev/null 2>&1
check "no_std links and runs" \
   "$(./target/x86_64-unknown-cosmo/release/nostd)" "no_std Rust APE"

# ---------------------------------------------------------------------- std
banner "std Rust -> fat APE"
cd "$REPO/examples/rust-std" || exit 1
cargo cosmo build --release >/dev/null || exit 1
out=$(./target/cosmo/stdhello.com 2>&1)
check "stdio"     "$out" "hello from a std Rust APE"
check "heap"      "$out" "heap  = works"
check "threads"   "$out" "thread sum 1..10 = 55"
check "file io"   "$out" "file  = file io works"
check "unwinding" "$out" "catch_unwind caught = true"
check "io::Error formatting (strerror_r shim)" "$out" "ioerr = No such file or directory"

# ------------------------------------------------------------------ fat-ness
banner "fat binary carries both architectures"
check-fat.py "$REPO/examples/rust-std/target/cosmo/stdhello.com"
ok_if $? "both architectures present" "fat binary missing an architecture"

# -------------------------------------------------------------- syscon probe
banner "cosmo runtime constants visible from Rust"
cd "$REPO/examples/syscon-probe" || exit 1
cargo cosmo build --release >/dev/null || exit 1
check "no constant mismatches" "$(./target/cosmo/syscon-probe.com)" "0 mismatch"

# ------------------------------------------------------ aarch64 reserved regs
banner "aarch64 honours cosmo's reserved x18/x28"
aarch64-linux-cosmo-objdump -d \
   "$REPO/examples/rust-std/target/aarch64-unknown-cosmo/release/stdhello" \
   2>/dev/null | check-regs.py
ok_if $? "rustc never allocates the reserved registers" \
         "rustc emitted x18/x28 inside Rust code"

echo
echo "================================"
echo " passed: $pass   failed: $fail"
echo "================================"
[ "$fail" -eq 0 ]
