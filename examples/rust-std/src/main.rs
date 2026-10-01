//! Exercises the parts of std that actually have to reach the OS: stdio,
//! filesystem, threads, and time.
use std::collections::HashMap;

// Without this shim, formatting any io::Error panics: cosmo's
// __xpg_strerror_r returns char* where std's XSI contract requires int.
extern crate cosmo_compat as _;

fn main() {
   println!("hello from a std Rust APE");
   println!("arch  = {}", std::env::consts::ARCH);
   println!("os    = {}", std::env::consts::OS);

   let args: Vec<String> = std::env::args().collect();
   println!("argv0 = {}", args.first().map(String::as_str).unwrap_or("?"));

   let mut m = HashMap::new();
   m.insert("alloc", "works");
   println!("heap  = {}", m["alloc"]);

   let h = std::thread::spawn(|| (1..=10).sum::<u32>());
   println!("thread sum 1..10 = {}", h.join().unwrap());

   let t = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_secs())
      .unwrap_or(0);
   println!("unix time = {}", t);

   let p = std::env::temp_dir().join("cosmo-rust-probe.txt");
   std::fs::write(&p, b"file io works\n").expect("write");
   print!("file  = {}", std::fs::read_to_string(&p).expect("read"));
   std::fs::remove_file(&p).ok();

   // Formatting a real io::Error goes through strerror_r.
   let e = std::fs::read_to_string("/definitely/not/here").unwrap_err();
   println!("ioerr = {} / {:?}", e, e.kind());

   probe_pipe2_and_accept();

   probe_unwind();
}

// Appended probe: pipe2 and accept4 carry O_*/SOCK_* flags across the libc
// boundary, and std numbers them for Linux. Untranslated, cosmopolitan's pipe2
// rejects them with EINVAL -- mio's wakeup pipe, and with it every tokio runtime,
// died at startup on macOS -- and the server side of a socket never accepts.
#[allow(dead_code)]
fn probe_pipe2_and_accept() {
   extern "C" {
      fn pipe2(fds: *mut i32, flags: i32) -> i32;
      fn close(fd: i32) -> i32;
   }
   // Linux's O_CLOEXEC|O_NONBLOCK: exactly the flags mio's wakeup pipe asks for.
   const LINUX_O_CLOEXEC: i32 = 0o2000000;
   const LINUX_O_NONBLOCK: i32 = 0o4000;
   let mut fds = [-1i32; 2];
   let r = unsafe { pipe2(fds.as_mut_ptr(), LINUX_O_CLOEXEC | LINUX_O_NONBLOCK) };
   if r == 0 {
      unsafe {
         close(fds[0]);
         close(fds[1]);
      }
   }
   println!("pipe2 = {}", if r == 0 { "works" } else { "FAILED" });

   // std's accept() calls accept4 with SOCK_CLOEXEC|SOCK_NONBLOCK on Linux targets.
   let r = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| {
      let addr = l.local_addr()?;
      let c = std::net::TcpStream::connect(addr)?;
      let (s, _) = l.accept()?;
      drop((c, s));
      Ok(())
   });
   match r {
      Ok(()) => println!("accept = works"),
      Err(e) => println!("accept = FAILED ({e})"),
   }
}

// Appended probe: unwinding across a panic boundary. This is the capability
// the older prior art said cosmo could not support.
#[allow(dead_code)]
fn probe_unwind() {
   let r = std::panic::catch_unwind(|| {
      panic!("deliberate panic for the unwind probe");
   });
   println!("catch_unwind caught = {}", r.is_err());
}
