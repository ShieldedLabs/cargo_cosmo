//! Shims for places where cosmopolitan's libc and Rust's std disagree.
//!
//! Link this into any cosmo target and the fixes apply themselves -- each is a
//! `#[no_mangle]` definition that the linker prefers over the one in
//! libcosmo.a, because an archive member is only pulled in to satisfy a symbol
//! that is still undefined.

use core::ffi::{c_char, c_int};

extern "C" {
   fn strerror(errnum: c_int) -> *const c_char;
   fn strlen(s: *const c_char) -> usize;
   fn memcpy(d: *mut c_char, s: *const c_char, n: usize) -> *mut c_char;
}

/// POSIX XSI `strerror_r`, which is what Rust's std actually calls.
///
/// std links this symbol and requires the XSI contract: return an `int`, zero
/// on success. Cosmopolitan declares it `char *__xpg_strerror_r(int, char *,
/// size_t)` -- the GNU contract -- so std reinterprets the returned pointer as
/// an int, finds the sign bit set often enough to matter, and panics with
/// "strerror_r failure". The effect is that formatting *any* io::Error blows
/// up, which takes out ordinary error paths well before anything exotic.
///
/// Copying out of cosmo's `strerror` and returning 0 restores the contract.
#[no_mangle]
pub unsafe extern "C" fn __xpg_strerror_r(
   errnum: c_int,
   buf: *mut c_char,
   buflen: usize,
) -> c_int {
   if buf.is_null() || buflen == 0 {
      return 22; // EINVAL, per XSI
   }
   let msg = strerror(errnum);
   if msg.is_null() {
      *buf = 0;
      return 0;
   }
   let n = core::cmp::min(strlen(msg), buflen - 1);
   memcpy(buf, msg, n);
   *buf.add(n) = 0;
   0
}
