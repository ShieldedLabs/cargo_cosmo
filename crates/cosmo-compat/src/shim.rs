//! The libc-boundary wrappers. cosmo-ld links every program with
//! `--wrap=NAME` for each name in ../wrap.txt, so a call to `open` from std
//! (or from anything else in the link) lands in `__wrap_open` here, which
//! translates the Linux-numbered arguments to the host's numbering, calls
//! `__real_open` (cosmo's), and translates errno / results back.
//!
//! Only the functions whose ARGUMENTS carry constants are written here; the
//! rest are generated register-passthrough wrappers in gen.rs that only fix
//! errno. Anything not in wrap.txt reaches cosmo directly.
//!
//! Known hazard: `--wrap` also redirects cosmopolitan's own internal calls to
//! these public names, and those pass HOST-numbered constants. On Windows the
//! groups cosmo's internals use (open, mmap, signals) keep Linux numbering, so
//! the translation is the identity there and the hazard is theoretical; on
//! XNU it is not, and a cosmo-internal open(O_CLOEXEC) would be re-translated.
//! The fix, if it bites, is for cosmo-ld to wrap only std's objects
//! (objcopy --redefine-sym on the rlibs) instead of the whole link.

#![allow(clippy::missing_safety_doc)]

use core::ffi::{c_char, c_int, c_uint, c_void};
use crate::gen;
use crate::xlate;

unsafe extern "C" {
    fn __errno_location() -> *mut c_int;
    fn __real_open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn __real_openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    fn __real_fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn __real_ioctl(fd: c_int, req: u64, ...) -> c_int;
    fn __real_socket(domain: c_int, ty: c_int, proto: c_int) -> c_int;
    fn __real_socketpair(domain: c_int, ty: c_int, proto: c_int, sv: *mut c_int) -> c_int;
    fn __real_setsockopt(fd: c_int, level: c_int, name: c_int, val: *const c_void, len: u32) -> c_int;
    fn __real_getsockopt(fd: c_int, level: c_int, name: c_int, val: *mut c_void, len: *mut u32) -> c_int;
    fn __real_send(fd: c_int, buf: *const c_void, n: usize, flags: c_int) -> isize;
    fn __real_sendto(fd: c_int, buf: *const c_void, n: usize, flags: c_int, addr: *const c_void, alen: u32) -> isize;
    fn __real_recv(fd: c_int, buf: *mut c_void, n: usize, flags: c_int) -> isize;
    fn __real_recvfrom(fd: c_int, buf: *mut c_void, n: usize, flags: c_int, addr: *mut c_void, alen: *mut u32) -> isize;
    fn __real_sendmsg(fd: c_int, msg: *const MsgHdr, flags: c_int) -> isize;
    fn __real_recvmsg(fd: c_int, msg: *mut MsgHdr, flags: c_int) -> isize;
    fn __real_poll(fds: *mut PollFd, n: u64, timeout: c_int) -> c_int;
    fn __real_mmap(addr: *mut c_void, len: u64, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void;
    fn __real_sigaction(sig: c_int, act: *const SigAction, old: *mut SigAction) -> c_int;
    fn __real_signal(sig: c_int, handler: usize) -> usize;
    fn __real_kill(pid: c_int, sig: c_int) -> c_int;
    fn __real_killpg(pgrp: c_int, sig: c_int) -> c_int;
    fn __real_sigaddset(set: *mut c_void, sig: c_int) -> c_int;
    fn __real_waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    fn __real_unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int;
    fn __real_linkat(olddir: c_int, old: *const c_char, newdir: c_int, new: *const c_char, flags: c_int) -> c_int;
    fn __real_renameat(olddir: c_int, old: *const c_char, newdir: c_int, new: *const c_char) -> c_int;
    fn __real_fchmodat(dirfd: c_int, path: *const c_char, mode: c_uint, flags: c_int) -> c_int;
    fn __real_utimensat(dirfd: c_int, path: *const c_char, times: *const c_void, flags: c_int) -> c_int;
    fn __real_getaddrinfo(node: *const c_char, service: *const c_char, hints: *const AddrInfo, res: *mut *mut AddrInfo) -> c_int;
    fn __real_freeaddrinfo(ai: *mut AddrInfo);
}

#[repr(C)]
pub struct PollFd { fd: i32, events: i16, revents: i16 }
#[repr(C)]
pub struct MsgHdr { name: *mut c_void, namelen: u32, iov: *mut c_void, iovlen: u64, control: *mut c_void, controllen: u64, flags: u32 }
/// Cosmo's layout: {handler/sigaction, sa_flags u64, sa_restorer, sa_mask}.
#[repr(C)]
pub struct SigAction { handler: usize, flags: u64, restorer: usize, mask: [u64; 2] }
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
unsafe fn family_to_host(addr: *mut u16) { if !addr.is_null() { unsafe { *addr = gen::af().to_host(*addr as i64) as u16; } } }
unsafe fn family_to_linux(addr: *mut u16) { if !addr.is_null() { unsafe { *addr = gen::af().to_linux(*addr as i64) as u16; } } }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_open(path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    ret(unsafe { __real_open(path, open_flags(flags), mode) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_openat(dirfd: c_int, path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    ret(unsafe { __real_openat(at_fd(dirfd), path, open_flags(flags), mode) })
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
    let r = unsafe { __real_fcntl(fd, hcmd, harg) };
    if r == -1 { fix_errno(); return r; }
    if cmd == F_GETFL { return gen::open().to_linux(r as i64) as c_int; }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_ioctl(fd: c_int, req: u64, arg: usize) -> c_int {
    ret(unsafe { __real_ioctl(fd, gen::ioctl().to_host(req as i64) as u64, arg) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int {
    ret(unsafe { __real_unlinkat(at_fd(dirfd), path, at_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_linkat(od: c_int, old: *const c_char, nd: c_int, new: *const c_char, flags: c_int) -> c_int {
    ret(unsafe { __real_linkat(at_fd(od), old, at_fd(nd), new, at_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_renameat(od: c_int, old: *const c_char, nd: c_int, new: *const c_char) -> c_int {
    ret(unsafe { __real_renameat(at_fd(od), old, at_fd(nd), new) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_fchmodat(dirfd: c_int, path: *const c_char, mode: c_uint, flags: c_int) -> c_int {
    ret(unsafe { __real_fchmodat(at_fd(dirfd), path, mode, at_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_utimensat(dirfd: c_int, path: *const c_char, times: *const c_void, flags: c_int) -> c_int {
    ret(unsafe { __real_utimensat(at_fd(dirfd), path, times, at_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_mmap(addr: *mut c_void, len: u64, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void {
    let r = unsafe { __real_mmap(addr, len, gen::mprot().to_host(prot as i64) as c_int, gen::mmap().to_host(flags as i64) as c_int, fd, off) };
    if r as isize == -1 { fix_errno(); }
    r
}

// ---- sockets ---------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_socket(domain: c_int, ty: c_int, proto: c_int) -> c_int {
    ret(unsafe { __real_socket(gen::af().to_host(domain as i64) as c_int, gen::sock().to_host(ty as i64) as c_int, proto) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_socketpair(domain: c_int, ty: c_int, proto: c_int, sv: *mut c_int) -> c_int {
    ret(unsafe { __real_socketpair(gen::af().to_host(domain as i64) as c_int, gen::sock().to_host(ty as i64) as c_int, proto, sv) })
}
/// (level, name) in host numbering. Levels: SOL_SOCKET is in the `so` group;
/// IPPROTO_* are the same everywhere. Option names are per level.
fn sockopt(level: c_int, name: c_int) -> (c_int, c_int) {
    const IPPROTO_IP: c_int = 0; const IPPROTO_TCP: c_int = 6; const IPPROTO_IPV6: c_int = 41;
    let so = gen::so();
    let sol_socket_linux = so.name_of_linux(1).map(|_| 1).unwrap_or(1);
    if level == sol_socket_linux && so.names.contains(&"SOL_SOCKET") {
        return (so.to_host(1) as c_int, so.to_host(name as i64) as c_int);
    }
    match level {
        IPPROTO_TCP => (level, gen::tcp().to_host(name as i64) as c_int),
        IPPROTO_IP => (level, gen::ip().to_host(name as i64) as c_int),
        IPPROTO_IPV6 => (level, gen::ipv6().to_host(name as i64) as c_int),
        _ => (level, name),
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_setsockopt(fd: c_int, level: c_int, name: c_int, val: *const c_void, len: u32) -> c_int {
    let (l, n) = sockopt(level, name);
    ret(unsafe { __real_setsockopt(fd, l, n, val, len) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_getsockopt(fd: c_int, level: c_int, name: c_int, val: *mut c_void, len: *mut u32) -> c_int {
    const SO_ERROR_LINUX: c_int = 4; const SO_TYPE_LINUX: c_int = 3;
    let (l, n) = sockopt(level, name);
    let r = unsafe { __real_getsockopt(fd, l, n, val, len) };
    if r == -1 { fix_errno(); return r; }
    // Results that are themselves constants.
    if level == 1 && !val.is_null() && unsafe { *len } >= 4 {
        let p = val as *mut c_int;
        if name == SO_ERROR_LINUX { unsafe { *p = xlate::errno_to_linux(*p as i64) as c_int; } }
        if name == SO_TYPE_LINUX { unsafe { *p = gen::sock().to_linux(*p as i64) as c_int; } }
    }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_send(fd: c_int, buf: *const c_void, n: usize, flags: c_int) -> isize {
    rets(unsafe { __real_send(fd, buf, n, msg_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sendto(fd: c_int, buf: *const c_void, n: usize, flags: c_int, addr: *const c_void, alen: u32) -> isize {
    unsafe { with_host_addr(addr, alen, |a| rets(__real_sendto(fd, buf, n, msg_flags(flags), a, alen))) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_recv(fd: c_int, buf: *mut c_void, n: usize, flags: c_int) -> isize {
    rets(unsafe { __real_recv(fd, buf, n, msg_flags(flags)) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_recvfrom(fd: c_int, buf: *mut c_void, n: usize, flags: c_int, addr: *mut c_void, alen: *mut u32) -> isize {
    let r = rets(unsafe { __real_recvfrom(fd, buf, n, msg_flags(flags), addr, alen) });
    if r >= 0 { unsafe { family_to_linux(addr as *mut u16); } }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sendmsg(fd: c_int, msg: *const MsgHdr, flags: c_int) -> isize {
    // The name pointer inside the header is const to the caller; rewrite through a copy of the header.
    let m = unsafe { &*msg };
    if m.name.is_null() { return rets(unsafe { __real_sendmsg(fd, msg, msg_flags(flags)) }); }
    unsafe {
        with_host_addr(m.name, m.namelen, |a| {
            let copy = MsgHdr { name: a as *mut c_void, namelen: m.namelen, iov: m.iov, iovlen: m.iovlen, control: m.control, controllen: m.controllen, flags: m.flags };
            rets(__real_sendmsg(fd, &copy, msg_flags(flags)))
        })
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_recvmsg(fd: c_int, msg: *mut MsgHdr, flags: c_int) -> isize {
    let r = rets(unsafe { __real_recvmsg(fd, msg, msg_flags(flags)) });
    if r >= 0 {
        unsafe {
            family_to_linux((*msg).name as *mut u16);
            (*msg).flags = gen::msg().to_linux((*msg).flags as i64) as u32;
        }
    }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_getaddrinfo(node: *const c_char, service: *const c_char, hints: *const AddrInfo, res: *mut *mut AddrInfo) -> c_int {
    let mut h_copy;
    let hp = if hints.is_null() { hints } else {
        let h = unsafe { &*hints };
        h_copy = AddrInfo { flags: h.flags, family: gen::af().to_host(h.family as i64) as c_int, socktype: gen::sock().to_host(h.socktype as i64) as c_int, protocol: h.protocol, addrlen: 0, addr: core::ptr::null_mut(), canonname: core::ptr::null_mut(), next: core::ptr::null_mut() };
        &mut h_copy as *const AddrInfo
    };
    let r = unsafe { __real_getaddrinfo(node, service, hp, res) };
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
    unsafe { __real_freeaddrinfo(ai) }
}

// ---- poll ------------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_poll(fds: *mut PollFd, n: u64, timeout: c_int) -> c_int {
    let g = gen::poll();
    let s = unsafe { core::slice::from_raw_parts_mut(fds, n as usize) };
    for p in s.iter_mut() { p.events = g.to_host(p.events as i64) as i16; }
    let r = unsafe { __real_poll(fds, n, timeout) };
    for p in s.iter_mut() { p.events = g.to_linux(p.events as i64) as i16; p.revents = g.to_linux(p.revents as i64) as i16; }
    ret(r)
}

// ---- signals ---------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sigaction(signum: c_int, act: *const SigAction, old: *mut SigAction) -> c_int {
    let mut a_copy;
    let ap = if act.is_null() { act } else {
        let a = unsafe { &*act };
        a_copy = SigAction { handler: a.handler, flags: gen::sigact().to_host(a.flags as i64) as u64, restorer: a.restorer, mask: a.mask };
        &mut a_copy as *const SigAction
    };
    let r = unsafe { __real_sigaction(sig(signum), ap, old) };
    if r == -1 { fix_errno(); return r; }
    if !old.is_null() { unsafe { (*old).flags = gen::sigact().to_linux((*old).flags as i64) as u64; } }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_signal(signum: c_int, handler: usize) -> usize {
    let r = unsafe { __real_signal(sig(signum), handler) };
    if r == usize::MAX { fix_errno(); }
    r
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_kill(pid: c_int, signum: c_int) -> c_int { ret(unsafe { __real_kill(pid, sig(signum)) }) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_killpg(pgrp: c_int, signum: c_int) -> c_int { ret(unsafe { __real_killpg(pgrp, sig(signum)) }) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_sigaddset(set: *mut c_void, signum: c_int) -> c_int { ret(unsafe { __real_sigaddset(set, sig(signum)) }) }

// ---- processes -------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int {
    let r = unsafe { __real_waitpid(pid, status, gen::waitpid().to_host(options as i64) as c_int) };
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
