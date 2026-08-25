//! Proof of the stage-2 primitive: read cosmopolitan's *runtime* constants
//! from Rust and compare them against the values rustc baked in at compile
//! time.
//!
//! Cosmo puts every system constant in BSS and fills it in once the host OS is
//! known, so each one is an ordinary linkable symbol. That is what makes a
//! translation shim possible at all -- Rust can see the host's real value and
//! map it to the canonical one the compiled code expects.
//!
//! On Linux every pair below matches, because cosmo's Linux column is exactly
//! what rustc bakes in. Any mismatch here would be a bug in the target spec.
//! The same binary on macOS or Windows is where the columns come apart, and
//! data/syscon-*.json records by how much.

// cosmo-ld wraps 123 libc symbols on every cosmo link; the translators live
// here, and without them the link fails on undefined __wrap_*.
extern crate cosmo_compat as _;
macro_rules! probe {
   ($($name:ident : $baked:expr),* $(,)?) => {
      extern "C" { $(static $name: core::ffi::c_int;)* }
      fn main() {
         println!("{:<18}{:>10}{:>10}   {}", "constant", "cosmo", "rustc", "");
         let mut mismatch = 0;
         $(
            // SAFETY: cosmo defines each of these as `extern const int`,
            // initialized by its runtime before main() is entered.
            let runtime = unsafe { $name };
            let baked: i32 = $baked;
            let ok = runtime == baked;
            if !ok { mismatch += 1; }
            println!("{:<18}{:>10}{:>10}   {}",
                     stringify!($name), runtime, baked,
                     if ok { "ok" } else { "MISMATCH" });
         )*
         println!("\n{} mismatch(es)", mismatch);
         std::process::exit(if mismatch == 0 { 0 } else { 1 });
      }
   };
}

// Right-hand values are Linux's, which is what rustc compiles in for this
// target. Deliberately includes constants that are known to differ on other
// platforms (ENOTSUP, O_NONBLOCK, MAP_ANONYMOUS, SIGCHLD).
probe! {
   EINVAL: 22,
   ENOENT: 2,
   EAGAIN: 11,
   ENOTSUP: 95,
   O_CREAT: 64,
   O_NONBLOCK: 2048,
   MAP_ANONYMOUS: 32,
   SIGCHLD: 17,
   SOCK_STREAM: 1,
   AF_INET: 2,
}
