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

   probe_unwind();
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
