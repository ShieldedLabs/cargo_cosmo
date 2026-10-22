//! Build a private, localized shim object without changing the user's graph.
use crate::cache::{Cache, CHANNEL};
use crate::driver::{ape, scrub};
use crate::sha256::Sha256;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

include!("../assets/compat-sources.rs");

fn run(cmd: &mut Command) -> Result<Output, String> {
   let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
   if !out.status.success() {
      return Err(format!("{cmd:?}:\n{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
   }
   Ok(out)
}

fn tool(cache: &Cache, arch: &str, name: &str) -> Command {
   ape(&cache.bin(&format!("{arch}-linux-cosmo-{name}")))
}

pub fn build(cache: &Cache, arch: &str, spec: &Path) -> Result<PathBuf, String> {
   let mut hash = Sha256::new();
   hash.update(CHANNEL.as_bytes());
   hash.update(&fs::read(spec).map_err(|e| format!("{}: {e}", spec.display()))?);
   for (name, text) in SOURCES {
      hash.update(name.as_bytes());
      hash.update(text.as_bytes());
   }
   let key: String = hash.finalize()[..8].iter().map(|b| format!("{b:02x}")).collect();
   let dir = cache.gen.join("compat").join(arch).join(key);
   fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
   let obj = dir.join("cosmo_compat_inject-shim.o");
   if obj.is_file() { return Ok(obj); }

   // Sources live in a content-addressed, immutable directory. Atomic rename
   // makes simultaneous first builds safe; Cargo serializes its target dir.
   let source = dir.join("source");
   if !source.is_dir() {
      let temp = dir.join(format!("source-{}", std::process::id()));
      fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
      for (name, text) in SOURCES {
         let p = temp.join(name);
         fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
         fs::write(p, text).map_err(|e| e.to_string())?;
      }
      match fs::rename(&temp, &source) {
         Ok(()) => (),
         Err(_) if source.is_dir() => { fs::remove_dir_all(&temp).map_err(|e| e.to_string())?; },
         Err(e) => return Err(e.to_string()),
      }
   }
   let target = dir.join("target");
   let mut cmd = Command::new("rustup");
   scrub(&mut cmd);
   cmd.current_dir(&source);
   cmd.env_remove("CARGO_TARGET_DIR").env_remove("CARGO_BUILD_TARGET");
   cmd.args(["run", CHANNEL.trim(), "cargo", "rustc", "--release", "--lib", "--crate-type", "staticlib"])
      .arg("--manifest-path").arg(source.join("Cargo.toml"))
      .args(["-Zjson-target-spec", "-Zbuild-std=std,panic_abort"])
      .arg("--target").arg(spec).arg("--target-dir").arg(&target)
      .args(["--config", "profile.release.panic=\"abort\"", "--config", "profile.release.codegen-units=1", "--config", "profile.release.lto=false",
         "--", "--cfg", "cosmo", "--cfg", "cosmo_compat_inject"]);
   run(&mut cmd)?;
   let stem = spec.file_stem().ok_or("target spec has no stem")?;
   let lib = target.join(stem).join("release/libcosmo_compat.a");
   let nm = run(tool(cache, arch, "nm").args(["-A", "-g", "--defined-only"]).arg(&lib))?;
   let mut exports = BTreeSet::new();
   for line in String::from_utf8_lossy(&nm.stdout).lines() {
      let p: Vec<_> = line.split_whitespace().collect();
      if p.len() == 3 && p[0].contains(":cosmo_compat-") && !p[2].starts_with("_ZN") && !p[2].starts_with("_R") {
         exports.insert(p[2].to_string());
      }
   }
   let wraps = SOURCES.iter().find(|(n, _)| *n == "wrap.txt").ok_or("embedded shim lacks wrap list")?.1;
   for wrap in wraps.lines().filter(|s| !s.is_empty() && !s.starts_with('#')) {
      if !exports.contains(&format!("__wrap_{wrap}")) { return Err(format!("shim lacks __wrap_{wrap}")); }
   }
   let temp = dir.join(format!("inject-{}", std::process::id()));
   let reloc = temp.with_extension("r");
   let list = temp.with_extension("exports");
   fs::write(&list, exports.iter().cloned().collect::<Vec<_>>().join("\n") + "\n").map_err(|e| e.to_string())?;
   let result = (|| {
      let mut ld = tool(cache, arch, "ld");
      ld.args(["-r", "--gc-sections"]);
      for export in &exports { ld.arg("-u").arg(export); }
      run(ld.arg("-o").arg(&reloc).arg(&lib))?;
      run(tool(cache, arch, "objcopy").arg(format!("--keep-global-symbols={}", list.display())).arg(&reloc).arg(&temp))?;
      let nm = run(tool(cache, arch, "nm").arg("-u").arg(&temp))?;
      for line in String::from_utf8_lossy(&nm.stdout).lines() {
         let name = line.split_whitespace().last().unwrap_or("");
         if name.starts_with("_ZN") || name.starts_with("_R") { return Err(format!("shim leaks Rust reference: {name}")); }
      }
      fs::rename(&temp, &obj).map_err(|e| e.to_string())?;
      Ok(obj)
   })();
   for path in [&temp, &reloc, &list] { let _ = fs::remove_file(path); }
   result
}

#[cfg(test)]
mod tests {
   #[cfg(unix)]
   #[test]
   fn non_elf_binutils_use_shell_dispatch() {
      use std::os::unix::fs::PermissionsExt;
      let base = std::env::temp_dir().join(format!("cosmo-inject-dispatch-{}", std::process::id()));
      std::fs::create_dir_all(base.join("bin")).unwrap();
      let cache = crate::cache::Cache { root: base.clone(), cosmocc: base.clone(), gen: base.clone() };
      for name in ["nm", "ld", "objcopy"] {
         let path = cache.bin(&format!("x86_64-linux-cosmo-{name}"));
         // Executable shell text without a shebang: direct spawn returns ENOEXEC.
         std::fs::write(&path, "printf 'DISPATCH_OK:%s' \"$1\"\n").unwrap();
         std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
         let out = super::run(super::tool(&cache, "x86_64", name).arg("argument with spaces")).unwrap();
         assert_eq!(out.stdout, b"DISPATCH_OK:argument with spaces");
      }
      std::fs::remove_dir_all(base).unwrap();
   }

   #[test]
   fn embedded_sources_match_checkout() {
      for (name, text) in super::SOURCES {
         if let Ok(source) = std::fs::read_to_string(std::path::Path::new("../cosmo-compat").join(name)) {
            assert_eq!(text, &source, "stale embedded {name}: run tools/gen-assets.py");
         }
      }
   }
}
