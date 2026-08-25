#!/usr/bin/env python3
"""Regenerate the assets embedded in the cosmo-build crate.

cosmo-build has to be self-contained on crates.io -- a published crate cannot
reach into this repo -- so the target specs, the linker shim and the toolchain
pin are baked into it. They are generated rather than hand-copied because all
three derive from something else here: the specs from rustc's musl specs, the
shim's wrap list from cosmo-compat, the channel from rust-toolchain.toml.

Run it before every publish; `cargo test -p cosmo-build` asserts the wrap list
half of it did not rot.
"""
import json
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = os.path.join(REPO, "crates", "cosmo-build", "assets")


def main():
   os.makedirs(ASSETS, exist_ok=True)

   # The specs must exist and be current before they can be embedded.
   if subprocess.call([sys.executable, os.path.join(REPO, "tools", "gen-target-specs.py")]):
      return "gen-target-specs.py failed"

   for arch in ("x86_64", "aarch64"):
      spec = json.load(open(os.path.join(REPO, "targets", f"{arch}-unknown-cosmo.json")))
      # The consuming machine's cache path is unknown here; cosmo-build fills
      # it in when it writes the spec out.
      spec["linker"] = "@COSMO_LD@"
      out = os.path.join(ASSETS, f"{arch}-unknown-cosmo.json")
      with open(out, "w") as f:
         json.dump(spec, f, indent=2, sort_keys=True)
         f.write("\n")
      print(f"wrote {out}")

   # The shell shim derives its paths from its own location in this repo; the
   # embedded copy is handed them instead. The wrap list is inlined so the
   # published crate does not have to ship wrap.txt and read it at link time.
   ld = open(os.path.join(REPO, "tools", "cosmo-ld")).read()
   ld = ld.replace(
      'REPO=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)\n'
      'BIN="$REPO/toolchain/cosmocc/bin"\n'
      'LIB="$REPO/toolchain/cosmocc/$ARCH-linux-cosmo/lib"',
      'COSMOCC=@COSMOCC@\nBIN="$COSMOCC/bin"\nLIB="$COSMOCC/$ARCH-linux-cosmo/lib"')

   wrap = os.path.join(REPO, "crates", "cosmo-compat", "wrap.txt")
   syms = [l.strip() for l in open(wrap) if l.strip() and not l.startswith("#")]
   start = ld.index("# Stage-2 normalization")
   end = ld.index("# COSMO_LD_DEBUG")
   ld = ld[:start] + (
      "# Stage-2 normalization: route every libc call std makes through\n"
      "# cosmo-compat's __wrap_* translators. Generated from the same wrap list\n"
      "# the crate is built from, so the two cannot drift.\n"
      'WRAPS="' + " ".join(f"-Wl,--wrap={s}" for s in syms) + '"\n\n') + ld[end:]

   out = os.path.join(ASSETS, "cosmo-ld.sh")
   open(out, "w").write(ld)
   print(f"wrote {out} ({len(syms)} wrapped symbols)")

   chan = re.search(r'channel\s*=\s*"([^"]+)"',
                    open(os.path.join(REPO, "rust-toolchain.toml")).read()).group(1)
   open(os.path.join(ASSETS, "channel.txt"), "w").write(chan + "\n")
   print(f"wrote channel.txt ({chan})")
   return 0


if __name__ == "__main__":
   sys.exit(main())
