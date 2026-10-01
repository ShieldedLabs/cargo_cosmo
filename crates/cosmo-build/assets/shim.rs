//! The compiler and linker drivers for the cosmo builds, in place of cosmocc's
//! shell scripts.
//!
//! cosmo-build compiles this with the host's rustc and links it under several
//! names; the name it runs as picks the job:
//!
//! | name                       | job                                          |
//! |----------------------------|----------------------------------------------|
//! | `cosmo-ld-<arch>`          | rustc's linker for one architecture's build  |
//! | `<arch>-unknown-cosmo-cc`  | cc-rs's C compiler, a port of cosmocross     |
//! | `<arch>-unknown-cosmo-c++` | the same for C++                             |
//! | `<arch>-unknown-cosmo-ar`  | cosmocc's archiver                           |
//!
//! It is a program rather than a script because nothing on Windows can exec a
//! script: rustc and cc-rs both spawn their tools directly. Everything it runs
//! in turn is a cosmocc tool, which Windows runs natively, because an APE is
//! also a PE.
//!
//! The paths and the wrap list are baked in at compile time, so the program is
//! complete on its own and a stale copy can never pair with a newer toolchain.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const COSMOCC: &str = env!("COSMO_SHIM_COSMOCC");
const LIBCOSMO: &str = env!("COSMO_SHIM_LIBCOSMO");
const WRAPS: &str = env!("COSMO_SHIM_WRAPS");

/// What cosmocross reports for `-dumpversion`; it is the gcc cosmocc ships.
const GCC_VERSION: &str = "14.1.0";

/// Past this many bytes of arguments the gcc command line goes through a
/// response file. Windows caps a whole command line at 32,767 UTF-16 units, and
/// a link line naming every rlib of a large crate graph passes that easily.
const RESPONSE_FILE_AT: usize = 30_000;

fn main() {
   let mut argv = env::args();
   let argv0 = PathBuf::from(argv.next().unwrap_or_default());
   let name = argv0.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
   let args: Vec<String> = argv.collect();

   let code = if let Some(arch) = name.strip_prefix("cosmo-ld-") {
      ld(arch, args)
   } else if let Some(arch) = name.strip_suffix("-unknown-cosmo-cc") {
      cc(arch, &name, false, args)
   } else if let Some(arch) = name.strip_suffix("-unknown-cosmo-c++") {
      cc(arch, &name, true, args)
   } else if let Some(arch) = name.strip_suffix("-unknown-cosmo-ar") {
      run(&bin(&format!("{arch}-linux-cosmo-ar")), &args)
   } else {
      die(&format!("cosmo-shim: run as an unknown name '{name}'"))
   };
   exit(code);
}

fn bin(name: &str) -> PathBuf {
   Path::new(COSMOCC).join("bin").join(name)
}

fn lib(arch: &str) -> PathBuf {
   Path::new(COSMOCC).join(format!("{arch}-linux-cosmo")).join("lib")
}

fn path_arg(prefix: &str, p: &Path) -> String {
   format!("{prefix}{}", p.display())
}

/// Link a rustc-produced object set into a cosmopolitan ELF for one architecture.
///
/// rustc drives this with the gnu-cc flavor, so it hands over a GCC command line
/// aimed at a normal musl toolchain. Two things have to happen: strip the args
/// that fight cosmo's model, and inject cosmo's CRT, linker script and libc in
/// the right order.
fn ld(arch: &str, args: Vec<String>) -> i32 {
   let lib = lib(arch);
   let cc = bin(&format!("{arch}-linux-cosmo-gcc"));
   if !cc.exists() {
      die(&format!("cosmo-ld: no compiler at {}", cc.display()));
   }

   // Mirrors LDFLAGS/LDFLAGS_$ARCH in cosmocc's own cosmocc script, in the same
   // order, so a diff against it stays readable when cosmo updates.
   let (crt, arch_flags): (Vec<String>, Vec<String>) = match arch {
      "x86_64" => (
         vec![path_arg("", &lib.join("ape.o")), path_arg("", &lib.join("crt.o"))],
         vec![
            path_arg("-Wl,-T,", &lib.join("ape.lds")),
            "-Wl,-z,common-page-size=4096".into(),
            "-Wl,-z,max-page-size=16384".into(),
         ],
      ),
      // No ape.o here: the aarch64 half is a plain ELF that apelink embeds.
      "aarch64" => (
         vec![path_arg("", &lib.join("crt.o"))],
         vec![
            path_arg("-Wl,-T,", &lib.join("aarch64.lds")),
            "-Wl,-z,common-page-size=16384".into(),
            "-Wl,-z,max-page-size=16384".into(),
         ],
      ),
      _ => die(&format!("cosmo-ld: unsupported arch '{arch}'")),
   };

   let mut output = None;
   let mut rest = Vec::new();
   let mut it = expand_response_files(args).into_iter();
   while let Some(a) = it.next() {
      match a.as_str() {
         // Captured and re-emitted first, so the CRT objects land ahead of
         // every user object.
         "-o" => output = it.next(),
         _ if a.starts_with("-o") => output = Some(a[2..].to_string()),

         // cosmo is non-PIE by construction.
         "-pie" | "-Wl,-pie" | "-Wl,--pic-executable" => {}

         // cosmocc links -z norelro; rustc's musl default asks for the
         // opposite, both as separate args and pre-joined as one -Wl,a,b,c.
         _ if a.starts_with("-Wl,-z,relro") || a.starts_with("-Wl,-z,now") => {}
         "-Wl,-z,defs" => {}

         // -lcosmo supplies libc, libm, pthreads, dl and the compiler runtime.
         // Letting any of these through pulls in host musl and duplicates
         // symbols.
         "-lc" | "-lm" | "-ldl" | "-lrt" | "-lutil" | "-lpthread" | "-lgcc" | "-lgcc_s"
         | "-lunwind" => {}

         // bfd and static linkage are forced below.
         _ if a.starts_with("-fuse-ld=") => {}
         "-Wl,-Bdynamic" | "-shared" | "-rdynamic" => {}

         // Already implied by -nostdlib.
         "-nodefaultlibs" | "-nostartfiles" | "-nostdlib" => {}

         _ => rest.push(a),
      }
   }
   let Some(output) = output else { die("cosmo-ld: no -o in link line") };

   // COSMO_LD_DEBUG=<file> records the reconciled link line. It is the only way
   // to see what rustc actually handed over -- which rlibs, in which order, and
   // whether fat LTO merged them into one object before the linker saw them.
   if let Some(log) = env::var_os("COSMO_LD_DEBUG") {
      let mut text = rest.join(" ");
      text.push('\n');
      let _ = fs::OpenOptions::new()
         .create(true)
         .append(true)
         .open(log)
         .and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
   }

   let mut line = vec!["-o".to_string(), output.clone()];
   line.extend(crt);
   for f in ["-static", "-nostdlib", "-no-pie", "-fuse-ld=bfd", "-Wl,-z,noexecstack"] {
      line.push(f.into());
   }
   line.extend(["-Wl,-z,norelro".into(), "-Wl,--gc-sections".into(), path_arg("-L", &lib)]);
   line.extend(arch_flags);

   // Route every libc call std makes through cosmo-compat's __wrap_*
   // translators, and link the copy of libcosmo in which every wrapped NAME is
   // renamed to __cosmo_real_NAME, definition and internal references alike.
   // --wrap redirects undefined references from every object in the link,
   // libcosmo's own included, so with the stock archive cosmo's routines called
   // the translators and got Linux-numbered results back where they expect the
   // host's. realpath is the case that was measured: its internal readlink came
   // back with errno 22, cosmo compared that against the host's EINVAL (87 on
   // Windows), and every canonicalize failed.
   let wraps: Vec<&str> = WRAPS.split_whitespace().collect();
   line.extend(wraps.iter().map(|w| format!("-Wl,--wrap={w}")));
   if wraps.is_empty() {
      line.extend(rest);
   } else {
      line.extend(rest.into_iter().map(|a| native_rlib(arch, a, &wraps)));
   }
   if wraps.is_empty() {
      line.push("-lcosmo".into());
   } else {
      line.push(path_arg("", &Path::new(LIBCOSMO).join(format!("{arch}.a"))));
   }

   let code = gcc(&cc, &line, Path::new(&output));
   if code != 0 {
      return code;
   }

   // cosmocc runs this on every linked image; it rewrites the ELF into the
   // shape apelink and the APE loader expect. Skipping it produces a binary
   // that links cleanly and then crashes on start.
   run(&bin("fixupobj"), &[output])
}

/// The same rename for the C code rustc bundles inside rlibs (rocksdb and its
/// friends): those objects call the wrapped names too, and --wrap would route
/// them through translators written for Rust's Linux-numbered ABI. C speaks
/// cosmo's own host-numbered ABI, so its flags, errno values and struct layouts
/// come back wrong -- stat's layout is the measured case, and it breaks the
/// database that way. An rlib carrying native objects gets a renamed copy beside
/// it, rebuilt when the rlib is newer, and that copy is linked instead. Pure Rust
/// rlibs keep their references, which is what they want: those calls do need
/// translating.
fn native_rlib(arch: &str, arg: String, wraps: &[&str]) -> String {
   if !arg.ends_with(".rlib") {
      return arg;
   }
   let members = command(&bin(&format!("{arch}-linux-cosmo-ar")))
      .arg("t")
      .arg(&arg)
      .output()
      .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
      .unwrap_or_default();
   let native = members.lines().map(str::trim_end).any(|m| m.ends_with(".o") && !m.ends_with(".rcgu.o"));
   if !native {
      return arg;
   }

   let out = format!("{arg}.native.a");
   let modified = |p: &str| fs::metadata(p).and_then(|m| m.modified()).ok();
   if modified(&out).is_some() && modified(&out) >= modified(&arg) {
      return out;
   }
   let tmp = format!("{out}.{}", std::process::id());
   let syms = format!("{tmp}.syms");
   let text: String = wraps.iter().map(|w| format!("{w} __cosmo_real_{w}
")).collect();
   if let Err(e) = fs::write(&syms, text) {
      die(&format!("cosmo-ld: {syms}: {e}"));
   }
   let code = run(
      &bin(&format!("{arch}-linux-cosmo-objcopy")),
      &[format!("--redefine-syms={syms}"), arg.clone(), tmp.clone()],
   );
   let _ = fs::remove_file(&syms);
   if code != 0 {
      let _ = fs::remove_file(&tmp);
      die(&format!("cosmo-ld: renaming wrapped symbols in {arg} failed"));
   }
   // Another link may have put the same copy in place meanwhile, and Windows
   // will not replace a file that is open.
   if fs::rename(&tmp, &out).is_err() {
      let _ = fs::remove_file(&tmp);
      if !Path::new(&out).is_file() {
         die(&format!("cosmo-ld: could not write {out}"));
      }
   }
   out
}

/// cosmocross, the driver behind cosmocc's `<arch>-unknown-cosmo-cc` names,
/// ported from the shell for the jobs cc-rs gives it: compiling, preprocessing
/// and the occasional link. Its -mtiny, -mdbg and -moptlinux library variants
/// are not carried over; nothing in a cargo build asks for them.
///
/// `COSMO_CFLAGS` and `COSMO_CXXFLAGS` add flags for C and C++ sources but never
/// for assembly. cc-rs hands its own CFLAGS to `.S` files as well, and some
/// flags (`-include` of a C header, most of all) make the assembler choke on C
/// declarations it cannot parse.
fn cc(arch: &str, prog: &str, plus: bool, args: Vec<String>) -> i32 {
   let include = Path::new(COSMOCC).join("include");
   let lib = lib(arch);

   if args.first().map(String::as_str) == Some("--version") {
      println!("{prog} (GCC) {GCC_VERSION}");
      return 0;
   }

   let mut x = None;
   let mut need_x = false;
   for a in &args {
      if need_x {
         need_x = false;
         x = Some(a.clone());
      } else if a == "-x" {
         need_x = true;
      } else if let Some(lang) = a.strip_prefix("-x") {
         x = Some(lang.to_string());
      }
   }
   let plus = match x.as_deref() {
      Some("c") | Some("c-header") => false,
      Some("c++") | Some("c++-header") => true,
      _ => plus,
   };

   let asm = args.iter().any(|a| a.ends_with(".S") || a.ends_with(".s"));
   let mut user = Vec::new();
   if !asm {
      let extra = env::var(if plus { "COSMO_CXXFLAGS" } else { "COSMO_CFLAGS" }).unwrap_or_default();
      user.extend(extra.split_whitespace().map(String::from));
   }
   user.extend(args);

   let platform = ["-D__COSMOPOLITAN__", "-D__COSMOCC__"];
   let predef = ["-include", "libc/integral/normalize.inc"];
   let mut cflags: Vec<String> = vec!["-fportcosmo".into(), "-fno-semantic-interposition".into()];
   let mut cppflags: Vec<String> = vec!["-fno-pie".into(), "-nostdinc".into(), "-isystem".into(), path_arg("", &include)];
   let mut ldflags: Vec<String> =
      ["-static", "-no-pie", "-nostdlib", "-fuse-ld=bfd", "-Wl,-z,noexecstack"].map(String::from).to_vec();
   let precious = "-fno-omit-frame-pointer";

   let mut compiler = bin(&format!("{arch}-linux-cosmo-gcc"));
   let mut crt = vec![path_arg("", &lib.join("crt.o"))];
   let mut ldlibs = vec!["-lcosmo".to_string()];
   ldflags.push(path_arg("-L", &lib));
   if plus {
      compiler = bin(&format!("{arch}-linux-cosmo-g++"));
      cppflags.splice(0..0, ["-isystem".into(), path_arg("", &include.join("third_party").join("libcxx"))]);
      ldlibs.insert(0, "-lcxx".into());
   } else {
      cflags.push("-Wno-implicit-int".into());
   }

   let pagesz = match arch {
      "x86_64" => {
         crt.insert(0, path_arg("", &lib.join("ape-no-modify-self.o")));
         cflags.push("-mno-tls-direct-seg-refs".into());
         ldflags.push(path_arg("-Wl,-T,", &lib.join("ape.lds")));
         cppflags.push("-mno-red-zone".into());
         4096
      }
      "aarch64" => {
         cppflags.push("-fsigned-char".into());
         cflags.extend(["-ffixed-x18".into(), "-ffixed-x28".into()]);
         ldflags.push(path_arg("-Wl,-T,", &lib.join("aarch64.lds")));
         16384
      }
      _ => die(&format!("{prog}: {arch}: unsupported architecture")),
   };
   ldflags.push(format!("-Wl,-z,common-page-size={pagesz}"));
   ldflags.push("-Wl,-z,max-page-size=16384".into());

   #[derive(PartialEq)]
   enum Intent { Ld, Cc, S, Cpp, H }
   let mut intent = Intent::Ld;
   let mut opt = String::new();
   let mut output = None;
   let mut strip = false;
   let mut relocatable = false;
   let mut got_some = false;
   let mut need_output = false;
   let mut pass = Vec::new();
   for a in user {
      if need_output {
         need_output = false;
         output = Some(a.clone());
         pass.push(a);
         continue;
      }
      match a.as_str() {
         _ if a == "-" || !a.starts_with('-') => got_some = true,
         "-static-libstdc++" | "-static-libgcc" => continue,
         _ if a.starts_with("-O") => opt = a.clone(),
         "-c" => intent = Intent::Cc,
         "-S" => intent = Intent::S,
         "-s" => {
            strip = true;
            continue;
         }
         "-r" => relocatable = true,
         "-E" | "-M" | "-MM" => intent = Intent::Cpp,
         "-o" => need_output = true,
         "-mcosmo" => {
            cppflags.push("-D_COSMO_SOURCE".into());
            continue;
         }
         "-mdbg" | "-mtiny" | "-moptlinux" | "-m64" => continue,
         _ if a.starts_with("-o") => output = Some(a[2..].to_string()),
         "-fpic" | "-fPIC" => continue,
         // No support for position independent executables:
         // https://github.com/jart/cosmopolitan/issues/1126
         "-fpie" | "-pie" => continue,
         "-shared" | "-nostdlib" | "-mred-zone" | "-fsanitize=thread" => {
            eprintln!("{prog}: {a} not supported");
            return 1;
         }
         // Quoth Apple: "The frame pointer register must always address a
         // valid frame record. Some functions -- such as leaf functions or tail
         // calls -- may opt not to create an entry in this list. As a result,
         // stack traces are always meaningful, even without debug information."
         "-fomit-frame-pointer" => {
            pass.extend(["-momit-leaf-frame-pointer".into(), "-foptimize-sibling-calls".into()]);
            continue;
         }
         "-dumpversion" => {
            println!("{GCC_VERSION}");
            return 0;
         }
         "-Wl,--version" | "-dumpmachine" => got_some = true,
         _ => {}
      }
      pass.push(a);
   }
   if !got_some {
      eprintln!("{prog}: fatal error: no input files\ncompilation terminated.");
      return 1;
   }
   if relocatable {
      ldflags.push("-r".into());
   }

   // Precompiled header mode.
   if intent != Intent::Cpp {
      let header_only = match x.as_deref() {
         None => pass.iter().all(|a| a.starts_with('-') || a.ends_with(".h") || a.ends_with(".hpp")),
         Some(x) => x == "c-header" || x == "c++-header",
      };
      if header_only {
         intent = Intent::H;
      }
   }

   // --ftrace support unless optimizing for size.
   if opt != "-Os" {
      match arch {
         "x86_64" => cflags.push("-fpatchable-function-entry=18,16".into()),
         _ => cflags.push("-fpatchable-function-entry=7,6".into()),
      }
      cflags.push("-fno-inline-functions-called-once".into());
   }
   if opt != "-O3" {
      cflags.push("-fno-schedule-insns2".into());
   }

   let mut line: Vec<String> = platform.map(String::from).to_vec();
   match intent {
      Intent::Cpp => {
         line.extend(cppflags);
         line.extend(pass);
      }
      Intent::Cc | Intent::S | Intent::H => {
         line.extend(predef.map(String::from));
         line.extend(cflags);
         line.extend(cppflags);
         line.extend(pass);
         line.push(precious.into());
      }
      Intent::Ld => {
         line.extend(predef.map(String::from));
         line.extend(cflags);
         line.extend(cppflags);
         line.extend(crt);
         line.extend(pass);
         line.extend(ldflags);
         line.extend(ldlibs);
         line.push(precious.into());
      }
   }

   let out = output.as_deref().map(Path::new).unwrap_or(Path::new("a.out"));
   let code = gcc(&compiler, &line, out);
   if code != 0 {
      return code;
   }

   let Some(output) = output else { return 0 };
   if !Path::new(&output).is_file() {
      return 0;
   }
   if intent == Intent::Cc || intent == Intent::Ld {
      let code = run(&bin("fixupobj"), &[output.clone()]);
      if code != 0 {
         return code;
      }
   }
   if intent == Intent::Ld {
      if output.ends_with(".com") || output.ends_with(".exe") {
         // cosmocc -o foo.com leaves foo.com (the APE) and foo.com.dbg (the ELF).
         let dbg = format!("{output}.dbg");
         if let Err(e) = fs::rename(&output, &dbg) {
            eprintln!("{prog}: {output}: {e}");
            return 1;
         }
         let objcopy = bin(&format!("{arch}-linux-cosmo-objcopy"));
         let flags: &[&str] = if arch == "x86_64" { &["-S", "-O", "binary"] } else { &["-S"] };
         let mut a: Vec<String> = flags.iter().map(|s| s.to_string()).collect();
         a.extend([dbg.clone(), output.clone()]);
         let code = run(&objcopy, &a);
         if code != 0 {
            return code;
         }
         return run(&bin("zipcopy"), &[dbg, output]);
      } else if strip {
         return run(&bin(&format!("{arch}-linux-cosmo-strip")), &[output]);
      }
   }
   0
}

/// Run gcc, through a response file when the line is too long for the host.
///
/// gcc reads `@file` itself, and once it has been given one it hands collect2
/// and ld their arguments the same way, so the limit never comes back further
/// down. The escaping is libiberty's, which is also what rustc writes for a
/// gnu-cc linker.
fn gcc(compiler: &Path, line: &[String], output: &Path) -> i32 {
   let size: usize = line.iter().map(|a| a.len() + 1).sum();
   if size < RESPONSE_FILE_AT {
      return run(compiler, line);
   }

   let mut text = String::with_capacity(size * 2);
   for a in line {
      for c in a.chars() {
         if c == '\\' || c == '"' || c == '\'' || c.is_whitespace() {
            text.push('\\');
         }
         text.push(c);
      }
      text.push('\n');
   }
   let mut rsp = output.as_os_str().to_owned();
   rsp.push(".cosmo.rsp");
   let rsp = PathBuf::from(rsp);
   if let Err(e) = fs::write(&rsp, text) {
      die(&format!("cosmo-shim: {}: {e}", rsp.display()));
   }
   let code = run(compiler, &[path_arg("@", &rsp)]);
   let _ = fs::remove_file(&rsp);
   code
}

/// Replace every `@file` argument with the arguments it holds, as gcc would.
///
/// rustc moves a link line into a response file once it is too long to spawn,
/// which on Windows is any real link, and the flags filtered above are as
/// likely to be in there as on the command line. Nested files are expanded
/// too; an `@` naming no readable file stays a literal argument, as in gcc.
fn expand_response_files(args: Vec<String>) -> Vec<String> {
   let mut out = Vec::with_capacity(args.len());
   for a in args {
      let text = a.strip_prefix('@').and_then(|p| fs::read_to_string(p).ok());
      match text {
         Some(text) => out.extend(expand_response_files(split_response_file(&text))),
         None => out.push(a),
      }
   }
   out
}

/// libiberty's buildargv: whitespace separates, quotes group, and a backslash
/// escapes the next character everywhere, inside quotes included.
fn split_response_file(text: &str) -> Vec<String> {
   let mut out = Vec::new();
   let mut cur = String::new();
   let mut started = false;
   let mut quote = None;
   let mut chars = text.chars();
   while let Some(c) = chars.next() {
      if c == '\\' {
         if let Some(n) = chars.next() {
            cur.push(n);
         }
         started = true;
      } else if let Some(q) = quote {
         if c == q {
            quote = None;
         } else {
            cur.push(c);
         }
      } else if c == '\'' || c == '"' {
         quote = Some(c);
         started = true;
      } else if c.is_whitespace() {
         if started {
            out.push(std::mem::take(&mut cur));
            started = false;
         }
      } else {
         cur.push(c);
         started = true;
      }
   }
   if started {
      out.push(cur);
   }
   out
}

/// Run a cosmocc tool to completion and return its exit code.
///
/// Windows runs an APE as the PE it also is. A Unix kernel cannot exec one --
/// no ELF magic up front and no shebang -- but a shell can, because the header
/// is also valid shell; once cosmo-build has assimilated the toolchain into
/// native ELF files, they exec directly.
fn run(prog: &Path, args: &[String]) -> i32 {
   match command(prog).args(args).status() {
      Ok(s) => s.code().unwrap_or(1),
      Err(e) => die(&format!("cosmo-shim: {}: {e}", prog.display())),
   }
}

fn command(prog: &Path) -> Command {
   if cfg!(windows) || is_elf(prog) {
      return Command::new(prog);
   }
   let mut c = Command::new("/bin/sh");
   c.arg(prog);
   c
}

fn is_elf(path: &Path) -> bool {
   let mut magic = [0u8; 4];
   fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == *b"\x7fELF"
}

fn die(msg: &str) -> ! {
   eprintln!("{msg}");
   exit(1)
}
