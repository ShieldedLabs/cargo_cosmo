//! The build itself: cargo once per architecture, then apelink.

use crate::cache::{Cache, CHANNEL};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const ARCHES: [&str; 2] = ["x86_64", "aarch64"];

/// Build every executable in this package for both architectures and fuse each
/// pair into an APE. Returns the paths written.
pub fn build(
   cache: &Cache,
   manifest_dir: &Path,
   release: bool,
   args: &[&str],
) -> Result<Vec<PathBuf>, String> {
   let specs = cache.materialize()?;

   let mut per_arch = Vec::new();
   for (arch, spec) in ARCHES.iter().zip(specs.iter()) {
      per_arch.push(cargo(arch, spec, manifest_dir, release, args)?);
   }

   // Only fuse binaries that exist on both sides; an example built for one
   // arch alone cannot become an APE, and silently dropping it beats failing
   // the whole build.
   // target/cosmo/<profile>/, mirroring cargo's own target/<profile>/. A flat
   // target/cosmo/ let a debug build and a release build overwrite each other,
   // so which one you had depended on which you ran last. Build scripts only
   // ever see "debug" or "release" -- a custom profile arrives as its base.
   let outdir = manifest_dir
      .join("target")
      .join("cosmo")
      .join(if release { "release" } else { "debug" });
   fs::create_dir_all(&outdir).map_err(|e| format!("{}: {e}", outdir.display()))?;

   let mut out = Vec::new();
   for (name, x86) in &per_arch[0] {
      let arm = match per_arch[1].iter().find(|(n, _)| n == name) {
         Some((_, p)) => p,
         None => continue,
      };
      out.push(apelink(cache, name, x86, arm, &outdir)?);
   }
   if out.is_empty() {
      return Err("no executable was built for both architectures".into());
   }
   Ok(out)
}

/// Run one architecture's build; return (binary name, path) for each executable.
fn cargo(
   arch: &str,
   spec: &Path,
   manifest_dir: &Path,
   release: bool,
   args: &[&str],
) -> Result<Vec<(String, PathBuf)>, String> {
   let mut cmd = Command::new("rustup");
   cmd.args(["run", CHANNEL.trim(), "cargo", "build"]);

   // std is not shipped prebuilt for a custom target spec, so every build is a
   // build-std build. panic_unwind because cosmo provides the full _Unwind_* ABI.
   cmd.args([
      "-Zjson-target-spec",
      "-Zbuild-std=std,panic_unwind",
      "-Zbuild-std-features=compiler-builtins-mem",
   ]);
   cmd.arg("--target").arg(spec);
   cmd.arg("--message-format=json-render-diagnostics");
   if release {
      cmd.arg("--release");
   }

   // Whatever the caller wants selected: --example, --bin, --features, ...
   // A library crate has no bins at all, so `--example demo` is the difference
   // between an APE and "no executable was built for both architectures".
   cmd.args(args);

   // Two profile settings produce APEs that misbehave off Linux, and because
   // -Zbuild-std compiles std with the crate's own profile they take std down
   // with them. Both were bisected on macOS/arm64:
   //
   //   codegen-units = 1   JoinHandle::join never returns -- the child runs and
   //                       exits, the parent waits forever.
   //   lto = "fat"         the caller of thread::spawn returns with a junk frame
   //                       pointer and faults on its next stack store.
   //
   // A crate cannot express "this profile, except when building an APE", so
   // they are overridden rather than warned about. COSMO_KEEP_PROFILE=1 keeps
   // whatever the crate asked for -- use it to check whether a cosmo release
   // has fixed either bug.
   if std::env::var_os("COSMO_KEEP_PROFILE").is_none() {
      cmd.args(["--config", "profile.release.codegen-units=16"]);
      cmd.args(["--config", "profile.release.lto=false"]);
   }

   cmd.current_dir(manifest_dir);
   scrub(&mut cmd);

   // cfg(cosmo) lets a crate pick a cosmo-specific code path, and gates the
   // cosmo-compat dependency. Appended to, not replacing, any RUSTFLAGS set.
   let flags = std::env::var("COSMO_RUSTFLAGS").unwrap_or_default();
   cmd.env("RUSTFLAGS", format!("{flags} --cfg cosmo").trim().to_string());

   // The outer cargo holds an exclusive lock on its target dir; a nested cargo
   // against the same one blocks on it until killed.
   cmd.env("CARGO_TARGET_DIR", manifest_dir.join("target").join("cosmo-inner"));

   // This build runs the package's build script again. Without the flag,
   // apeify would start another pair of builds, forever.
   cmd.env("COSMO_APE_INNER", "1");

   let out = cmd.output().map_err(|e| format!("cargo: {e}"))?;
   if !out.status.success() {
      return Err(format!(
         "{arch} build failed:\n{}",
         String::from_utf8_lossy(&out.stderr)
      ));
   }

   // The artifact stream is scanned for executables rather than parsed: one
   // field is wanted out of a message format that changes shape regularly, and
   // the binary's name is its file stem, so no JSON parser earns its keep here.
   let mut found = Vec::new();
   for line in String::from_utf8_lossy(&out.stdout).lines() {
      let Some(exe) = json_string_field(line, "\"executable\":") else {
         continue;
      };
      let path = PathBuf::from(&exe);
      let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
         continue;
      };
      found.push((name, path));
   }
   Ok(found)
}

/// Pull one `"key":"value"` out of a JSON line. Returns None for `null`.
fn json_string_field(line: &str, key: &str) -> Option<String> {
   let rest = line.find(key).map(|i| &line[i + key.len()..])?;
   let rest = rest.trim_start();
   let mut chars = rest.strip_prefix('"')?.chars();

   let mut out = String::new();
   loop {
      match chars.next()? {
         '"' => return Some(out),
         '\\' => out.push(match chars.next()? {
            'n' => '\n',
            't' => '\t',
            c => c,
         }),
         c => out.push(c),
      }
   }
}

fn apelink(
   cache: &Cache,
   name: &str,
   x86: &Path,
   arm: &Path,
   outdir: &Path,
) -> Result<PathBuf, String> {
   let out = outdir.join(format!("{name}.com"));
   let bin = |n: &str| cache.bin(n);

   let mut cmd = ape(&bin("apelink"));
   cmd.arg("-V").arg("-1"); // support every OS cosmo knows
   cmd.arg("-l").arg(bin("ape-x86_64.elf"));
   cmd.arg("-l").arg(bin("ape-aarch64.elf"));
   cmd.arg("-M").arg(bin("ape-m1.c")); // Apple silicon bootstrap
   cmd.arg("-o").arg(&out);
   cmd.arg(x86).arg(arm);

   let r = cmd.output().map_err(|e| format!("apelink: {e}"))?;
   if !r.status.success() {
      return Err(format!(
         "apelink failed for {name}:\n{}{}",
         String::from_utf8_lossy(&r.stdout),
         String::from_utf8_lossy(&r.stderr)
      ));
   }

   // cosmocc verifies the PE half on every link; a malformed one fails only on
   // Windows, which is the platform you are least likely to be testing on.
   let chk = ape(&bin("pecheck"))
      .arg(&out)
      .output()
      .map_err(|e| format!("pecheck: {e}"))?;
   if !chk.status.success() {
      return Err(format!(
         "pecheck rejected {}:\n{}{}",
         out.display(),
         String::from_utf8_lossy(&chk.stdout),
         String::from_utf8_lossy(&chk.stderr)
      ));
   }
   Ok(out)
}

/// Build a command that can actually exec an APE.
///
/// The kernel cannot exec one directly -- no ELF magic up front and no shebang
/// -- which is fine from a shell, because the APE header is also valid /bin/sh,
/// but spawning it as a program fails with ENOEXEC. Going through sh is what
/// the format expects. Native ELF files are run directly.
pub fn ape(path: &Path) -> Command {
   let elf = fs::read(path)
      .map(|b| b.starts_with(b"\x7fELF"))
      .unwrap_or(false);
   if elf {
      return Command::new(path);
   }
   let mut cmd = Command::new("/bin/sh");
   cmd.arg(path);
   cmd
}

/// Drop the toolchain the outer cargo pinned into our environment.
///
/// Inherited, these override the nested `rustup run <nightly> cargo` and the
/// inner build dies with "1 nightly option were parsed" from a stable rustc.
fn scrub(cmd: &mut Command) {
   for key in [
      "RUSTC",
      "RUSTDOC",
      "RUSTC_WRAPPER",
      "RUSTC_WORKSPACE_WRAPPER",
      "RUSTUP_TOOLCHAIN",
      "CARGO",
      "RUSTFLAGS",
      "CARGO_ENCODED_RUSTFLAGS",
      "CARGO_MAKEFLAGS",
   ] {
      cmd.env_remove(key);
   }
}
