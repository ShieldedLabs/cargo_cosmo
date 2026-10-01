//! Where the toolchain and the generated glue live, and how they get there.

use crate::driver::{ape, ARCHES};
use crate::sha256::Sha256;
use crate::toolchain::run;
use std::env::consts::EXE_SUFFIX;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Target specs and the compiler and linker drivers are generated rather than
/// shipped ready-to-use: the spec has to name an absolute path to the linker,
/// and the drivers have to name an absolute path to the toolchain. Both are
/// known only once the cache location is.
const SPEC_X86: &str = include_str!("../assets/x86_64-unknown-cosmo.json");
const SPEC_ARM: &str = include_str!("../assets/aarch64-unknown-cosmo.json");
const SHIM: &str = include_str!("../assets/shim.rs");

/// The libc symbols cosmo-compat translates. The linker driver adds a --wrap for
/// each, and the copy of libcosmo it links has each one renamed.
pub const WRAP_LIST: &str = include_str!("../assets/wrap.txt");

/// The nightly these specs were generated against. Custom target JSON is
/// schema-checked strictly and the schema drifts between nightlies, so the
/// build is pinned to the one the specs match rather than to whatever nightly
/// is newest. Regenerate the assets with tools/gen-target-specs.py when moving.
pub const CHANNEL: &str = include_str!("../assets/channel.txt");

pub struct Cache {
   pub root: PathBuf,
   pub cosmocc: PathBuf,
   pub gen: PathBuf,
}

/// What [`Cache::materialize`] produced for one build.
pub struct Glue {
   /// One target spec per entry of [`ARCHES`], in the same order.
   pub specs: [PathBuf; 2],
   /// The directory holding the compiler and linker drivers.
   pub tools: PathBuf,
}

impl Glue {
   pub fn tool(&self, name: &str) -> PathBuf {
      self.tools.join(format!("{name}{EXE_SUFFIX}"))
   }
}

impl Cache {
   /// Everything downloaded is shared between projects, so a second project
   /// costs no disk. cosmocc sits at the root because it is version-agnostic
   /// and 1.4GB; the generated glue is namespaced per crate version so an
   /// upgrade cannot read a stale spec.
   pub fn locate() -> Result<Cache, String> {
      let root = match std::env::var_os("COSMO_HOME") {
         Some(d) => PathBuf::from(d),
         None => base_cache_dir()?.join("cargo-cosmo"),
      };
      Ok(Cache {
         cosmocc: root.join("cosmocc"),
         gen: root.join(concat!("v", env!("CARGO_PKG_VERSION"))),
         root,
      })
   }

   pub fn bin(&self, name: &str) -> PathBuf {
      self.cosmocc.join("bin").join(name)
   }

   /// Build the drivers and the renamed libcosmo if they are missing, write
   /// both target specs, and hand back where it all is. Cheap when everything
   /// is already there, which also repairs a cache someone has moved or
   /// half-deleted.
   pub fn materialize(&self) -> Result<Glue, String> {
      fs::create_dir_all(&self.gen).map_err(|e| format!("{}: {e}", self.gen.display()))?;

      let wraps: Vec<&str> = WRAP_LIST
         .lines()
         .map(str::trim)
         .filter(|l| !l.is_empty() && !l.starts_with('#'))
         .collect();
      let libcosmo = self.unwrapped_libcosmo(&wraps)?;
      let tools = self.drivers(&wraps, &libcosmo)?;
      let glue = Glue { specs: [PathBuf::new(), PathBuf::new()], tools };

      let mut specs = Vec::new();
      for (arch, spec) in [("x86_64", SPEC_X86), ("aarch64", SPEC_ARM)] {
         let ld = glue.tool(&format!("cosmo-ld-{arch}"));
         let path = self.gen.join(format!("{arch}-unknown-cosmo.json"));
         let text = spec.replace("@COSMO_LD@", &escape_json(&ld.to_string_lossy()));
         // Rewritten only when it changed: a concurrent build may be reading it.
         if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
         }
         specs.push(path);
      }
      Ok(Glue { specs: [specs.remove(0), specs.remove(0)], ..glue })
   }

   /// A copy of each architecture's libcosmo in which every wrapped symbol NAME
   /// is renamed to `__cosmo_real_NAME`, definition and internal references
   /// alike; cosmo-compat calls the real function by that name. `--wrap`
   /// redirects undefined references from every object in the link, libcosmo's
   /// own included, so with the stock archive cosmo's own calls to itself would
   /// go through the translators too. Keyed by the wrap list, since the objcopy
   /// pass over the whole archive is not free: over a minute per architecture
   /// on Windows.
   fn unwrapped_libcosmo(&self, wraps: &[&str]) -> Result<PathBuf, String> {
      let dir = self.gen.join(format!("libcosmo-{}", short_hash(&[&wraps.join(" ")])));
      if wraps.is_empty() {
         return Ok(dir);
      }
      for arch in ARCHES {
         let out = dir.join(format!("{arch}.a"));
         if out.is_file() {
            continue;
         }
         fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

         // Concurrent builds each write their own temp file; the rename is atomic.
         let pid = std::process::id();
         let syms = dir.join(format!("{arch}.syms.{pid}"));
         let tmp = dir.join(format!("{arch}.a.{pid}"));
         let text: String = wraps.iter().map(|s| format!("{s} __cosmo_real_{s}\n")).collect();
         fs::write(&syms, text).map_err(|e| format!("{}: {e}", syms.display()))?;
         let lib = self.cosmocc.join(format!("{arch}-linux-cosmo")).join("lib").join("libcosmo.a");
         let r = run(
            ape(&self.bin(&format!("{arch}-linux-cosmo-objcopy")))
               .arg(format!("--redefine-syms={}", syms.display()))
               .arg(&lib)
               .arg(&tmp),
         );
         let _ = fs::remove_file(&syms);
         r?;
         settle(&tmp, &out)?;
      }
      Ok(dir)
   }

   /// Compile assets/shim.rs with the host's rustc and link it under every name
   /// it answers to. The directory is keyed by everything baked into the
   /// program, so a changed source or a moved cache gets a fresh one rather
   /// than a stale one, and an unchanged one is never rebuilt -- which matters
   /// on Windows, where a program that a concurrent build is running cannot be
   /// replaced.
   fn drivers(&self, wraps: &[&str], libcosmo: &Path) -> Result<PathBuf, String> {
      let cosmocc = self.cosmocc.to_string_lossy();
      let libcosmo = libcosmo.to_string_lossy();
      let wraps = wraps.join(" ");
      let key = short_hash(&[SHIM, &cosmocc, &libcosmo, &wraps]);
      let dir = self.gen.join(format!("tools-{key}"));
      if dir.is_dir() {
         return Ok(dir);
      }

      // Built beside its final place and renamed into it whole, so a build that
      // dies half way never leaves a directory that looks finished.
      let tmp = self.gen.join(format!("tools-{key}.{}", std::process::id()));
      let _ = fs::remove_dir_all(&tmp);
      fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
      let src = tmp.join("cosmo-shim.rs");
      fs::write(&src, SHIM).map_err(|e| format!("{}: {e}", src.display()))?;

      // cargo hands every build script the rustc it is building with.
      let exe = tmp.join(format!("cosmo-shim{EXE_SUFFIX}"));
      let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
      run(Command::new(rustc)
         .args(["--edition", "2021", "-O", "--crate-name", "cosmo_shim", "-o"])
         .arg(&exe)
         .arg(&src)
         .env("COSMO_SHIM_COSMOCC", &*cosmocc)
         .env("COSMO_SHIM_LIBCOSMO", &*libcosmo)
         .env("COSMO_SHIM_WRAPS", &wraps))?;

      for arch in ARCHES {
         for name in [
            format!("cosmo-ld-{arch}"),
            format!("{arch}-unknown-cosmo-cc"),
            format!("{arch}-unknown-cosmo-c++"),
            format!("{arch}-unknown-cosmo-ar"),
         ] {
            let link = tmp.join(format!("{name}{EXE_SUFFIX}"));
            fs::hard_link(&exe, &link)
               .or_else(|_| fs::copy(&exe, &link).map(|_| ()))
               .map_err(|e| format!("{}: {e}", link.display()))?;
         }
      }

      match fs::rename(&tmp, &dir) {
         Ok(()) => Ok(dir),
         // Another build got there first with the same key, so the same contents.
         Err(_) if dir.is_dir() => {
            let _ = fs::remove_dir_all(&tmp);
            Ok(dir)
         }
         Err(e) => Err(format!("{} -> {}: {e}", tmp.display(), dir.display())),
      }
   }
}

/// Move a finished temp file into place. Losing a race to an identical file is
/// success: on Windows the rename fails when a concurrent link has it open.
fn settle(tmp: &Path, out: &Path) -> Result<(), String> {
   match fs::rename(tmp, out) {
      Ok(()) => Ok(()),
      Err(_) if out.is_file() => {
         let _ = fs::remove_file(tmp);
         Ok(())
      }
      Err(e) => Err(format!("{} -> {}: {e}", tmp.display(), out.display())),
   }
}

fn short_hash(parts: &[&str]) -> String {
   let mut h = Sha256::new();
   for p in parts {
      h.update(p.as_bytes());
      h.update(&[0]);
   }
   h.finalize()[..8].iter().map(|b| format!("{b:02x}")).collect()
}

fn base_cache_dir() -> Result<PathBuf, String> {
   if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
      return Ok(PathBuf::from(d));
   }
   // Windows has no XDG convention and often no HOME; its per-user cache root is
   // LOCALAPPDATA.
   if cfg!(windows) {
      if let Some(d) = std::env::var_os("LOCALAPPDATA") {
         return Ok(PathBuf::from(d));
      }
   }
   match std::env::var_os("HOME") {
      Some(h) => Ok(PathBuf::from(h).join(".cache")),
      None => Err("none of COSMO_HOME, XDG_CACHE_HOME, LOCALAPPDATA or HOME is set".into()),
   }
}

fn escape_json(s: &str) -> String {
   s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn set_exec(path: &Path) -> Result<(), String> {
   #[cfg(unix)]
   {
      use std::os::unix::fs::PermissionsExt;
      let mut perm = fs::metadata(path)
         .map_err(|e| format!("{}: {e}", path.display()))?
         .permissions();
      perm.set_mode(perm.mode() | 0o755);
      fs::set_permissions(path, perm).map_err(|e| format!("{}: {e}", path.display()))?;
   }
   let _ = path;
   Ok(())
}
