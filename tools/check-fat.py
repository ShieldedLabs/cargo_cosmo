#!/usr/bin/env python3
"""Verify an APE really carries both architectures.

An APE is not two ELFs glued together, so the two halves must be checked in
different ways. The x86-64 half *is* the file's own PE/ELF body -- that is what
the MZ header at offset 0 is for -- while the arm64 half is appended as a
discrete ELF for the loader to select at startup. Scanning for two ELF magics
finds only the arm64 one and wrongly looks like a broken build.

    tools/check-fat.py <file.com>
"""
import re
import struct
import sys


def main(path):
   d = open(path, "rb").read()

   lfanew = struct.unpack_from("<I", d, 0x3C)[0]
   x86 = (d[lfanew:lfanew + 4] == b"PE\0\0"
          and struct.unpack_from("<H", d, lfanew + 4)[0] == 0x8664)

   arm = any((d[m.start() + 18] | (d[m.start() + 19] << 8)) == 183   # EM_AARCH64
             for m in re.finditer(b"\x7fELF", d)
             if m.start() + 20 <= len(d))

   print(f"    x86_64  (PE COFF machine 0x8664): {'yes' if x86 else 'NO'}")
   print(f"    aarch64 (embedded EM_AARCH64):    {'yes' if arm else 'NO'}")
   return 0 if (x86 and arm) else 1


if __name__ == "__main__":
   sys.exit(main(sys.argv[1]))
