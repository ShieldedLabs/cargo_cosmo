//! Acquiring the two toolchains: the pinned Rust nightly and cosmocc.

use crate::cache::{self, Cache, CHANNEL};
use crate::sha256::Sha256;
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;

/// Pinned to a versioned release, not the moving `cosmocc.zip`, and checked
/// against its hash. Everything this crate claims to work was measured against
/// exactly this toolchain; letting the build pick up whatever cosmo.zip serves
/// today would mean shipping a different compiler to every user who builds on a
/// different day, and executing 1.4GB of it unverified.
const COSMOCC_VERSION: &str = "4.0.2";
const COSMOCC_SHA256: &str = "85b8c37a406d862e656ad4ec14be9f6ce474c1b436b9615e91a55208aced3f44";

/// `COSMO_COSMOCC_URL` points the fetch at a mirror or an internal cache, for
/// a build host that cannot reach cosmo.zip.
fn cosmocc_url() -> String {
   std::env::var("COSMO_COSMOCC_URL").unwrap_or_else(|_| {
      format!("https://cosmo.zip/pub/cosmocc/cosmocc-{COSMOCC_VERSION}.zip")
   })
}

/// Install the pinned nightly if it is missing, and `rust-src` with it.
///
/// The consuming project keeps whatever toolchain it already uses; only the
/// cosmo builds are forced onto this one, because -Zbuild-std, -Zjson-target-spec
/// and custom target JSON exist on no other channel.
pub fn ensure_rust() -> Result<(), String> {
   let channel = CHANNEL.trim();

   let listed = Command::new("rustup").args(["toolchain", "list"]).output();
   let listed = match listed {
      Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
      Err(e) => {
         return Err(format!(
            "rustup is required to install the pinned toolchain ({channel}): {e}"
         ))
      }
   };

   if !listed.lines().any(|l| l.starts_with(channel)) {
      run(Command::new("rustup").args([
         "toolchain", "install", "--profile", "minimal", "-c", "rust-src", channel,
      ]))?;
      return Ok(());
   }

   // Present but possibly without rust-src, which -Zbuild-std needs and which
   // the minimal profile does not carry.
   let comps = Command::new("rustup")
      .args(["component", "list", "--toolchain", channel, "--installed"])
      .output()
      .map_err(|e| format!("rustup component list: {e}"))?;
   if !String::from_utf8_lossy(&comps.stdout).lines().any(|l| l.starts_with("rust-src")) {
      run(Command::new("rustup").args([
         "component", "add", "--toolchain", channel, "rust-src",
      ]))?;
   }
   Ok(())
}

/// Download and unpack cosmocc unless it is already there.
///
/// ~440MB down and ~1.4GB unpacked, so this happens once per cache and never
/// per project. It is fetched rather than vendored because crates.io is not a
/// place to ship a gigabyte of GPL toolchain.
pub fn ensure_cosmocc(cache: &Cache) -> Result<(), String> {
   if cache.bin("apelink").exists() {
      return patch_tools(&cache.cosmocc);
   }
   fs::create_dir_all(&cache.cosmocc).map_err(|e| format!("{}: {e}", cache.cosmocc.display()))?;

   // Download to a sibling and rename, so an interrupted fetch never leaves
   // something that looks like a complete archive.
   let zip = cache.root.join("cosmocc.zip.part");
   let url = cosmocc_url();
   let resp = ureq::get(&url).call().map_err(|e| format!("GET {url}: {e}"))?;
   let mut body = resp.into_body().into_reader();
   let mut out = File::create(&zip).map_err(|e| format!("{}: {e}", zip.display()))?;
   let mut hasher = Sha256::new();
   io::copy(&mut body, &mut Tee(&mut out, &mut hasher))
      .map_err(|e| format!("downloading cosmocc: {e}"))?;
   drop(out);

   // Verified whatever the source: a mirror is only a different route to the
   // same bytes, and one that cannot produce them is exactly what a pin is for.
   // COSMO_COSMOCC_SHA256 is the way to run a deliberately different toolchain.
   let want = std::env::var("COSMO_COSMOCC_SHA256").unwrap_or_else(|_| COSMOCC_SHA256.into());
   let got = hex(&hasher.finalize());
   if got != want {
      let _ = fs::remove_file(&zip);
      return Err(format!(
         "{url} does not match the expected hash\n  \
          expected {want}\n  got      {got}\n\
          Refusing to unpack it. To run a different cosmocc deliberately, set \
          COSMO_COSMOCC_SHA256 to its hash."
      ));
   }

   unzip(&zip, &cache.cosmocc)?;
   let _ = fs::remove_file(&zip);
   assimilate(&cache.cosmocc)?;

   if !cache.bin("apelink").exists() {
      return Err(format!(
         "cosmocc unpacked into {} but bin/apelink is missing",
         cache.cosmocc.display()
      ));
   }
   patch_tools(&cache.cosmocc)
}

/// Patch two cosmo 4.0.2 runtime bugs out of every cosmocc tool, on Windows.
/// Between them they made a few compiles in a hundred crash or hang, and long
/// compiles on a busy machine nearly always. No later cosmocc exists to move to.
///
/// dlmalloc merges adjacent mmaps into one segment, and trimming the top of it
/// munmaps a range that covers several of cosmo's Windows mappings and part of
/// another. munmap releases the whole ones, fails on the part and returns an
/// error, which dlmalloc reads as nothing released: it goes on allocating from
/// freed pages. The next malloc there faults, and gcc's crash handler deadlocks
/// on the malloc lock, so the compile hangs at zero CPU. `sys_trim` opens with
/// `cmp $MAX_REQUEST, %rsi; jbe body; xor %eax, %eax; ret`; replacing the `jbe`
/// with two nops makes it always report nothing released, which dlmalloc
/// already handles. The tools are short-lived, so keeping freed memory costs
/// nothing.
///
/// `_Exit` unmaps the process's signal word and then calls TerminateProcess,
/// while the signal worker thread may still write through its pointer to it.
/// A tool that loses that race dies with an access violation after finishing
/// its work: gcc exits 0xc0000005, or reports `as` or cc1 "terminated" by
/// SIGTRAP, the low byte of that status. TerminateProcess unmaps everything
/// anyway, so the `call *UnmapViewOfFile` becomes a six-byte nop.
///
/// Both patches find their own instructions and no longer match once applied,
/// so a cache patched by an older version picks up whichever it lacks. Unix
/// has neither bug, so the tools are left alone there.
fn patch_tools(cosmocc: &Path) -> Result<(), String> {
   const SYS_TRIM: [u8; 12] = [0x48, 0x81, 0xfe, 0x7f, 0xff, 0xff, 0xff, 0x76, 0x05, 0x31, 0xc0, 0xc3];
   // `lea -0x118(%rbp), %rax; mov %rax, __sig.process(%rip)`, then the call.
   const EXIT_SWAP: [u8; 10] = [0x48, 0x8d, 0x85, 0xe8, 0xfe, 0xff, 0xff, 0x48, 0x89, 0x05];
   const EXIT_NEXT: [u8; 7] = [0x48, 0x8d, 0x8d, 0xf0, 0xfe, 0xff, 0xff];
   if !cfg!(windows) {
      return Ok(());
   }
   let marker = cosmocc.join(".patched");
   if marker.exists() {
      return Ok(());
   }
   for path in walk(cosmocc) {
      if !is_ape(&path) {
         continue;
      }
      let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
      let mut edits: Vec<(usize, &[u8])> = Vec::new();
      if let Some(at) = data.windows(SYS_TRIM.len()).position(|w| w == SYS_TRIM) {
         edits.push((at + 7, &[0x90, 0x90]));
      }
      let unmap = data.windows(EXIT_SWAP.len()).enumerate().find_map(|(at, w)| {
         let call = at + EXIT_SWAP.len() + 4;
         let ok = w == EXIT_SWAP
            && data.get(call..call + 2) == Some(&[0xff, 0x15])
            && data.get(call + 6..call + 6 + EXIT_NEXT.len()) == Some(&EXIT_NEXT);
         ok.then_some(call)
      });
      if let Some(at) = unmap {
         edits.push((at, &[0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00]));
      }
      if edits.is_empty() {
         continue;
      }
      // In place, because bin/ holds hard links that must all see the change.
      let mut f = fs::OpenOptions::new()
         .write(true)
         .open(&path)
         .map_err(|e| format!("{}: {e}", path.display()))?;
      for (at, bytes) in edits {
         f.seek(SeekFrom::Start(at as u64))
            .and_then(|_| f.write_all(bytes))
            .map_err(|e| format!("{}: {e}", path.display()))?;
      }
   }
   File::create(&marker).map_err(|e| format!("{}: {e}", marker.display()))?;
   Ok(())
}

/// Rewrite cosmocc's own binaries from APEs into native executables.
///
/// cosmocc ships every tool as an APE, and the kernel cannot exec one. A shell
/// can, which is why running apelink through `sh` works -- but only a shell
/// that parses the APE header the way cosmo's loader expects. `/bin/sh` is dash
/// on Debian and Ubuntu, which does not, and gcc's own `posix_spawnp` of `ld`,
/// `as` and `cc1` has no shell in the loop at all: it fails with "cannot
/// execute 'ld'", which rustc reports as a *warning* and still exits 0, so the
/// build succeeds and produces no binary.
///
/// Converting the toolchain once, here, means nothing downstream has to care:
/// every tool is then a native ELF that execs directly.
fn assimilate(cosmocc: &Path) -> Result<(), String> {
   // Not on macOS: the only native form assimilate can produce there is x86-64
   // Mach-O, so on Apple silicon it converts nothing and reports "macho dd
   // command for arm64 not found". It is also unnecessary: /bin/sh is bash
   // there, which parses the APE header, and every tool in the chain that
   // spawns another (gcc -> cc1, as, ld) is itself a cosmo program whose execve
   // knows how to launch an APE. The tools stay APEs and run through the shell.
   // Nor on Windows, where an APE is also a PE and runs as one.
   if cfg!(target_os = "macos") || cfg!(windows) {
      return Ok(());
   }
   let tool = cosmocc.join("bin").join("assimilate");
   if !tool.exists() {
      return Err(format!("no {} in the toolchain", tool.display()));
   }

   let mut done = 0;
   for path in walk(cosmocc) {
      // Data, not programs: object files, archives, linker scripts, headers.
      let skip = matches!(
         path.extension().and_then(|e| e.to_str()),
         Some("elf" | "a" | "o" | "h" | "c" | "lds")
      );
      // assimilate keeps the original beside its work; converting those too
      // leaves .bak.bak and duplicates the toolchain on every pass.
      let is_bak = path.to_string_lossy().contains(".bak");
      // Rewriting the converter while it is the thing doing the converting is
      // not worth the risk; nothing execs it after this.
      if skip || is_bak || path == tool || !is_ape(&path) {
         continue;
      }

      let ok = shell_exec(&tool, &path)?;
      if ok {
         done += 1;
         // The backup is a second copy of a 1.4GB toolchain, and the download
         // it came from is reproducible.
         let bak = path.with_extension(match path.extension() {
            Some(e) => format!("{}.bak", e.to_string_lossy()),
            None => "bak".to_string(),
         });
         let _ = fs::remove_file(&bak);
         let _ = fs::remove_file(path.with_file_name(format!(
            "{}.bak",
            path.file_name().unwrap_or_default().to_string_lossy()
         )));
      }
   }
   if done == 0 {
      return Err("assimilate converted nothing; the toolchain is unusable as it is".into());
   }
   Ok(())
}

/// Run an APE that has not been assimilated yet.
///
/// The shell is what makes this possible at all, and it has to be one that
/// parses the APE header: bash does, dash does not, and `/bin/sh` is dash on
/// Debian and Ubuntu. This is the only place an unconverted APE is launched --
/// after `assimilate` has run, everything is native.
fn shell_exec(tool: &Path, arg: &Path) -> Result<bool, String> {
   let shell = ["/bin/bash", "/usr/bin/bash", "/bin/sh"]
      .into_iter()
      .find(|s| Path::new(s).exists())
      .ok_or("no shell found to run the APE toolchain")?;
   let out = Command::new(shell)
      .arg(tool)
      .arg(arg)
      .output()
      .map_err(|e| format!("{shell} {}: {e}", tool.display()))?;
   Ok(out.status.success())
}

fn is_ape(path: &Path) -> bool {
   let mut buf = [0u8; 4];
   match File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut buf)) {
      Ok(()) => buf == *b"MZqF",
      Err(_) => false,
   }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
   let mut out = Vec::new();
   let mut stack = vec![dir.to_path_buf()];
   while let Some(d) = stack.pop() {
      let Ok(entries) = fs::read_dir(&d) else { continue };
      for e in entries.flatten() {
         let p = e.path();
         match e.file_type() {
            Ok(t) if t.is_dir() => stack.push(p),
            Ok(t) if t.is_file() => out.push(p),
            _ => {}
         }
      }
   }
   out
}

fn unzip(zip: &Path, into: &Path) -> Result<(), String> {
   let f = File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
   let mut ar = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", zip.display()))?;

   let mut links = Vec::new();
   for i in 0..ar.len() {
      let mut entry = ar.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
      // enclosed_name rejects paths that escape the destination; a toolchain
      // archive has no business writing outside it.
      let rel = match entry.enclosed_name() {
         Some(p) => p,
         None => continue,
      };
      let path = into.join(rel);
      if entry.is_dir() {
         fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
         continue;
      }
      if let Some(parent) = path.parent() {
         fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
      }

      // 38 entries in cosmocc.zip are symlinks -- bin/*-as, *-cpp, *-ld.bfd and
      // friends pointing at libexec or at each other. An extractor that writes
      // them as regular files produces text files holding a path, which exec
      // cannot run. It goes unnoticed because the entries that matter most
      // resolve by another route, so the toolchain half-works.
      if entry.unix_mode().map(|m| m & 0xf000 == 0xa000).unwrap_or(false) {
         let mut target = String::new();
         std::io::Read::read_to_string(&mut entry, &mut target)
            .map_err(|e| format!("{}: {e}", path.display()))?;
         links.push((path, target));
         continue;
      }

      let mut out = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
      io::copy(&mut entry, &mut out).map_err(|e| format!("{}: {e}", path.display()))?;
      drop(out);

      // Every compiler, linker and APE in here needs its executable bit back;
      // zip carries the mode and the extractor has to honour it.
      if entry.unix_mode().map(|m| m & 0o111 != 0).unwrap_or(false) {
         cache::set_exec(&path)?;
      }
   }

   // Links name links, so they are made in passes until one makes no progress.
   while !links.is_empty() {
      let before = links.len();
      let mut left = Vec::new();
      for (path, target) in links {
         if !link(&path, &target)? {
            left.push((path, target));
         }
      }
      if left.len() == before {
         let (path, target) = &left[0];
         return Err(format!("{} -> {target}: the link names nothing in the archive", path.display()));
      }
      links = left;
   }
   Ok(())
}

#[cfg(unix)]
fn link(path: &Path, target: &str) -> Result<bool, String> {
   let _ = fs::remove_file(path);
   std::os::unix::fs::symlink(target, path)
      .map_err(|e| format!("{} -> {target}: {e}", path.display()))?;
   Ok(true)
}

/// Windows makes creating a symlink a privilege, so the link becomes a hard link
/// to the file it names, or a copy of it. False while that file is itself a link
/// still to be made.
#[cfg(not(unix))]
fn link(path: &Path, target: &str) -> Result<bool, String> {
   let src = path.parent().unwrap_or(Path::new("")).join(target);
   if !src.is_file() {
      return Ok(false);
   }
   let _ = fs::remove_file(path);
   fs::hard_link(&src, path)
      .or_else(|_| fs::copy(&src, path).map(|_| ()))
      .map_err(|e| format!("{} -> {}: {e}", path.display(), src.display()))?;
   Ok(true)
}

pub(crate) fn run(cmd: &mut Command) -> Result<(), String> {
   let out = cmd.output().map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
   if out.status.success() {
      return Ok(());
   }
   Err(format!(
      "{:?} failed:\n{}{}",
      cmd.get_program(),
      String::from_utf8_lossy(&out.stdout),
      String::from_utf8_lossy(&out.stderr)
   ))
}

/// Hash the stream on its way to disk, so 440MB is not read back to verify it.
struct Tee<'a>(&'a mut File, &'a mut Sha256);

impl Write for Tee<'_> {
   fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
      let n = self.0.write(buf)?;
      self.1.update(&buf[..n]);
      Ok(n)
   }
   fn flush(&mut self) -> io::Result<()> {
      self.0.flush()
   }
}

fn hex(bytes: &[u8]) -> String {
   let mut s = String::with_capacity(bytes.len() * 2);
   for b in bytes {
      s.push_str(&format!("{b:02x}"));
   }
   s
}
