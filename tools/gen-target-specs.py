#!/usr/bin/env python3
"""Generate the cosmo target specs by patching rustc's own musl specs.

Deriving from `rustc --print target-spec-json` rather than hand-writing the JSON
keeps data-layout, llvm-target and the mandatory-field set in sync with whatever
nightly is installed. Custom target JSON is schema-checked strictly, and the
schema drifts between nightlies; hand-written specs rot within weeks.
"""
import json
import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "targets")
TOOLCHAIN = os.path.join(REPO, "toolchain", "cosmocc")

# Overrides shared by both arches. Every one of these traces to a flag in
# toolchain/cosmocc/bin/cosmocc -- see docs/DESIGN.md.
COMMON = {
   # cosmocc links -no-pie -fno-pie: an APE loads at a fixed address.
   "position-independent-executables": False,
   "static-position-independent-executables": False,
   "relocation-model": "static",

   # Always static, never a dynamic loader.
   "crt-static-default": True,
   "crt-static-respected": True,
   "dynamic-linking": False,
   "has-rpath": False,
   "plt-by-default": False,

   # cosmo supplies ape.o + crt.o and its own ape.lds; rustc must not inject
   # musl's crt1.o/crti.o or they fight over _start.
   "crt-objects-fallback": "false",
   "pre-link-objects-fallback": {},
   "post-link-objects-fallback": {},

   # cosmocc builds with -mno-tls-direct-seg-refs, which rustc does not expose
   # (verified: not in `rustc --print target-features`). Rather than emit native
   # ELF TLS that cosmo's loader cannot service, turn thread-locals off so std
   # falls back to its pthread_key_create path, which cosmo does implement.
   "has-thread-local": False,

   # Cosmopolitan bundles the full LLVM libunwind: libcosmo.a defines the
   # complete _Unwind_* ABI (RaiseException, Resume, GetIP, Backtrace, ...),
   # and both ape.lds and aarch64.lds emit .eh_frame_hdr + .gcc_except_table.
   # So panic=unwind works -- the "cosmo has no unwinder" limitation in the
   # older prior art no longer holds.
   "panic-strategy": "unwind",
   "default-uwtable": True,

   # cosmocc treats -fno-omit-frame-pointer as precious.
   "frame-pointer": "always",

   "linker-flavor": "gnu-cc",
   "executables": True,
}

ARCH = {
   "x86_64": {
      "base": "x86_64-unknown-linux-musl",
      "overrides": {},
   },
   "aarch64": {
      "base": "aarch64-unknown-linux-musl",
      # cosmocc passes -ffixed-x18 -ffixed-x28: cosmo reserves x18 for its own
      # thread pointer and x28 internally. If rustc allocates either as a
      # scratch GPR the result is silent memory corruption, not a link error.
      "overrides": {"features": "+reserve-x18,+reserve-x28"},
   },
}


def spec_for(base):
   # Honour the same toolchain override cargo-cosmo uses. Bare `rustc` is
   # whatever the *consuming* project's rust-toolchain.toml selects, which off
   # nightly rejects -Z and leaves the caller with a traceback instead of a
   # spec -- the failure mode for driving an external crate from a fresh clone.
   tc = os.environ.get("CARGO_COSMO_TOOLCHAIN")
   rustc = ["rustup", "run", tc, "rustc"] if tc else ["rustc"]
   out = subprocess.run(
      [*rustc, "-Z", "unstable-options",
       "--target", base, "--print", "target-spec-json"],
      capture_output=True, text=True, check=True)
   return json.loads(out.stdout)


def main():
   os.makedirs(OUT, exist_ok=True)
   for arch, cfg in ARCH.items():
      spec = spec_for(cfg["base"])
      spec.update(COMMON)
      spec.update(cfg["overrides"])

      spec["linker"] = os.path.join(REPO, "tools", f"cosmo-ld-{arch}")
      spec["metadata"] = {
         "description": f"Cosmopolitan Libc APE ({arch})",
         "host_tools": False,
         "std": True,
         "tier": 3,
      }
      # rustc emits these for informational purposes; they are rejected on input.
      for k in ("is-builtin",):
         spec.pop(k, None)

      path = os.path.join(OUT, f"{arch}-unknown-cosmo.json")
      with open(path, "w") as f:
         json.dump(spec, f, indent=2, sort_keys=True)
         f.write("\n")
      print(f"wrote {path}")


if __name__ == "__main__":
   sys.exit(main())
