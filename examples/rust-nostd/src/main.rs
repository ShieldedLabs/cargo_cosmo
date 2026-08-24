//! Smallest thing that proves rustc codegen links against cosmopolitan.a.
#![no_std]
#![no_main]

use core::panic::PanicInfo;

extern "C" {
   fn write(fd: i32, buf: *const u8, n: usize) -> isize;
}

const MSG: &[u8] = b"hello from a no_std Rust APE\n";

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
   // cosmo's crt.o calls main; going straight at write() keeps this test about
   // linkage and nothing else.
   unsafe { write(1, MSG.as_ptr(), MSG.len()) };
   0
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
   loop {}
}
