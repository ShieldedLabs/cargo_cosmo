//! Assert that the cosmo-compat in the graph matches this crate.
//!
//! The two ship as a pair: the wrap list handed to the linker is embedded here,
//! and the `__wrap_*` translators it names live in cosmo-compat. Versions are
//! published major-only and in lockstep, so agreeing on the major is the whole
//! contract. Getting it wrong produces a hundred undefined `__wrap_*` symbols
//! at link time, and nothing in that wall of output says "your versions
//! disagree" -- hence checking it here, before a two-minute build.

use std::path::Path;

/// Look for cosmo-compat in the resolved graph and compare majors.
///
/// Reads Cargo.lock rather than asking cargo: the lock is already on disk by
/// the time a build script runs, it lists the whole resolved graph including
/// target-scoped dependencies, and reading it costs nothing next to spawning
/// `cargo metadata`.
pub fn check(manifest_dir: &Path) -> Result<(), String> {
   let ours = env!("CARGO_PKG_VERSION");
   let Some(lock) = find_lock(manifest_dir) else {
      return Ok(()); // no lock to read: leave it to the linker
   };
   let Ok(text) = std::fs::read_to_string(&lock) else {
      return Ok(());
   };

   let Some(theirs) = locked_version(&text, "cosmo-compat") else {
      return Err(format!(
         "cosmo-compat is not in this project's dependency graph.\n\
          The shim is not optional -- without it the link fails with a hundred\n\
          undefined __wrap_* symbols. Add to Cargo.toml:\n\n    \
          [target.'cfg(cosmo)'.dependencies]\n    \
          cosmo-compat = \"{major}\"\n\n\
          and to the crate root:\n\n    \
          #[cfg(cosmo)]\n    extern crate cosmo_compat as _;",
         major = major(ours)
      ));
   };

   if major(&theirs) != major(ours) {
      return Err(format!(
         "cosmo-build {ours} needs cosmo-compat {}.x, but the graph resolved {theirs}.\n\
          The wrap list here and the translators there are published together;\n\
          mixing majors is a link error. Set both to \"{}\" in Cargo.toml.",
         major(ours),
         major(ours)
      ));
   }
   Ok(())
}

/// The lock lives at the workspace root, which may be several levels up.
fn find_lock(from: &Path) -> Option<std::path::PathBuf> {
   let mut dir = Some(from);
   while let Some(d) = dir {
      let lock = d.join("Cargo.lock");
      if lock.is_file() {
         return Some(lock);
      }
      dir = d.parent();
   }
   None
}

/// Pull `version` out of the `[[package]]` block naming `want`.
///
/// A three-line scan beats a TOML parser here: the lock's shape is stable, the
/// keys are unquoted and unindented, and a dependency is not worth a dependency.
fn locked_version(lock: &str, want: &str) -> Option<String> {
   let mut in_package = false;
   for line in lock.lines() {
      let line = line.trim_end();
      if line == "[[package]]" {
         in_package = false;
         continue;
      }
      if let Some(name) = line.strip_prefix("name = \"").and_then(|r| r.strip_suffix('"')) {
         in_package = name == want;
         continue;
      }
      if in_package {
         if let Some(v) = line.strip_prefix("version = \"").and_then(|r| r.strip_suffix('"')) {
            return Some(v.to_string());
         }
      }
   }
   None
}

fn major(v: &str) -> &str {
   v.split('.').next().unwrap_or(v)
}

#[cfg(test)]
mod tests {
   use super::*;

   const LOCK: &str = r#"
[[package]]
name = "ext"
version = "0.1.0"
dependencies = [
 "cosmo-compat",
]

[[package]]
name = "cosmo-compat"
version = "3.0.0"
"#;

   #[test]
   fn reads_the_locked_version() {
      assert_eq!(locked_version(LOCK, "cosmo-compat").as_deref(), Some("3.0.0"));
      assert_eq!(locked_version(LOCK, "ext").as_deref(), Some("0.1.0"));
      assert_eq!(locked_version(LOCK, "absent"), None);
   }

   /// The name of a dependency appears inside the block of whatever depends on
   /// it; only the `name =` key may claim the block.
   #[test]
   fn dependency_listing_is_not_a_package() {
      let lock = "[[package]]\nname = \"ext\"\ndependencies = [\n \"cosmo-compat\",\n]\n";
      assert_eq!(locked_version(lock, "cosmo-compat"), None);
   }

   #[test]
   fn majors() {
      assert_eq!(major("3.0.0"), "3");
      assert_eq!(major("12.1.4"), "12");
   }
}
