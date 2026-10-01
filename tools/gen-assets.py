#!/usr/bin/env python3
"""Regenerate the assets embedded in the cosmo-build crate.

cosmo-build has to be self-contained on crates.io -- a published crate cannot
reach into this repo -- so the target specs, the wrap list and the toolchain
pin are baked into it. They are generated rather than hand-copied because all
three derive from something else here: the specs from rustc's musl specs, the
wrap list from cosmo-compat, the channel from rust-toolchain.toml.

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

   # The linker and compiler drivers are assets/shim.rs, a Rust program the
   # crate compiles on first use with this list baked in.
   wrap = os.path.join(REPO, "crates", "cosmo-compat", "wrap.txt")
   out = os.path.join(ASSETS, "wrap.txt")
   open(out, "w").write(open(wrap).read())
   print(f"wrote {out}")

   chan = re.search(r'channel\s*=\s*"([^"]+)"',
                    open(os.path.join(REPO, "rust-toolchain.toml")).read()).group(1)
   open(os.path.join(ASSETS, "channel.txt"), "w").write(chan + "\n")
   print(f"wrote channel.txt ({chan})")
   return 0


if __name__ == "__main__":
   sys.exit(main())
