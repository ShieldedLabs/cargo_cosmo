#!/usr/bin/env python3
"""Generate the stage-2 normalization tables for crates/cosmo-compat.

Rust's std is compiled with Linux's values for every OS constant; cosmopolitan
resolves the running host's values at load time. The shim in cosmo-compat
translates at the libc call boundary. This script emits the data it needs:

  crates/cosmo-compat/src/gen.rs   per group: the Linux values std was compiled
                                   with, `extern` declarations of cosmo's
                                   load-time symbols (the host values), and --
                                   for `cargo test` on a Linux host -- the
                                   Windows and XNU columns from the extracted
                                   table, so the translation logic is testable
                                   without a Windows machine.
  crates/cosmo-compat/wrap.txt     the libc symbols the shim wraps; cosmo-ld
                                   turns each into -Wl,--wrap=NAME.

Inputs: data/syscon-x86_64.json (from extract-syscon.py), `nm` over libcosmo.a
(which names are real symbols rather than #defines) and cosmo's
include/libc/sysv/consts/*.h (each constant's declared width: POLL* are
int16_t, O_* unsigned, most int).

    tools/gen-shim.py
"""
import json
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TABLE = os.path.join(REPO, "data", "syscon-x86_64.json")
ARCHIVE = os.path.join(REPO, "toolchain", "cosmocc", "x86_64-linux-cosmo", "lib", "libcosmo.a")
NM = os.path.join(REPO, "toolchain", "cosmocc", "bin", "x86_64-linux-cosmo-nm")
OUT_RS = os.path.join(REPO, "crates", "cosmo-compat", "src", "gen.rs")
OUT_WRAP = os.path.join(REPO, "crates", "cosmo-compat", "wrap.txt")

# Groups the shim translates, and how. "mask": a bit set, translated bit by
# bit (multi-bit entries first). "enum": a single value, translated by lookup.
GROUPS = {
   "errno": "enum", "compat": "enum", "junkerr": "enum",
   "open": "mask", "mmap": "mask", "mprot": "mask", "msg": "mask", "poll": "mask",
   "sock": "mask", "sigact": "mask", "waitpid": "mask", "waitid": "mask",
   "at": "enum", "af": "enum", "pf": "enum", "so": "enum", "tcp": "enum", "ip": "enum",
   "ipv6": "enum", "sig": "enum", "clock": "enum", "fcntl": "enum", "fcntl2": "enum",
   "fcntl3": "enum", "ioctl": "enum", "access": "mask", "lock": "mask", "madv": "enum",
   "shut": "enum", "ss": "enum", "scm": "enum", "limits": "enum", "rlimit": "enum",
}

# std's libc imports on the cosmo target (nm -u over the build-std rlibs), minus
# what needs no translation (malloc/free/environ/strnlen/gai_strerror/abort/
# exit/_exit/sigemptyset/pthread_self/pthread_getspecific/getauxval/getenv/
# setenv/unsetenv/getpid/getppid/gettid/getuid/sched_yield). Each entry says
# what the wrapper does after `__real_*` returns:
#   int   -1 means errno was set: translate it in place
#   ptr   NULL means errno was set
#   code  the return value IS an errno (pthread_*, posix_spawn*, getpwuid_r)
# Functions whose ARGUMENTS also carry constants are hand-written in shim.rs
# and listed under HAND so they get --wrap but no generated body.
# Not wrapped: pidfd_spawnp/pidfd_getpid -- std imports them WEAK (null when
# absent, which cosmo lacks); a wrapper's __real_ reference would be strong.
PASSTHROUGH = {
   "int": """accept4 bind chdir chmod chown chroot close closedir connect
      copy_file_range dirfd dup dup2 execvp fchmod fchown fdatasync flock fork
      fstat fsync ftruncate futimens gethostname getpeername getrandom
      getsockname lchown listen lseek lstat mkdir mkfifo munmap pause
      pipe2 pread preadv pwrite pwritev read readlink
      readv rename rmdir sched_getaffinity sendfile setgid setgroups setpgid
      setsid setuid splice stat symlink sysconf syscall unlink write writev
      shutdown mprotect sigaltstack clock_gettime clock_nanosleep""".split(),
   "ptr": "fdopendir getcwd opendir readdir realpath".split(),
   "code": """getpwuid_r posix_spawnattr_destroy posix_spawnattr_init
      posix_spawnattr_setflags posix_spawnattr_setpgroup
      posix_spawnattr_setsigdefault posix_spawn_file_actions_addchdir_np
      posix_spawn_file_actions_adddup2 posix_spawn_file_actions_destroy
      posix_spawn_file_actions_init posix_spawnp pthread_attr_destroy
      pthread_attr_getguardsize pthread_attr_getstack pthread_attr_init
      pthread_attr_setstacksize pthread_create pthread_detach
      pthread_getattr_np pthread_join pthread_key_create pthread_key_delete
      pthread_setname_np pthread_setspecific""".split(),
}
HAND = """open openat fcntl ioctl socket socketpair setsockopt getsockopt send
   sendto recv recvfrom sendmsg recvmsg poll mmap sigaction signal kill killpg
   sigaddset waitpid unlinkat linkat renameat fchmodat utimensat getaddrinfo
   freeaddrinfo""".split()


def ape(path, *args):
   with open(path, "rb") as f:
      if f.read(4) == b"\x7fELF":
         return [path, *args]
   return ["/bin/sh", path, *args]


HEADERS = os.path.join(REPO, "toolchain", "cosmocc", "include", "libc", "sysv", "consts")
CTYPE = {"int": "i32", "unsigned": "u32", "uint32_t": "u32", "int32_t": "i32", "int16_t": "i16", "uint16_t": "u16",
         "int64_t": "i64", "uint64_t": "u64", "long": "i64", "unsigned long": "u64", "errno_t": "i32", "int8_t": "i8", "uint8_t": "u8"}


def symbol_sizes():
   """{NAME: rust type} for every constant cosmo exports as a symbol.

   The width comes from the header declaration (`extern const int16_t POLLIN;`):
   nm reports section sizes for these, which are all 8, and reading an int16_t
   through an 8-byte load would pick up the neighbours. Presence in libcosmo.a
   (nm) is what decides whether the name is a symbol at all rather than a
   #define.
   """
   out = subprocess.run(ape(NM, ARCHIVE), capture_output=True, text=True, check=True).stdout
   present = set(re.findall(r"^[0-9a-f]+ [BDRbdr] ([A-Z][A-Z0-9_]+)$", out, re.M))
   decl = re.compile(r"^extern\s+(?:const\s+)?(int16_t|uint16_t|int32_t|uint32_t|int64_t|uint64_t|unsigned long|unsigned|long|int|errno_t|int8_t|uint8_t)\s+([A-Z][A-Z0-9_]+)\s*;", re.M)
   types = {}
   for fn in os.listdir(HEADERS):
      if not fn.endswith(".h"):
         continue
      with open(os.path.join(HEADERS, fn), errors="replace") as f:
         for ctype, name in decl.findall(f.read()):
            types[name] = CTYPE[ctype]
   return {name: types.get(name, "i32") for name in present}


def rust_type(t):
   return t


def main():
   table = json.load(open(TABLE))
   sizes = symbol_sizes()
   groups = {g: [] for g in GROUPS}
   skipped = []
   for name, ent in table.items():
      g = ent["group"]
      if g not in GROUPS:
         continue
      if name not in sizes:
         skipped.append(name)      # a #define in cosmo's headers, not a symbol
         continue
      groups[g].append((name, ent["values"], sizes[name]))
   for g in groups:
      groups[g].sort()

   rs = ["//! GENERATED by tools/gen-shim.py from data/syscon-x86_64.json -- do not edit.",
         "//!",
         "//! One block per constant group: the Linux values (what std was compiled",
         "//! with), cosmo's load-time symbols (the host's values), and for tests the",
         "//! Windows and XNU columns of the extracted table.",
         "#![allow(non_upper_case_globals, dead_code, non_snake_case)]",
         "use crate::xlate::{Group, Kind};", ""]
   for g, entries in groups.items():
      if not entries:
         continue
      kind = GROUPS[g]
      n = len(entries)
      names = ", ".join(f'"{e[0]}"' for e in entries)
      linux = ", ".join(str(e[1]["linux"]) for e in entries)
      win = ", ".join(str(e[1].get("windows", 0)) for e in entries)
      xnu = ", ".join(str(e[1].get("xnu", 0)) for e in entries)
      rs.append(f"// ---- {g} ({kind}) ----")
      rs.append("#[cfg(not(test))]")
      rs.append("unsafe extern \"C\" {")
      for name, _, size in entries:
         rs.append(f"    static {name}: {rust_type(size)};")
      rs.append("}")
      rs.append("#[cfg(not(test))]")
      rs.append(f"fn host_{g}() -> [i64; {n}] {{ unsafe {{ [{', '.join(f'{e[0]} as i64' for e in entries)}] }} }}")
      rs.append("#[cfg(test)]")
      rs.append(f"fn host_{g}() -> [i64; {n}] {{ crate::xlate::test_host(&WINDOWS_{g}, &XNU_{g}) }}")
      rs.append(f"pub static NAMES_{g}: [&str; {n}] = [{names}];")
      rs.append(f"pub static LINUX_{g}: [i64; {n}] = [{linux}];")
      rs.append(f"pub static WINDOWS_{g}: [i64; {n}] = [{win}];")
      rs.append(f"pub static XNU_{g}: [i64; {n}] = [{xnu}];")
      rs.append(f"static mut HOST_{g}: [i64; {n}] = [0; {n}];")
      rs.append(f"static STATE_{g}: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);")
      rs.append(f"pub fn {g}() -> Group {{")
      rs.append(f"    crate::xlate::group(Kind::{kind.capitalize()}, &NAMES_{g}, &LINUX_{g}, &STATE_{g}, &raw mut HOST_{g}, host_{g})")
      rs.append("}")
      rs.append("")
   # the generated passthrough wrappers
   rs.append("// ---- errno-only wrappers: pass the registers through, fix errno on failure ----")
   rs.append("// Sound because every function here takes only integer/pointer arguments (no")
   rs.append("// floats, no structs by value), which the SysV and AAPCS conventions pass in the")
   rs.append("// first six/eight integer registers regardless of declared type; an untyped")
   rs.append("// six-register signature therefore forwards any of them unchanged.")
   for kind, fns in PASSTHROUGH.items():
      for f in fns:
         rs.append("unsafe extern \"C\" { fn __real_" + f + "(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize; }")
         rs.append("#[unsafe(no_mangle)]")
         rs.append(f"pub unsafe extern \"C\" fn __wrap_{f}(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {{")
         rs.append(f"    let r = unsafe {{ __real_{f}(a, b, c, d, e, f) }};")
         if kind == "int":
            rs.append("    if r as i32 == -1 { crate::shim::fix_errno(); }")
            rs.append("    r")
         elif kind == "ptr":
            rs.append("    if r == 0 { crate::shim::fix_errno(); }")
            rs.append("    r")
         else:
            rs.append("    crate::xlate::errno_to_linux(r as i32 as i64) as isize")
         rs.append("}")
   os.makedirs(os.path.dirname(OUT_RS), exist_ok=True)
   with open(OUT_RS, "w") as f:
      f.write("\n".join(rs) + "\n")

   wraps = sorted(set(HAND) | {f for fns in PASSTHROUGH.values() for f in fns})
   with open(OUT_WRAP, "w") as f:
      f.write("# GENERATED by tools/gen-shim.py: libc symbols cosmo-compat wraps (cosmo-ld adds --wrap for each).\n")
      f.write("\n".join(wraps) + "\n")

   total = sum(len(v) for v in groups.values())
   print(f"gen-shim: {total} constants in {sum(1 for v in groups.values() if v)} groups, {len(wraps)} wrapped functions")
   if skipped:
      print(f"gen-shim: {len(skipped)} table entries are #defines, not symbols (skipped): {' '.join(skipped[:12])}{' ...' if len(skipped) > 12 else ''}")


if __name__ == "__main__":
   main()
