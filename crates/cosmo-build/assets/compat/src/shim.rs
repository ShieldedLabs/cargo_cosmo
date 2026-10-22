//! The libc-boundary wrappers. cosmo-ld links every program with
//! `--wrap=NAME` for each name in ../wrap.txt, so a call to `open` from std
//! (or from anything else in the link) lands in `__wrap_open` here, which
//! translates the Linux-numbered arguments to the host's numbering, calls
//! `__cosmo_real_open` (cosmo's), and translates errno / results back.
//!
//! Only the functions whose ARGUMENTS carry constants are written here; the
//! rest are generated register-passthrough wrappers in gen.rs that only fix
//! errno. Anything not in wrap.txt reaches cosmo directly.
//!
//! `__cosmo_real_NAME` rather than `--wrap`'s own `__real_NAME`: cosmo-ld links
//! a copy of libcosmo with NAME renamed to that throughout, so cosmopolitan's
//! calls to its own public functions (realpath -> readlink, open -> openat)
//! stay inside cosmo instead of landing here and being translated as if they
//! came from std. `__real_NAME` would not survive the rename: `--wrap` resolves
//! it to NAME, which the renamed archive no longer defines.
//!
//! C code compiled by cosmocc and linked alongside (a `cc`-built static library)
//! still reaches these wrappers, although it passes host-numbered constants.

#![allow(clippy::missing_safety_doc)]

use core::ffi::{c_char, c_int, c_uint, c_void};
use crate::gen;
use crate::xlate;

unsafe extern "C" {
    fn __errno_location() -> *mut c_int;
    fn __cosmo_real_open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn __cosmo_real_openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    fn __cosmo_real_stat(path: *const c_char, buf: *mut c_void) -> c_int;
    fn __cosmo_real_fstat(fd: c_int, buf: *mut c_void) -> c_int;
    fn __cosmo_real_lstat(path: *const c_char, buf: *mut c_void) -> c_int;
    fn __cosmo_real_fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn __cosmo_real_ioctl(fd: c_int, req: u64, ...) -> c_int;
    fn __cosmo_real_socket(domain: c_int, ty: c_int, proto: c_int) -> c_int;
    fn __cosmo_real_socketpair(domain: c_int, ty: c_int, proto: c_int, sv: *mut c_int) -> c_int;
    fn __cosmo_real_accept4(fd: c_int, addr: *mut c_void, alen: *mut u32, flags: c_int) -> c_int;
    fn __cosmo_real_pipe2(fds: *mut c_int, flags: c_int) -> c_int;
    fn __cosmo_real_setsockopt(fd: c_int, level: c_int, name: c_int, val: *const c_void, len: u32) -> c_int;
    fn __cosmo_real_getsockopt(fd: c_int, level: c_int, name: c_int, val: *mut c_void, len: *mut u32) -> c_int;
    fn __cosmo_real_send(fd: c_int, buf: *const c_void, n: usize, flags: c_int) -> isize;
    fn __cosmo_real_sendto(fd: c_int, buf: *const c_void, n: usize, flags: c_int, addr: *const c_void, alen: u32) -> isize;
    fn __cosmo_real_bind(fd: c_int, addr: *const c_void, alen: u32) -> c_int;
    fn __cosmo_real_connect(fd: c_int, addr: *const c_void, alen: u32) -> c_int;
    fn __cosmo_real_getsockname(fd: c_int, addr: *mut c_void, alen: *mut u32) -> c_int;
    fn __cosmo_real_getpeername(fd: c_int, addr: *mut c_void, alen: *mut u32) -> c_int;
    fn __cosmo_real_recv(fd: c_int, buf: *mut c_void, n: usize, flags: c_int) -> isize;
    fn __cosmo_real_recvfrom(fd: c_int, buf: *mut c_void, n: usize, flags: c_int, addr: *mut c_void, alen: *mut u32) -> isize;
    fn __cosmo_real_sendmsg(fd: c_int, msg: *const MsgHdr, flags: c_int) -> isize;
    fn __cosmo_real_recvmsg(fd: c_int, msg: *mut MsgHdr, flags: c_int) -> isize;
    fn __cosmo_real_pthread_setschedparam(thread: usize, policy: c_int, param: *const c_void) -> c_int;
    fn __cosmo_real_poll(fds: *mut PollFd, n: u64, timeout: c_int) -> c_int;
    fn __cosmo_real_mmap(addr: *mut c_void, len: u64, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void;
    fn __cosmo_real_sigaction(sig: c_int, act: *const CosmoSigAction, old: *mut CosmoSigAction) -> c_int;
    fn __cosmo_real_signal(sig: c_int, handler: usize) -> usize;
    fn __cosmo_real_kill(pid: c_int, sig: c_int) -> c_int;
    fn __cosmo_real_killpg(pgrp: c_int, sig: c_int) -> c_int;
    fn __cosmo_real_sigaddset(set: *mut c_void, sig: c_int) -> c_int;
    fn __cosmo_real_waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    fn __cosmo_real_unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int;
    fn __cosmo_real_linkat(olddir: c_int, old: *const c_char, newdir: c_int, new: *const c_char, flags: c_int) -> c_int;
    fn __cosmo_real_renameat(olddir: c_int, old: *const c_char, newdir: c_int, new: *const c_char) -> c_int;
    fn __cosmo_real_fchmodat(dirfd: c_int, path: *const c_char, mode: c_uint, flags: c_int) -> c_int;
    fn __cosmo_real_utimensat(dirfd: c_int, path: *const c_char, times: *const c_void, flags: c_int) -> c_int;
    fn __cosmo_real_getaddrinfo(node: *const c_char, service: *const c_char, hints: *const AddrInfo, res: *mut *mut AddrInfo) -> c_int;
    fn __cosmo_real_freeaddrinfo(ai: *mut AddrInfo);
}

#[repr(C)]
pub struct PollFd { fd: i32, events: i16, revents: i16 }
#[repr(C)]
pub struct MsgHdr { name: *mut c_void, namelen: u32, iov: *mut c_void, iovlen: u64, control: *mut c_void, controllen: u64, flags: u32 }
/// Cosmopolitan's `struct sigaction`, 32 bytes: {sa_handler, sa_flags,
/// sa_restorer, sa_mask} with an 8-byte sigset_t -- checked against
/// `sizeof(struct sigaction)` and the offsets with cosmocc. The caller's
/// `libc::sigaction` is musl's, 152 bytes with the 128-byte sa_mask *second*,
/// so reading sa_flags at offset 8 (as this shim did) read inside sa_mask and
/// installed every handler with flags 0: SA_RESTART, and with it
/// SA_SIGINFO|SA_ONSTACK for std's stack-overflow handler, was silently lost.
#[repr(C)]
pub struct CosmoSigAction { handler: usize, flags: c_int, pad: c_int, restorer: usize, mask: u64 }
/// Linux layout, which cosmo shares.
#[repr(C)]
pub struct AddrInfo { flags: c_int, family: c_int, socktype: c_int, protocol: c_int, addrlen: u32, addr: *mut u16, canonname: *mut c_char, next: *mut AddrInfo }

/// Rewrite errno in place from the host's numbering to Linux's. Called by every
/// wrapper on failure, exactly once per failed call, which is what keeps the
/// translation from being applied twice.
#[inline]
pub fn fix_errno() {
    unsafe {
        let p = __errno_location();
        *p = xlate::errno_to_linux(*p as i64) as c_int;
    }
}
#[inline] fn ret(r: c_int) -> c_int { if r == -1 { fix_errno(); } r }
#[inline] fn rets(r: isize) -> isize { if r == -1 { fix_errno(); } r }

#[inline] fn open_flags(f: c_int) -> c_int { gen::open().to_host(f as i64) as c_int }
#[inline] fn at_fd(fd: c_int) -> c_int { gen::at().to_host(fd as i64) as c_int }
#[inline] fn at_flags(f: c_int) -> c_int {
    // AT_* flags are an enum group in the table but combine as bits; translate each set bit.
    let g = gen::at(); let mut out = 0i64; let mut rest = f as i64;
    for (i, &l) in g.linux.iter().enumerate() { if l > 0 && rest & l == l { out |= g.host[i]; rest &= !l; } }
    (out | rest) as c_int
}
#[inline] fn msg_flags(f: c_int) -> c_int { gen::msg().to_host(f as i64) as c_int }
#[inline] fn sig(s: c_int) -> c_int { gen::sig().to_host(s as i64) as c_int }
#[inline] fn sig_back(s: c_int) -> c_int { gen::sig().to_linux(s as i64) as c_int }

/// sockaddr.sa_family is the first u16 of every sockaddr; AF_INET6 differs per host.
unsafe fn family_to_host(addr: *mut u16) { if !addr.is_null() { unsafe { core::ptr::write_unaligned(addr, gen::af().to_host(core::ptr::read_unaligned(addr) as i64) as u16); } } }
unsafe fn family_to_linux(addr: *mut u16) { if !addr.is_null() { unsafe { core::ptr::write_unaligned(addr, gen::af().to_linux(core::ptr::read_unaligned(addr) as i64) as u16); } } }
/// Input sockaddrs are const: copy into a stack buffer with the family rewritten.
unsafe fn with_host_addr<R>(addr: *const c_void, alen: u32, f: impl FnOnce(*const c_void) -> R) -> R {
    if addr.is_null() || alen < 2 || alen as usize > 128 { return f(addr); }
    let mut buf = [0u8; 128];
    unsafe {
        core::ptr::copy_nonoverlapping(addr as *const u8, buf.as_mut_ptr(), alen as usize);
        family_to_host(buf.as_mut_ptr() as *mut u16);
    }
    f(buf.as_ptr() as *const c_void)
}

// ---- files ----------------------------------------------------------------------
/// Goes to `__cosmo_real_openat` because cosmo's own `open()` only forwards
/// there. This was once load-bearing -- before cosmo-ld renamed libcosmo's
/// internal references, that forwarding call landed back in `__wrap_openat`
/// and XNU got its flags translated twice (docs/DESIGN.md) -- and is now just
/// the shorter path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_open(path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    const AT_FDCWD: c_int = -100;   // canonical (Linux); at_fd() gives the host's
    ret(unsafe { __cosmo_real_openat(at_fd(AT_FDCWD), path, open_flags(flags), mode) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_openat(dirfd: c_int, path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    ret(unsafe { __cosmo_real_openat(at_fd(dirfd), path, open_flags(flags), mode) })
}

/// Cosmopolitan's `struct stat`: 144 bytes, the x86-64 Linux shape on both
/// arches (checked with cosmocc: sizeof=144, st_mode@24, st_size@48). The
/// caller's `libc::stat` is musl's and differs per arch -- 128 bytes with
/// st_mode@16 on aarch64 -- so letting cosmo fill the caller's buffer overran
/// it by 16 bytes and corrupted the caller's stack: the `stat` inside
/// `create_dir_all` faulted the next statement in its caller.
#[repr(C)]
#[derive(Default)]
pub struct CosmoStat {
    dev: u64, ino: u64, nlink: u32, pad0: u32, mode: u32, uid: u32, gid: u32, pad1: u32,
    rdev: u64, size: i64, blksize: i32, pad2: i32, blocks: i64,
    atim: [i64; 2], mtim: [i64; 2], ctim: [i64; 2], reserved: [u64; 3],
}

/// Field by field, so the two layouts' differing offsets fall out of the
/// compiler instead of being duplicated here as constants.
pub fn stat_to_caller(dst: &mut libc::stat, src: &CosmoStat) {
    dst.st_dev = src.dev as _;
    dst.st_ino = src.ino as _;
    dst.st_mode = src.mode as _;
    dst.st_nlink = src.nlink as _;
    dst.st_uid = src.uid as _;
    dst.st_gid = src.gid as _;
    dst.st_rdev = src.rdev as _;
    dst.st_size = src.size as _;
    dst.st_blksize = src.blksize as _;
    dst.st_blocks = src.blocks as _;
    dst.st_atime = src.atim[0] as _;
    dst.st_atime_nsec = src.atim[1] as _;
    dst.st_mtime = src.mtim[0] as _;
    dst.st_mtime_nsec = src.mtim[1] as _;
    dst.st_ctime = src.ctim[0] as _;
    dst.st_ctime_nsec = src.ctim[1] as _;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_stat(path: *const c_char, buf: *mut libc::stat) -> c_int {
    let mut c = CosmoStat::default();
    let r = unsafe { __cosmo_real_stat(path, &mut c as *mut CosmoStat as *mut c_void) };
    if r == -1 { fix_errno(); return r; }
    if !buf.is_null() { stat_to_caller(unsafe { &mut *buf }, &c); }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_fstat(fd: c_int, buf: *mut libc::stat) -> c_int {
    let mut c = CosmoStat::default();
    let r = unsafe { __cosmo_real_fstat(fd, &mut c as *mut CosmoStat as *mut c_void) };
    if r == -1 { fix_errno(); return r; }
    if !buf.is_null() { stat_to_caller(unsafe { &mut *buf }, &c); }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_lstat(path: *const c_char, buf: *mut libc::stat) -> c_int {
    let mut c = CosmoStat::default();
    let r = unsafe { __cosmo_real_lstat(path, &mut c as *mut CosmoStat as *mut c_void) };
    if r == -1 { fix_errno(); return r; }
    if !buf.is_null() { stat_to_caller(unsafe { &mut *buf }, &c); }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_fcntl(fd: c_int, cmd: c_int, arg: usize) -> c_int {
    // Commands: fcntl/fcntl2/fcntl3 groups. F_SETFL's argument is O_* bits; F_GETFL's result too.
    const F_GETFL: c_int = 3; const F_SETFL: c_int = 4;
    let hcmd = {
        let mut c = gen::fcntl().to_host(cmd as i64);
        if c == cmd as i64 { c = gen::fcntl2().to_host(cmd as i64); }
        if c == cmd as i64 { c = gen::fcntl3().to_host(cmd as i64); }
        c as c_int
    };
    let harg = if cmd == F_SETFL { open_flags(arg as c_int) as usize } else { arg };
    let r = unsafe { __cosmo_real_fcntl(fd, hcmd, harg) };
    if r == -1 { fix_errno(); return r; }
    if cmd == F_GETFL { return gen::open().to_linux(r as i64) as c_int; }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_ioctl(fd: c_int, req: u64, arg: usize) -> c_int {
    ret(unsafe { __cosmo_real_ioctl(fd, gen::ioctl().to_host(req as i64) as u64, arg) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int {
    ret(unsafe { __cosmo_real_unlinkat(at_fd(dirfd), path, at_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_linkat(od: c_int, old: *const c_char, nd: c_int, new: *const c_char, flags: c_int) -> c_int {
    ret(unsafe { __cosmo_real_linkat(at_fd(od), old, at_fd(nd), new, at_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_renameat(od: c_int, old: *const c_char, nd: c_int, new: *const c_char) -> c_int {
    ret(unsafe { __cosmo_real_renameat(at_fd(od), old, at_fd(nd), new) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_fchmodat(dirfd: c_int, path: *const c_char, mode: c_uint, flags: c_int) -> c_int {
    ret(unsafe { __cosmo_real_fchmodat(at_fd(dirfd), path, mode, at_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_utimensat(dirfd: c_int, path: *const c_char, times: *const c_void, flags: c_int) -> c_int {
    ret(unsafe { __cosmo_real_utimensat(at_fd(dirfd), path, times, at_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_mmap(addr: *mut c_void, len: u64, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void {
    let r = unsafe { __cosmo_real_mmap(addr, len, gen::mprot().to_host(prot as i64) as c_int, gen::mmap().to_host(flags as i64) as c_int, fd, off) };
    if r as isize == -1 { fix_errno(); }
    r
}

// ---- pipes -----------------------------------------------------------------------
/// pipe2's flags are O_* bits, so open_flags is the whole translation. Generated as
/// an errno-only passthrough it handed cosmopolitan Linux's `O_CLOEXEC|O_NONBLOCK`
/// (0x80800), which its pipe2 rejects with EINVAL -- mio's wakeup pipe, and with it
/// every tokio runtime, died at startup on a Mac.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_pipe2(fds: *mut c_int, flags: c_int) -> c_int {
    ret(unsafe { __cosmo_real_pipe2(fds, open_flags(flags)) })
}

// ---- sockets ---------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_socket(domain: c_int, ty: c_int, proto: c_int) -> c_int {
    ret(unsafe { __cosmo_real_socket(gen::af().to_host(domain as i64) as c_int, gen::sock().to_host(ty as i64) as c_int, proto) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_socketpair(domain: c_int, ty: c_int, proto: c_int, sv: *mut c_int) -> c_int {
    // Cosmopolitan's socketpair() is a stub on Windows (ENOSYS). The
    // self-pipe / wakeup-fd pattern (mio, tokio, polling) needs it, so on a
    // Windows host fall back to a connected localhost TCP pair, which works
    // for stream sockets there. Everywhere else, and for datagram pairs,
    // cosmo's own call stands.
    unsafe extern "C" { static __hostos: c_int; }
    if unsafe { __hostos } == 4 && crate::socketpair::supported(domain, ty, proto) {
        return unsafe { crate::socketpair::tcp_socketpair(ty, sv) };
    }
    ret(unsafe { __cosmo_real_socketpair(gen::af().to_host(domain as i64) as c_int, gen::sock().to_host(ty as i64) as c_int, proto, sv) })
}
/// accept4's flags are SOCK_* bits (`SOCK_CLOEXEC|SOCK_NONBLOCK` from std's accept),
/// translated like socket()'s type. Untranslated they are an unknown-bit EINVAL on
/// XNU, and the server side of every socket would refuse to accept.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_accept4(fd: c_int, addr: *mut c_void, alen: *mut u32, flags: c_int) -> c_int {
    let r = ret(unsafe { __cosmo_real_accept4(fd, addr, alen, gen::sock().to_host(flags as i64) as c_int) });
    if r >= 0 { unsafe { family_to_linux(addr as *mut u16); } }
    r
}
// Linux errnos the wrappers set themselves.
const EINVAL_LINUX: c_int = 22;
const ENOPROTOOPT_LINUX: c_int = 92;
const EFAULT_LINUX: c_int = 14;
#[inline] fn fail(e: c_int) -> c_int { unsafe { *__errno_location() = e; } -1 }

/// Socket options: see net.rs. Absent or unknown on this host -> ENOPROTOOPT,
/// never passed through raw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_setsockopt(fd: c_int, level: c_int, name: c_int, val: *const c_void, len: u32) -> c_int {
    let Some(o) = crate::net::sockopt(level, name) else { return fail(ENOPROTOOPT_LINUX) };
    if o.val == crate::net::Val::Same {
        return ret(unsafe { __cosmo_real_setsockopt(fd, o.level, o.name, val, len) });
    }
    if val.is_null() || (len as usize) < 4 { return fail(EINVAL_LINUX); }
    let Some(hv) = crate::net::val_to_host(o.val, unsafe { core::ptr::read_unaligned(val as *const c_int) }) else { return fail(EINVAL_LINUX) };
    ret(unsafe { __cosmo_real_setsockopt(fd, o.level, o.name, &hv as *const c_int as *const c_void, 4) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_getsockopt(fd: c_int, level: c_int, name: c_int, val: *mut c_void, len: *mut u32) -> c_int {
    const SO_ERROR_LINUX: c_int = 4; const SO_TYPE_LINUX: c_int = 3;
    let Some(o) = crate::net::sockopt(level, name) else { return fail(ENOPROTOOPT_LINUX) };
    let r = unsafe { __cosmo_real_getsockopt(fd, o.level, o.name, val, len) };
    if r == -1 { fix_errno(); return r; }
    // Results that are themselves constants.
    if !val.is_null() && !len.is_null() && unsafe { *len } >= 4 {
        let p = val as *mut c_int;
        let mut v = unsafe { core::ptr::read_unaligned(p) };
        if o.val != crate::net::Val::Same { { v = crate::net::val_to_linux(o.val, v); } }
        if level == crate::net::SOL_SOCKET {
            if name == SO_ERROR_LINUX { { v = xlate::errno_to_linux(v as i64) as c_int; } }
            if name == SO_TYPE_LINUX { { v = gen::sock().to_linux(v as i64) as c_int; } }
        }
        unsafe { core::ptr::write_unaligned(p, v); }
    }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_send(fd: c_int, buf: *const c_void, n: usize, flags: c_int) -> isize {
    rets(unsafe { __cosmo_real_send(fd, buf, n, msg_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_rename(old: *const c_char, new: *const c_char) -> c_int {
    // Not __cosmo_real_rename: that fails in cosmo's userspace without ever
    // reaching the kernel (reproduced in isolation). renameat is the call it
    // would have made, and it works.
    ret(unsafe { __cosmo_real_renameat(at_fd(-100), old, at_fd(-100), new) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_bind(fd: c_int, addr: *const c_void, alen: u32) -> c_int {
    unsafe { with_host_addr(addr, alen, |a| ret(__cosmo_real_bind(fd, a, alen))) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_connect(fd: c_int, addr: *const c_void, alen: u32) -> c_int {
    unsafe { with_host_addr(addr, alen, |a| ret(__cosmo_real_connect(fd, a, alen))) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_getsockname(fd: c_int, addr: *mut c_void, alen: *mut u32) -> c_int {
    let r = ret(unsafe { __cosmo_real_getsockname(fd, addr, alen) });
    if r == 0 { unsafe { family_to_linux(addr as *mut u16); } }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_getpeername(fd: c_int, addr: *mut c_void, alen: *mut u32) -> c_int {
    let r = ret(unsafe { __cosmo_real_getpeername(fd, addr, alen) });
    if r == 0 { unsafe { family_to_linux(addr as *mut u16); } }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sendto(fd: c_int, buf: *const c_void, n: usize, flags: c_int, addr: *const c_void, alen: u32) -> isize {
    unsafe { with_host_addr(addr, alen, |a| rets(__cosmo_real_sendto(fd, buf, n, msg_flags(flags), a, alen))) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_recv(fd: c_int, buf: *mut c_void, n: usize, flags: c_int) -> isize {
    rets(unsafe { __cosmo_real_recv(fd, buf, n, msg_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_recvfrom(fd: c_int, buf: *mut c_void, n: usize, flags: c_int, addr: *mut c_void, alen: *mut u32) -> isize {
    let r = rets(unsafe { __cosmo_real_recvfrom(fd, buf, n, msg_flags(flags), addr, alen) });
    if r >= 0 { unsafe { family_to_linux(addr as *mut u16); } }
    r
}
/// Scratch space for converted control data. Linux callers on this target
/// use a few dozen bytes (TCLASS + PKTINFO); larger buffers are converted up to
/// this size and the rest is reported as truncation.
const CTL_MAX: usize = 1024;
#[repr(C, align(8))]
struct Ctl([u8; CTL_MAX]);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sendmsg(fd: c_int, msg: *const MsgHdr, flags: c_int) -> isize {
    if msg.is_null() { return fail(EFAULT_LINUX) as isize; }
    let m = unsafe { &*msg };
    let host = xlate::hostos();
    let mut ctl = Ctl([0; CTL_MAX]);
    let (control, controllen) = if host == xlate::HOST_LINUX {
        (m.control, m.controllen)
    } else if m.controllen == 0 {
        (core::ptr::null_mut(), 0)
    } else if m.control.is_null() {
        return fail(EFAULT_LINUX) as isize;
    } else if host == xlate::HOST_XNU && m.controllen <= CTL_MAX as u64 {
        match unsafe { crate::net::linux_to_xnu(m.control as *const u8, m.controllen as usize, ctl.0.as_mut_ptr(), CTL_MAX) } {
            Some(n) => (ctl.0.as_mut_ptr() as *mut c_void, n as u64),
            None => return fail(EINVAL_LINUX) as isize,
        }
    } else {
        // Windows (cosmopolitan has no ancillary path there) and the untested BSDs.
        return fail(EINVAL_LINUX) as isize;
    };
    // The name pointer inside the header is const to the caller; rewrite through a copy of the header.
    unsafe {
        with_host_addr(m.name, m.namelen, |a| {
            let copy = MsgHdr { name: a as *mut c_void, namelen: m.namelen, iov: m.iov, iovlen: m.iovlen, control, controllen, flags: m.flags };
            rets(__cosmo_real_sendmsg(fd, &copy, msg_flags(flags)))
        })
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_recvmsg(fd: c_int, msg: *mut MsgHdr, flags: c_int) -> isize {
    const MSG_CTRUNC_LINUX: u32 = 8;
    if msg.is_null() { return fail(EFAULT_LINUX) as isize; }
    let host = xlate::hostos();
    let m = unsafe { &mut *msg };
    let namecap = m.namelen;
    if host == xlate::HOST_LINUX {
        let r = rets(unsafe { __cosmo_real_recvmsg(fd, msg, msg_flags(flags)) });
        if r >= 0 {
            if namecap >= 2 && m.namelen >= 2 { unsafe { family_to_linux(m.name as *mut u16); } }
            m.flags = gen::msg().to_linux(m.flags as i64) as u32;
        }
        return r;
    }
    // Only XNU's ancillary layout is converted. Unsupported hosts receive
    // without a control buffer: the kernel discards SCM_RIGHTS without installing
    // descriptors, rather than leaking descriptors from discarded scratch data.
    let mut ctl = Ctl([0; CTL_MAX]);
    let convert_control = host == xlate::HOST_XNU;
    let want_control = !m.control.is_null() && m.controllen != 0;
    let mut copy = MsgHdr {
        name: m.name, namelen: m.namelen, iov: m.iov, iovlen: m.iovlen,
        control: if !convert_control || !want_control { core::ptr::null_mut() } else { ctl.0.as_mut_ptr() as *mut c_void },
        controllen: if !convert_control || !want_control { 0 } else { core::cmp::min(m.controllen as usize, CTL_MAX) as u64 },
        flags: 0,
    };
    let r = rets(unsafe { __cosmo_real_recvmsg(fd, &mut copy, msg_flags(flags)) });
    if r < 0 { return r; }
    if namecap >= 2 && copy.namelen >= 2 { unsafe { family_to_linux(m.name as *mut u16); } }
    m.namelen = copy.namelen;
    let mut lflags = gen::msg().to_linux(copy.flags as i64) as u32;
    let got = if convert_control { core::cmp::min(copy.controllen as usize, CTL_MAX) } else { 0 };
    m.controllen = if host == xlate::HOST_XNU {
        let (n, trunc) = unsafe { crate::net::xnu_to_linux(ctl.0.as_ptr(), got, m.control as *mut u8, m.controllen as usize, |fd| { unsafe extern "C" { fn __cosmo_real_close(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize; } __cosmo_real_close(fd as usize, 0, 0, 0, 0, 0); }) };
        if trunc { lflags |= MSG_CTRUNC_LINUX; }
        n as u64
    } else {
        if got > 0 { lflags |= MSG_CTRUNC_LINUX; }
        0
    };
    m.flags = lflags;
    r
}
/// `pthread_*` functions return an errno code directly rather than -1. The
/// Linux-ABI caller (the thread-priority crate) requests SCHED_OTHER with
/// static priority 0 before lowering the nice value through setpriority().
/// XNU has no SCHED_OTHER constant (its policy numbering differs entirely), so
/// cosmo's pthread_setschedparam rejects the call and the caller keeps the
/// thread at default priority -- which is how the miner's solver ended up
/// competing with the GUI at full scheduler priority. Honour the default-policy
/// request as success and let the following setpriority() do the real work.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_pthread_setschedparam(thread: usize, policy: c_int, param: *const c_void) -> c_int {
    let _ = thread;
    if policy == 0 { // Linux SCHED_OTHER: default scheduling
        let prio = if param.is_null() { 0 } else { unsafe { *(param as *const i32) } };
        return if prio == 0 { 0 } else { 22 /* EINVAL, as Linux for a nonzero static priority */ };
    }
    95 // ENOTSUP: Linux realtime policies have no XNU equivalent on this path
}

// ---- timed waits -----------------------------------------------------------
/// POSIX timed waits take an absolute deadline on CLOCK_REALTIME, but callers
/// that think on a monotonic clock -- the Rust std thread parker among them --
/// hand in a deadline computed from CLOCK_MONOTONIC. Cosmo judges the deadline
/// against real time, so such a value reads as "already past" and the wait
/// returns ETIMEDOUT before ever blocking; every idle scheduler park (rayon's
/// worker pool, tokio's park) then spins and burns a core per parked thread.
/// Rebase any deadline that cannot be a real-time date (before 2020 -- i.e. a
/// monotonic reading) from CLOCK_MONOTONIC onto CLOCK_REALTIME before handing
/// it down. These functions return the error as their return value (sem_* is
/// the exception and keeps -1/errno), so the codes are translated to Linux's
/// numbering like every other wrapper's.
#[repr(C)]
#[derive(Copy, Clone)]
struct Timespec { tv_sec: i64, tv_nsec: i64 }
extern "C" {
    fn __cosmo_real_pthread_cond_timedwait(c: *mut c_void, m: *mut c_void, ts: *const Timespec) -> c_int;
    fn __cosmo_real_pthread_mutex_timedlock(m: *mut c_void, ts: *const Timespec) -> c_int;
    fn __cosmo_real_pthread_rwlock_timedrdlock(rw: *mut c_void, ts: *const Timespec) -> c_int;
    fn __cosmo_real_pthread_rwlock_timedwrlock(rw: *mut c_void, ts: *const Timespec) -> c_int;
    fn __cosmo_real_sem_timedwait(sem: *mut c_void, ts: *const Timespec) -> c_int;
}
unsafe fn rebase_deadline(ts: *const Timespec) -> Timespec {
    let mut out = unsafe { *ts };
    if out.tv_sec < 1_600_000_000 {
        let mut rt = Timespec { tv_sec: 0, tv_nsec: 0 };
        let mut mn = Timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe {
            __wrap_clock_gettime(0, &mut rt as *mut _ as *mut c_void); // CLOCK_REALTIME
            __wrap_clock_gettime(1, &mut mn as *mut _ as *mut c_void); // CLOCK_MONOTONIC
        }
        let delta_ns = (out.tv_sec - mn.tv_sec) as i64 * 1_000_000_000 + (out.tv_nsec - mn.tv_nsec) as i64;
        let now_ns = rt.tv_sec as i64 * 1_000_000_000 + rt.tv_nsec as i64 + if delta_ns > 0 { delta_ns } else { 0 };
        out.tv_sec = now_ns / 1_000_000_000;
        out.tv_nsec = now_ns % 1_000_000_000;
    }
    out
}
#[inline] fn pret(rc: c_int) -> c_int { if rc != 0 { xlate::errno_to_linux(rc as i64) as c_int } else { 0 } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_pthread_cond_timedwait(c: *mut c_void, m: *mut c_void, ts: *const Timespec) -> c_int {
    let fixed = unsafe { rebase_deadline(ts) };
    pret(unsafe { __cosmo_real_pthread_cond_timedwait(c, m, &fixed) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_pthread_mutex_timedlock(m: *mut c_void, ts: *const Timespec) -> c_int {
    let fixed = unsafe { rebase_deadline(ts) };
    pret(unsafe { __cosmo_real_pthread_mutex_timedlock(m, &fixed) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_pthread_rwlock_timedrdlock(rw: *mut c_void, ts: *const Timespec) -> c_int {
    let fixed = unsafe { rebase_deadline(ts) };
    pret(unsafe { __cosmo_real_pthread_rwlock_timedrdlock(rw, &fixed) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_pthread_rwlock_timedwrlock(rw: *mut c_void, ts: *const Timespec) -> c_int {
    let fixed = unsafe { rebase_deadline(ts) };
    pret(unsafe { __cosmo_real_pthread_rwlock_timedwrlock(rw, &fixed) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sem_timedwait(sem: *mut c_void, ts: *const Timespec) -> c_int {
    let fixed = unsafe { rebase_deadline(ts) };
    ret(unsafe { __cosmo_real_sem_timedwait(sem, &fixed) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_getaddrinfo(node: *const c_char, service: *const c_char, hints: *const AddrInfo, res: *mut *mut AddrInfo) -> c_int {
    let mut h_copy;
    let hp = if hints.is_null() { hints } else {
        let h = unsafe { &*hints };
        h_copy = AddrInfo { flags: h.flags, family: gen::af().to_host(h.family as i64) as c_int, socktype: gen::sock().to_host(h.socktype as i64) as c_int, protocol: h.protocol, addrlen: 0, addr: core::ptr::null_mut(), canonname: core::ptr::null_mut(), next: core::ptr::null_mut() };
        &mut h_copy as *const AddrInfo
    };
    let r = unsafe { __cosmo_real_getaddrinfo(node, service, hp, res) };
    if r != 0 { fix_errno(); return r; }   // EAI_SYSTEM reads errno
    // Every result carries the host's family twice: in ai_family and in the sockaddr.
    let mut p = unsafe { *res };
    while !p.is_null() {
        unsafe {
            (*p).family = gen::af().to_linux((*p).family as i64) as c_int;
            (*p).socktype = gen::sock().to_linux((*p).socktype as i64) as c_int;
            family_to_linux((*p).addr);
            p = (*p).next;
        }
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_freeaddrinfo(ai: *mut AddrInfo) {
    // Undo what __wrap_getaddrinfo did before handing the list back to cosmo's allocator walk;
    // it only frees, but keep its view consistent.
    unsafe { __cosmo_real_freeaddrinfo(ai) }
}

// ---- clocks ----------------------------------------------------------------------
// These were generated as errno-only passthroughs, which is wrong: the first
// argument is a clockid_t, and the clock group is one of the widest divergences
// in the table (CLOCK_MONOTONIC is 1 on Linux, 8 on XNU). Untranslated, the
// first `Instant::now()` on a Mac fails with EINVAL and std panics before the
// program has done anything.
unsafe extern "C" {
    fn __cosmo_real_clock_gettime(id: c_int, ts: *mut c_void) -> c_int;
    fn __cosmo_real_clock_nanosleep(id: c_int, flags: c_int, req: *const c_void, rem: *mut c_void) -> c_int;
}

#[inline] fn clockid(id: c_int) -> c_int { gen::clock().to_host(id as i64) as c_int }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_clock_gettime(id: c_int, ts: *mut c_void) -> c_int {
    ret(unsafe { __cosmo_real_clock_gettime(clockid(id), ts) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_clock_nanosleep(id: c_int, flags: c_int, req: *const c_void, rem: *mut c_void) -> c_int {
    // clock_nanosleep reports failure as a positive errno, not -1/errno.
    let r = unsafe { __cosmo_real_clock_nanosleep(clockid(id), flags, req, rem) };
    if r != 0 { xlate::errno_to_linux(r as i64) as c_int } else { 0 }
}

// ---- poll ------------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_poll(fds: *mut PollFd, n: u64, timeout: c_int) -> c_int {
    let g = gen::poll();
    if n > isize::MAX as u64 / core::mem::size_of::<PollFd>() as u64 { return fail(EINVAL_LINUX); }
    if fds.is_null() && n != 0 { return fail(EFAULT_LINUX); }
    let s = if n == 0 { &mut [] } else { unsafe { core::slice::from_raw_parts_mut(fds, n as usize) } };
    for p in s.iter_mut() { p.events = g.to_host(p.events as i64) as i16; }
    let r = unsafe { __cosmo_real_poll(fds, n, timeout) };
    for p in s.iter_mut() { p.events = g.to_linux(p.events as i64) as i16; p.revents = g.to_linux(p.revents as i64) as i16; }
    ret(r)
}

// ---- signals ---------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sigaction(signum: c_int, act: *const libc::sigaction, old: *mut libc::sigaction) -> c_int {
    let mut a_copy = CosmoSigAction { handler: 0, flags: 0, pad: 0, restorer: 0, mask: 0 };
    let ap = if act.is_null() { core::ptr::null() } else {
        let a = unsafe { &*act };
        a_copy.handler = a.sa_sigaction as usize;
        a_copy.flags = gen::sigact().to_host(a.sa_flags as i64) as c_int;
        // sa_restorer stays 0: std never sets one and cosmo returns from
        // signals on its own. sa_mask is 128 bytes in the caller's struct and
        // 8 in cosmo's -- the first 8 are the only ones either can hold.
        a_copy.mask = unsafe { core::ptr::read_unaligned(&a.sa_mask as *const _ as *const u64) };
        &a_copy as *const CosmoSigAction
    };
    let mut old_copy = CosmoSigAction { handler: 0, flags: 0, pad: 0, restorer: 0, mask: 0 };
    let oldp = if old.is_null() { core::ptr::null_mut() } else { &mut old_copy };
    let r = unsafe { __cosmo_real_sigaction(sig(signum), ap, oldp) };
    if r == -1 { fix_errno(); return r; }
    if !old.is_null() {
        unsafe {
            // Zero the whole caller's struct first: its sa_mask is 16x wider
            // than cosmo's, and the fields sit in a different order.
            let o = &mut *old;
            core::ptr::write_bytes(o as *mut libc::sigaction as *mut u8, 0, core::mem::size_of::<libc::sigaction>());
            core::ptr::write_unaligned(&mut o.sa_mask as *mut _ as *mut u64, old_copy.mask);
            o.sa_sigaction = old_copy.handler as _;
            o.sa_flags = gen::sigact().to_linux(old_copy.flags as i64) as _;
        }
    }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_signal(signum: c_int, handler: usize) -> usize {
    let r = unsafe { __cosmo_real_signal(sig(signum), handler) };
    if r == usize::MAX { fix_errno(); }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_kill(pid: c_int, signum: c_int) -> c_int { ret(unsafe { __cosmo_real_kill(pid, sig(signum)) }) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_killpg(pgrp: c_int, signum: c_int) -> c_int { ret(unsafe { __cosmo_real_killpg(pgrp, sig(signum)) }) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sigaddset(set: *mut c_void, signum: c_int) -> c_int { ret(unsafe { __cosmo_real_sigaddset(set, sig(signum)) }) }

// ---- processes -------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int {
    let r = unsafe { __cosmo_real_waitpid(pid, status, gen::waitpid().to_host(options as i64) as c_int) };
    if r == -1 { fix_errno(); return r; }
    // The wait status encodes the signal number for WIFSIGNALED/WIFSTOPPED.
    if r > 0 && !status.is_null() {
        let s = unsafe { *status };
        let low = s & 0x7f;
        if low != 0 && low != 0x7f { unsafe { *status = (s & !0x7f) | (sig_back(low) & 0x7f); } }
        else if low == 0x7f { let stop = (s >> 8) & 0xff; unsafe { *status = (s & !0xff00) | ((sig_back(stop) & 0xff) << 8); } }
    }
    r
}

/// setxattr and lsetxattr are referenced by rustix's libc backend and by the
/// C code cosmo ships, but cosmo's libc does not export them under those
/// names. Zebra never sets extended attributes -- rustix calls them for
/// tempfile's best-effort metadata -- so answer ENOSYS and let the caller
/// fall back, exactly as a filesystem without xattr support would.
#[unsafe(no_mangle)]
pub extern "C" fn setxattr(path: *const i8, name: *const i8, value: *const u8, size: usize, flags: i32) -> i32 {
    let _ = (path, name, value, size, flags);
    unsafe { *__errno_location() = libc::ENOSYS; }
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn lsetxattr(path: *const i8, name: *const i8, value: *const u8, size: usize, flags: i32) -> i32 {
    let _ = (path, name, value, size, flags);
    unsafe { *__errno_location() = libc::ENOSYS; }
    -1
}

// ---- ppoll -----------------------------------------------------------------------
unsafe extern "C" {
    fn __cosmo_real_ppoll(fds: *mut PollFd, n: u64, ts: *const c_void, mask: *const [u64; 2]) -> c_int;
}
/// A Linux sigset (bit sig-1, Linux signal numbers) in the host's numbering.
fn sigset_to_host(m: &[u64; 2]) -> [u64; 2] {
    let mut out = [0u64; 2];
    for s in 1..=128i32 {
        let (w, b) = (((s - 1) / 64) as usize, ((s - 1) % 64) as u32);
        if m[w] >> b & 1 == 1 {
            let h = sig(s);
            if (1..=128).contains(&h) { out[((h - 1) / 64) as usize] |= 1 << ((h - 1) % 64); }
        }
    }
    out
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_ppoll(fds: *mut PollFd, n: u64, ts: *const c_void, mask: *const [u64; 2]) -> c_int {
    let g = gen::poll();
    if n > isize::MAX as u64 / core::mem::size_of::<PollFd>() as u64 { return fail(EINVAL_LINUX); }
    if fds.is_null() && n != 0 { return fail(EFAULT_LINUX); }
    let s = if n == 0 { &mut [] } else { unsafe { core::slice::from_raw_parts_mut(fds, n as usize) } };
    for p in s.iter_mut() { p.events = g.to_host(p.events as i64) as i16; }
    let hmask;
    let mp = if mask.is_null() || xlate::hostos() == xlate::HOST_LINUX { mask } else {
        hmask = sigset_to_host(unsafe { &*mask });
        &hmask as *const [u64; 2]
    };
    let r = unsafe { __cosmo_real_ppoll(fds, n, ts, mp) };
    for p in s.iter_mut() { p.events = g.to_linux(p.events as i64) as i16; p.revents = g.to_linux(p.revents as i64) as i16; }
    ret(r)
}

// ---- getifaddrs ------------------------------------------------------------------
/// Linux layout, which cosmo shares.
#[repr(C)]
pub struct IfAddrs { next: *mut IfAddrs, name: *mut c_char, flags: c_uint, addr: *mut u16, netmask: *mut u16, ifu: *mut u16, data: *mut c_void }
unsafe extern "C" { fn __cosmo_real_getifaddrs(out: *mut *mut IfAddrs) -> c_int; }
/// Families inside each sockaddr and the IFF_* flags come back host-numbered.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_getifaddrs(out: *mut *mut IfAddrs) -> c_int {
    if out.is_null() { return fail(EFAULT_LINUX); }
    let r = unsafe { __cosmo_real_getifaddrs(out) };
    if r != 0 { fix_errno(); return r; }
    let mut p = unsafe { *out };
    while !p.is_null() {
        unsafe {
            (*p).flags = gen::iff().to_linux((*p).flags as i64) as c_uint;
            family_to_linux((*p).addr);
            family_to_linux((*p).netmask);
            family_to_linux((*p).ifu);
            p = (*p).next;
        }
    }
    0
}

// ---- syscall: futex ----------------------------------------------------------------
// cosmopolitan's syscall() implements only getrandom, gettid and getcpu and
// fails everything else with ENOSYS, on every host. std's Mutex/Condvar/park
// (sys/sync/futex) call syscall(SYS_futex, ...), so without this every
// blocking wait in std busy-spins. Map the futex operations std and other
// Linux code use onto cosmo's portable futex (futex(2) on Linux, ulock on XNU,
// WaitOnAddress on Windows, futex/umtx on the BSDs).
unsafe extern "C" {
    fn __cosmo_real_syscall(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize;
    fn cosmo_futex_wait(w: *mut c_int, expect: c_int, pshared: c_char, clock: c_int, abstime: *const Timespec) -> c_int;
    fn cosmo_futex_wake(w: *mut c_int, count: c_int, pshared: c_char) -> c_int;
}

#[cfg(target_arch = "x86_64")] const SYS_FUTEX: usize = 202;
#[cfg(target_arch = "aarch64")] const SYS_FUTEX: usize = 98;

const ENOSYS_LINUX: c_int = 38;
#[inline] fn fail_host(neg_host_errno: c_int) -> isize {
    unsafe { *__errno_location() = xlate::errno_to_linux(-neg_host_errno as i64) as c_int; }
    -1
}

unsafe fn futex(uaddr: *mut c_int, op: c_int, val: c_int, timeout: *const Timespec, val3: u32) -> isize {
    const WAIT: c_int = 0; const WAKE: c_int = 1; const WAIT_BITSET: c_int = 9; const WAKE_BITSET: c_int = 10;
    const PRIVATE: c_int = 128; const CLOCK_REALTIME_FLAG: c_int = 256;
    const CLOCK_REALTIME: c_int = 0; const CLOCK_MONOTONIC: c_int = 1; // Linux numbering
    let pshared = (op & PRIVATE == 0) as c_char;
    let cmd = op & !(PRIVATE | CLOCK_REALTIME_FLAG);
    match cmd {
        WAKE | WAKE_BITSET => {
            if cmd == WAKE_BITSET && val3 != u32::MAX { return fail(ENOSYS_LINUX) as isize; }
            let r = unsafe { cosmo_futex_wake(uaddr, val, pshared) };
            if r < 0 { fail_host(r) } else { r as isize }
        }
        WAIT | WAIT_BITSET => {
            if cmd == WAIT_BITSET && val3 != u32::MAX { return fail(ENOSYS_LINUX) as isize; }
            if cmd == WAIT && op & CLOCK_REALTIME_FLAG != 0 { return fail(ENOSYS_LINUX) as isize; }
            let lclock = if op & CLOCK_REALTIME_FLAG != 0 { CLOCK_REALTIME } else { CLOCK_MONOTONIC };
            let hclock = clockid(lclock);
            let abs;
            let ap = if timeout.is_null() { core::ptr::null() } else {
                let t = unsafe { *timeout };
                if t.tv_nsec < 0 || t.tv_nsec >= 1_000_000_000 || t.tv_sec < 0 { return fail(EINVAL_LINUX) as isize; }
                if cmd == WAIT {
                    // FUTEX_WAIT's timeout is relative (CLOCK_MONOTONIC); cosmo wants absolute.
                    let mut now = Timespec { tv_sec: 0, tv_nsec: 0 };
                    if unsafe { __cosmo_real_clock_gettime(hclock, &mut now as *mut Timespec as *mut c_void) } != 0 { fix_errno(); return -1; }
                    let mut ns = now.tv_nsec + t.tv_nsec;
                    let mut s = now.tv_sec.saturating_add(t.tv_sec);
                    if ns >= 1_000_000_000 { ns -= 1_000_000_000; s = s.saturating_add(1); }
                    abs = Timespec { tv_sec: s, tv_nsec: ns };
                } else {
                    abs = t;
                }
                &abs as *const Timespec
            };
            let r = unsafe { cosmo_futex_wait(uaddr, val, pshared, hclock, ap) };
            if r < 0 { fail_host(r) } else { 0 }
        }
        _ => fail(ENOSYS_LINUX) as isize,
    }
}

/// syscall(2) is variadic; on SysV x86-64 and AAPCS64 integer varargs travel
/// exactly like fixed arguments, so a seven-word signature forwards any call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_syscall(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    if n == SYS_FUTEX {
        return unsafe { futex(a as *mut c_int, b as c_int, c as c_int, d as *const Timespec, f as u32) };
    }
    let r = unsafe { __cosmo_real_syscall(n, a, b, c, d, e, f) };
    if r as i32 == -1 { fix_errno(); }
    r
}
