//! Acquiring the two toolchains: the pinned Rust nightly and cosmocc.

use crate::cache::{self, Cache, CHANNEL};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Write};
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
      return Ok(());
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

   if !cache.bin("apelink").exists() {
      return Err(format!(
         "cosmocc unpacked into {} but bin/apelink is missing",
         cache.cosmocc.display()
      ));
   }
   Ok(())
}

fn unzip(zip: &Path, into: &Path) -> Result<(), String> {
   let f = File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
   let mut ar = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", zip.display()))?;

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
      let mut out = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
      io::copy(&mut entry, &mut out).map_err(|e| format!("{}: {e}", path.display()))?;
      drop(out);

      // Every compiler, linker and APE in here needs its executable bit back;
      // zip carries the mode and the extractor has to honour it.
      #[cfg(unix)]
      if entry.unix_mode().map(|m| m & 0o111 != 0).unwrap_or(false) {
         cache::set_exec(&path)?;
      }
   }
   Ok(())
}

fn run(cmd: &mut Command) -> Result<(), String> {
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
