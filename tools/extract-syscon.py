#!/usr/bin/env python3
"""Extract cosmopolitan's complete per-OS constant table out of libcosmo.a.

This is the generative source for the stage-2 normalization shim described in
docs/DESIGN.md. Cosmopolitan compiles every system constant into an object
carrying one tiny section per supported OS:

    .sort.rodata.syscon.<os>.<rank>.<group>.<NAME>

whose contents are the value for that OS, LEB128-encoded. Reading those gives
the whole cross-OS divergence table without parsing a single C header, and
without depending on a cosmopolitan source checkout.

Usage:
    tools/extract-syscon.py [--arch x86_64] [--json out.json]
"""
import argparse
import collections
import json
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OSES = ("linux", "xnu", "freebsd", "openbsd", "netbsd", "windows")

SECTION = re.compile(
   r"^Contents of section \.sort\.rodata\.syscon\."
   r"(?P<os>\w+)\.(?P<rank>\d+)\.(?P<group>[^.]+)\.(?P<name>[A-Za-z0-9_]+):$")
HEXLINE = re.compile(r"^ [0-9a-f]{4} ((?:[0-9a-f]{2}| )+?)  ")


def argv_for(tool, *args):
   """Build an argv that can actually exec `tool`.

   Most of the cosmocc toolchain binaries are themselves APEs. The kernel
   cannot exec those directly -- no ELF magic up front and no shebang -- which
   a shell papers over because the APE header is also valid /bin/sh, but
   posix_spawn just returns ENOEXEC. Native ELF tools are run directly.
   """
   with open(tool, "rb") as f:
      if f.read(4) == b"\x7fELF":
         return [tool, *args]
   return ["/bin/sh", tool, *args]


def uleb(b):
   """Decode unsigned LEB128. Cosmo stores syscon values in this form."""
   v = 0
   for i, byte in enumerate(b):
      v |= (byte & 0x7F) << (7 * i)
      if not byte & 0x80:
         break
   return v


def extract(archive, objdump):
   """Return {name: {"group": g, "values": {os: int}}}."""
   proc = subprocess.run(argv_for(objdump, "-s", archive),
                         capture_output=True, text=True)
   if proc.returncode != 0:
      print(proc.stderr, file=sys.stderr)
      sys.exit(f"objdump failed on {archive}")

   table = collections.defaultdict(lambda: {"group": None, "values": {}})
   cur = None
   raw = []

   def flush():
      if cur is None:
         return
      b = bytes.fromhex("".join(raw))
      if not b:
         return
      # Decode unsigned, then read the result as 64-bit two's complement.
      # Genuinely negative constants (AT_FDCWD is -100, MAP_FAILED is -1) come
      # out of LEB128 as huge positives otherwise, which then overflow i64 in
      # the generated Rust and silently misreport as enormous flag values.
      v = uleb(b)
      if v >= 1 << 63:
         v -= 1 << 64
      table[cur["name"]]["group"] = cur["group"]
      table[cur["name"]]["values"][cur["os"]] = v

   for line in proc.stdout.splitlines():
      m = SECTION.match(line)
      if m:
         flush()
         cur, raw = m.groupdict(), []
         continue
      if cur is not None:
         h = HEXLINE.match(line)
         if h:
            raw.append(h.group(1).replace(" ", ""))
         elif line.startswith("Contents of section") or not line.strip():
            flush()
            cur, raw = None, []
   flush()
   return dict(table)


def report(table):
   print(f"constants extracted: {len(table)}")
   print(f"groups: {len(set(v['group'] for v in table.values()))}\n")

   print("divergence from Linux (constants whose value differs):")
   print(f"  {'os':<10}{'differs':>9}{'absent':>9}{'total':>8}{'% differ':>10}")
   for o in OSES:
      if o == "linux":
         continue
      differ = absent = total = 0
      for name, e in table.items():
         lin = e["values"].get("linux")
         other = e["values"].get(o)
         if lin is None or other is None:
            continue
         total += 1
         # cosmo encodes "this OS has no such constant" as 0 for most groups.
         if other == 0 and lin != 0:
            absent += 1
         elif other != lin:
            differ += 1
      pct = 100.0 * (differ + absent) / total if total else 0
      print(f"  {o:<10}{differ:>9}{absent:>9}{total:>8}{pct:>9.1f}%")

   print("\nper-group divergence (worst first, non-Linux OSes pooled):")
   bad = collections.Counter()
   size = collections.Counter()
   for name, e in table.items():
      lin = e["values"].get("linux")
      size[e["group"]] += 1
      for o in OSES:
         if o == "linux":
            continue
         other = e["values"].get(o)
         if other is not None and lin is not None and other != lin:
            bad[e["group"]] += 1
            break
   for g, n in bad.most_common(15):
      print(f"  {g:<24}{n:>5} / {size[g]:<5} diverge")


def main():
   ap = argparse.ArgumentParser()
   ap.add_argument("--arch", default="x86_64")
   ap.add_argument("--json")
   a = ap.parse_args()

   tc = os.path.join(REPO, "toolchain", "cosmocc")
   archive = os.path.join(tc, f"{a.arch}-linux-cosmo", "lib", "libcosmo.a")
   objdump = os.path.join(tc, "bin", f"{a.arch}-linux-cosmo-objdump")

   table = extract(archive, objdump)
   report(table)

   if a.json:
      with open(a.json, "w") as f:
         json.dump(table, f, indent=1, sort_keys=True)
      print(f"\nwrote {a.json}")


if __name__ == "__main__":
   main()
