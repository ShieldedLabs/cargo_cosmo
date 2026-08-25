//! Where the toolchain and the generated glue live, and how they get there.

use std::fs;
use std::path::{Path, PathBuf};

/// Target specs, the linker shim and the wrap list are generated rather than
/// shipped ready-to-use: the spec has to name an absolute path to the linker,
/// and the linker has to name an absolute path to the toolchain. Both are
/// known only once the cache location is.
const SPEC_X86: &str = include_str!("../assets/x86_64-unknown-cosmo.json");
const SPEC_ARM: &str = include_str!("../assets/aarch64-unknown-cosmo.json");
const COSMO_LD: &str = include_str!("../assets/cosmo-ld.sh");

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

   /// Write the linker shim and both target specs, then hand back the spec
   /// paths. Cheap enough to redo on every build, which also repairs a cache
   /// someone has moved or half-deleted.
   pub fn materialize(&self) -> Result<[PathBuf; 2], String> {
      fs::create_dir_all(&self.gen).map_err(|e| format!("{}: {e}", self.gen.display()))?;

      let mut specs = Vec::new();
      for (arch, spec) in [("x86_64", SPEC_X86), ("aarch64", SPEC_ARM)] {
         // One shim per arch: cosmo-ld reads the architecture out of its own
         // name, exactly as the shell version in tools/ does.
         let ld = self.gen.join(format!("cosmo-ld-{arch}"));
         write_exec(&ld, &COSMO_LD.replace("@COSMOCC@", &shell_quote(&self.cosmocc)))?;

         let path = self.gen.join(format!("{arch}-unknown-cosmo.json"));
         let text = spec.replace("@COSMO_LD@", &escape_json(&ld.to_string_lossy()));
         fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
         specs.push(path);
      }
      Ok([specs.remove(0), specs.remove(0)])
   }

}

fn base_cache_dir() -> Result<PathBuf, String> {
   if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
      return Ok(PathBuf::from(d));
   }
   match std::env::var_os("HOME") {
      Some(h) => Ok(PathBuf::from(h).join(".cache")),
      None => Err("neither COSMO_HOME, XDG_CACHE_HOME nor HOME is set".into()),
   }
}

/// Single-quote a path for the shell: the cache can sit under a home directory
/// with a space in it, and the shim assigns this to a variable unquoted.
fn shell_quote(p: &Path) -> String {
   format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

fn escape_json(s: &str) -> String {
   s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn write_exec(path: &Path, text: &str) -> Result<(), String> {
   fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
   set_exec(path)
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
