#!/bin/sh
# Convert cosmocc's own binaries from APEs into native executables.
#
# cosmocc ships every tool as an Actually Portable Executable. A shell can run
# one, which is why tools/cosmo-ld works as it is, but cosmocc's gcc spawns
# `ld`, `as` and `cc1` through posix_spawnp, and the kernel cannot exec an APE:
#
#     x86_64-linux-cosmo-gcc: fatal error: cannot execute 'ld':
#         posix_spawnp: No such file or directory
#
# rustc reports that as a warning and still exits 0, so the build "succeeds"
# and produces no binary. Running cosmo's own `assimilate` over the toolchain
# rewrites each file in place into the host's native format and it goes away.
#
# Safe to re-run: anything already native is skipped. Only touches toolchain/,
# which is downloaded rather than committed.
#
# The alternative is registering APE with binfmt_misc, which is system-wide and
# needs root. This needs neither.

set -e

REPO=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
COSMO="$REPO/toolchain/cosmocc"
[ -d "$COSMO" ] || { echo "assimilate-toolchain: no toolchain at $COSMO" >&2; exit 1; }

ASSIMILATE="$COSMO/bin/assimilate"
[ -f "$ASSIMILATE" ] || { echo "assimilate-toolchain: no $ASSIMILATE" >&2; exit 1; }

# *.bak is excluded for a reason: assimilate keeps a copy of each original
# beside it, and that copy is itself an APE. Without the exclusion a second
# run assimilates the backups, leaving .bak.bak, and every pass duplicates
# the whole toolchain on disk while reporting work it did not do.
#
# Must be exec'd directly rather than as `sh assimilate`: the APE header is
# valid sh only in the way cosmo's loader expects, and dash mis-parses it
# ("Syntax error: Unterminated quoted string").
n=0
for f in $(find "$COSMO" -type f ! -name '*.elf' ! -name '*.a' ! -name '*.o' \
                         ! -name '*.h' ! -name '*.c' ! -name '*.lds' \
                         ! -name '*.bak*'); do
   magic=$(od -An -N4 -tx1 "$f" 2>/dev/null | tr -d ' \n')
   if [ "$magic" = "4d5a7146" ]; then
      if "$ASSIMILATE" "$f" >/dev/null 2>&1; then
         n=$((n + 1))
      else
         echo "assimilate-toolchain: could not convert $f" >&2
      fi
   fi
done
echo "assimilate-toolchain: converted $n binaries"
