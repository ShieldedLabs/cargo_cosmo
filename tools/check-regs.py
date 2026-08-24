#!/usr/bin/env python3
"""Verify rustc never allocated cosmo's reserved aarch64 registers.

cosmocc compiles with -ffixed-x18 -ffixed-x28: cosmo uses x18 as its thread
pointer and reserves x28 for its own runtime. If rustc allocates either as a
scratch GPR the result is silent memory corruption at runtime, not a link
error, so this is worth asserting on every build.

Cosmo's own C and assembly reference both registers deliberately (pthread
setup, signal contexts, unwinder register files), so only rustc-generated
symbols -- the Itanium-mangled _ZN* ones -- are counted.

    aarch64-...-objdump -d <bin> | tools/check-regs.py
"""
import re
import sys

RESERVED = re.compile(r"\b[xw](?:18|28)\b")
LABEL = re.compile(r"^[0-9a-f]+ <(.+)>:")


def main():
   cur = None
   offenders = {}
   for line in sys.stdin:
      m = LABEL.match(line)
      if m:
         cur = m.group(1)
         continue
      if cur and cur.startswith("_ZN") and RESERVED.search(line):
         offenders[cur] = offenders.get(cur, 0) + 1

   total = sum(offenders.values())
   print(f"    rustc-generated uses of x18/x28: {total}")
   for name, n in sorted(offenders.items(), key=lambda t: -t[1])[:5]:
      print(f"      {n:5d}  {name[:70]}")
   return 1 if total else 0


if __name__ == "__main__":
   sys.exit(main())
