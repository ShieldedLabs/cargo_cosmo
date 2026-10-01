//! Windows APE has no AF_UNIX socketpair. Tokio only needs a byte-stream wakeup pair;
//! build one on loopback TCP while keeping Linux ABI flags and descriptor ownership.

use core::ffi::{c_int, c_void};

const AF_UNIX: c_int = 1;
const AF_INET: c_int = 2;
const SOCK_STREAM: c_int = 1;
const SOCK_NONBLOCK: c_int = 2048;
const SOCK_CLOEXEC: c_int = 524288;
const F_GETFD: c_int = 1;
const F_SETFD: c_int = 2;
const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;
const FD_CLOEXEC: c_int = 1;
const O_NONBLOCK: c_int = 2048;

// These go through cosmo-compat's existing --wrap shims, so both inputs and errno are
// Linux-numbered. In the standalone Linux tests they bind directly to libc.
unsafe extern "C" {
    fn __errno_location() -> *mut c_int;
    fn socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, address: *const SockAddr, length: u32) -> c_int;
    fn listen(fd: c_int, backlog: c_int) -> c_int;
    fn getsockname(fd: c_int, address: *mut SockAddr, length: *mut u32) -> c_int;
    fn connect(fd: c_int, address: *const SockAddr, length: u32) -> c_int;
    fn accept4(fd: c_int, address: *mut SockAddr, length: *mut u32, flags: c_int) -> c_int;
    fn fcntl(fd: c_int, command: c_int, ...) -> c_int;
    fn setsockopt(fd: c_int, level: c_int, name: c_int, value: *const c_void, length: u32) -> c_int;
    fn close(fd: c_int) -> c_int;
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SockAddr {
    family: u16,
    port: u16,
    address: [u8; 4],
    padding: [u8; 8],
}

struct Socket(c_int);

impl Socket {
    fn take(mut self) -> c_int {
        let fd = self.0;
        self.0 = -1;
        fd
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        if self.0 >= 0 {
            unsafe {
                // Cleanup must not overwrite the failing operation's errno.
                let error = *__errno_location();
                close(self.0);
                *__errno_location() = error;
            }
        }
    }
}

pub(crate) fn supported(domain: c_int, ty: c_int, protocol: c_int) -> bool {
    domain == AF_UNIX && ty & !(SOCK_NONBLOCK | SOCK_CLOEXEC) == SOCK_STREAM && protocol == 0
}

/// `sv` must point to two writable ints, exactly as for libc socketpair.
/// Does not publish descriptors unless every setup operation succeeded.
pub(crate) unsafe fn tcp_socketpair(ty: c_int, sv: *mut c_int) -> c_int {
    if sv.is_null() {
        unsafe { *__errno_location() = 14; } // EFAULT in the Linux ABI.
        return -1;
    }
    if !supported(AF_UNIX, ty, 0) {
        unsafe { *__errno_location() = 22; } // EINVAL.
        return -1;
    }
    match unsafe { make_pair(ty) } {
        Ok((client, peer)) => {
            unsafe {
                *sv = client.take();
                *sv.add(1) = peer.take();
            }
            0
        }
        Err(()) => -1,
    }
}

// All fallible calls pass through this adapter so tests can force every failure boundary.
#[inline]
fn syscall(call: impl FnOnce() -> c_int) -> c_int {
    #[cfg(test)]
    if tests::inject_failure() {
        unsafe { *__errno_location() = 5; } // EIO, preserved by Socket::drop.
        return -1;
    }
    call()
}

fn check(result: c_int) -> Result<c_int, ()> {
    if result < 0 { Err(()) } else { Ok(result) }
}

unsafe fn make_pair(ty: c_int) -> Result<(Socket, Socket), ()> {
    unsafe {
        // Keep setup sockets blocking and non-inheritable until connection establishment.
        let listener = Socket(check(syscall(|| socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0)))?);
        let mut address = SockAddr { family: AF_INET as u16, address: [127, 0, 0, 1], ..Default::default() };
        let size = core::mem::size_of::<SockAddr>() as u32;
        check(syscall(|| bind(listener.0, &address, size)))?;
        check(syscall(|| listen(listener.0, 1)))?;
        let mut length = size;
        check(syscall(|| getsockname(listener.0, &mut address, &mut length)))?;
        let client = Socket(check(syscall(|| socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0)))?);
        check(syscall(|| connect(client.0, &address, size)))?;
        let mut client_address = SockAddr::default();
        length = size;
        check(syscall(|| getsockname(client.0, &mut client_address, &mut length)))?;
        let mut peer_address = SockAddr::default();
        length = size;
        let peer = Socket(check(syscall(|| accept4(listener.0, &mut peer_address, &mut length, SOCK_CLOEXEC)))?);
        // Never accept an unrelated local connection that raced the private client.
        if peer_address.family != client_address.family || peer_address.port != client_address.port
            || peer_address.address != client_address.address {
            *__errno_location() = 5;
            return Err(());
        }
        drop(listener);
        for fd in [client.0, peer.0] {
            let enabled: c_int = 1;
            // Tiny wakeup writes must not wait for Nagle's algorithm.
            check(syscall(|| setsockopt(fd, 6, 1, &enabled as *const _ as *const c_void, 4)))?;
            if ty & SOCK_NONBLOCK != 0 {
                let flags = check(syscall(|| fcntl(fd, F_GETFL)))?;
                check(syscall(|| fcntl(fd, F_SETFL, flags | O_NONBLOCK)))?;
            }
            if ty & SOCK_CLOEXEC == 0 {
                let flags = check(syscall(|| fcntl(fd, F_GETFD)))?;
                check(syscall(|| fcntl(fd, F_SETFD, flags & !FD_CLOEXEC)))?;
            }
        }
        Ok((client, peer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, fs, io::{Read, Write}, os::fd::FromRawFd, os::unix::net::UnixStream};

    std::thread_local! {
        static FAIL_AT: Cell<usize> = const { Cell::new(0) };
        static CALLS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn inject_failure() -> bool {
        CALLS.with(|calls| {
            let index = calls.get() + 1;
            calls.set(index);
            FAIL_AT.with(|fail| fail.get() == index)
        })
    }

    fn descriptors() -> usize {
        fs::read_dir("/proc/self/fd").unwrap().count()
    }

    #[test]
    fn forced_tcp_fallback_flags_io_and_failure_cleanup() {
        for flags in [0, SOCK_NONBLOCK, SOCK_CLOEXEC, SOCK_NONBLOCK | SOCK_CLOEXEC] {
            let mut pair = [-1; 2];
            CALLS.with(|calls| calls.set(0));
            assert_eq!(unsafe { tcp_socketpair(SOCK_STREAM | flags, pair.as_mut_ptr()) }, 0);
            let calls = CALLS.with(Cell::get);
            for fd in pair {
                let descriptor_flags = unsafe { fcntl(fd, F_GETFD) };
                let status_flags = unsafe { fcntl(fd, F_GETFL) };
                assert_eq!(descriptor_flags & FD_CLOEXEC != 0, flags & SOCK_CLOEXEC != 0);
                assert_eq!(status_flags & O_NONBLOCK != 0, flags & SOCK_NONBLOCK != 0);
            }
            // Use the same UnixStream wrapper Tokio uses, despite these being TCP descriptors.
            let mut first = unsafe { UnixStream::from_raw_fd(pair[0]) };
            let mut second = unsafe { UnixStream::from_raw_fd(pair[1]) };
            first.set_nonblocking(false).unwrap();
            second.set_nonblocking(false).unwrap();
            first.write_all(b"a").unwrap();
            let mut buffer = [0; 1];
            second.read_exact(&mut buffer).unwrap();
            assert_eq!(&buffer, b"a");
            second.write_all(b"b").unwrap();
            first.read_exact(&mut buffer).unwrap();
            assert_eq!(&buffer, b"b");
            drop(first);
            second.read_exact(&mut buffer).unwrap_err(); // EOF on the other endpoint's close.
            drop(second);

            for failure in 1..=calls {
                let before = descriptors();
                CALLS.with(|calls| calls.set(0));
                FAIL_AT.with(|fail| fail.set(failure));
                let mut untouched = [-123, -456];
                assert_eq!(unsafe { tcp_socketpair(SOCK_STREAM | flags, untouched.as_mut_ptr()) }, -1);
                assert_eq!(unsafe { *__errno_location() }, 5);
                assert_eq!(untouched, [-123, -456]);
                FAIL_AT.with(|fail| fail.set(0));
                assert_eq!(descriptors(), before, "descriptor leak at call {failure}");
            }
        }
        assert_eq!(unsafe { tcp_socketpair(SOCK_STREAM, core::ptr::null_mut()) }, -1);
        assert_eq!(unsafe { *__errno_location() }, 14);
        assert!(!supported(AF_UNIX, 2, 0));
        assert!(!supported(AF_UNIX, SOCK_STREAM, 1));
        assert!(!supported(AF_INET, SOCK_STREAM, 0));
    }
}
