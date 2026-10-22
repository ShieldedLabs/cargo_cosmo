//! Linux-host runtime regression probe: no Cargo dependencies, all libc calls
//! use the Linux ABI, exactly as std and an unmodified dependency-free crate do.
use std::ffi::{c_char, c_int, c_void};
use std::net::UdpSocket;
use std::os::fd::AsRawFd;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[repr(C)] struct Iovec { base: *mut c_void, len: usize }
#[repr(C)] struct MsgHdr {
    name: *mut c_void, namelen: u32, iov: *mut Iovec, iovlen: usize,
    control: *mut c_void, controllen: usize, flags: c_int,
}
#[repr(C)] struct PollFd { fd: c_int, events: i16, revents: i16 }
#[repr(C)] struct Timespec { sec: i64, nsec: i64 }
#[repr(C)] struct IfAddrs {
    next: *mut IfAddrs, name: *mut c_char, flags: u32, addr: *mut u16,
    mask: *mut u16, ifu: *mut u16, data: *mut c_void,
}
#[repr(C, align(8))] struct Control([u8; 256]);
unsafe extern "C" {
    fn setsockopt(fd: c_int, level: c_int, name: c_int, val: *const c_void, len: u32) -> c_int;
    fn getsockopt(fd: c_int, level: c_int, name: c_int, val: *mut c_void, len: *mut u32) -> c_int;
    fn sendmsg(fd: c_int, msg: *const MsgHdr, flags: c_int) -> isize;
    fn recvmsg(fd: c_int, msg: *mut MsgHdr, flags: c_int) -> isize;
    fn ppoll(fd: *mut PollFd, n: usize, timeout: *const Timespec, mask: *const u64) -> c_int;
    fn socketpair(af: c_int, ty: c_int, protocol: c_int, pair: *mut c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
    fn close(fd: c_int) -> c_int;
    fn getifaddrs(out: *mut *mut IfAddrs) -> c_int;
    fn freeifaddrs(list: *mut IfAddrs);
    fn syscall(n: usize, ...) -> isize;
    fn __errno_location() -> *mut c_int;
}
fn opt(s: &UdpSocket, level: i32, name: i32, val: i32) {
    let r = unsafe { setsockopt(s.as_raw_fd(), level, name, &val as *const _ as *const c_void, 4) };
    assert_eq!(r, 0, "setsockopt({level},{name}): {}", std::io::Error::last_os_error());
}
fn get(s: &UdpSocket, level: i32, name: i32) -> i32 {
    let mut v = 0i32; let mut n = 4;
    assert_eq!(unsafe { getsockopt(s.as_raw_fd(), level, name, &mut v as *mut _ as *mut c_void, &mut n) }, 0);
    assert_eq!(n, 4); v
}
fn cmsg(c: &mut Control, off: usize, level: i32, ty: i32, data: &[u8]) -> usize {
    let n = 16 + data.len();
    c.0[off..off+8].copy_from_slice(&(n as u64).to_ne_bytes());
    c.0[off+8..off+12].copy_from_slice(&level.to_ne_bytes());
    c.0[off+12..off+16].copy_from_slice(&ty.to_ne_bytes());
    c.0[off+16..off+n].copy_from_slice(data);
    off + ((n + 7) & !7)
}
fn udp(v6: bool) {
    let addr = if v6 { "[::1]:0" } else { "127.0.0.1:0" };
    let tx = UdpSocket::bind(addr).unwrap(); let rx = UdpSocket::bind(addr).unwrap();
    tx.connect(rx.local_addr().unwrap()).unwrap();
    rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let level = if v6 { 41 } else { 0 }; let mtu = if v6 { 23 } else { 10 };
    opt(&tx, level, mtu, 3); assert_eq!(get(&tx, level, mtu), 3);
    opt(&rx, level, if v6 { 66 } else { 13 }, 1);
    if v6 { opt(&rx, 41, 49, 1); }
    let mut out = Control([0; 256]);
    let mut n = cmsg(&mut out, 0, level, if v6 { 67 } else { 1 }, &0x2ei32.to_ne_bytes());
    if v6 {
        let mut info = [0u8; 20]; info[15] = 1; // in6_pktinfo: ::1, ifindex=0
        n = cmsg(&mut out, n, 41, 50, &info);
    }
    let mut byte = 123u8;
    let mut iov = Iovec { base: &mut byte as *mut _ as *mut c_void, len: 1 };
    let mut msg = MsgHdr { name: null_mut(), namelen: 0, iov: &mut iov, iovlen: 1,
        control: out.0.as_mut_ptr().cast(), controllen: n, flags: 0 };
    assert_eq!(unsafe { sendmsg(tx.as_raw_fd(), &msg, 0) }, 1, "sendmsg: {}", std::io::Error::last_os_error());
    byte = 0;
    let mut control = Control([0; 256]);
    msg.control = control.0.as_mut_ptr().cast(); msg.controllen = 256;
    assert_eq!(unsafe { recvmsg(rx.as_raw_fd(), &mut msg, 0) }, 1, "recvmsg: {}", std::io::Error::last_os_error());
    assert_eq!(byte, 123); assert_eq!(msg.flags & 8, 0);
    let mut off = 0; let mut tos = false; let mut pkt = !v6;
    while off + 16 <= msg.controllen {
        let n = u64::from_ne_bytes(control.0[off..off+8].try_into().unwrap()) as usize;
        assert!(n >= 16 && off+n <= msg.controllen);
        let l = i32::from_ne_bytes(control.0[off+8..off+12].try_into().unwrap());
        let t = i32::from_ne_bytes(control.0[off+12..off+16].try_into().unwrap());
        if l == level && t == if v6 { 67 } else { 1 } { assert_eq!(control.0[off+16], 0x2e); tos = true; }
        if v6 && l == 41 && t == 50 { assert_eq!(n, 36); assert_eq!(control.0[off+31], 1); pkt = true; }
        off += (n+7) & !7;
    }
    assert!(tos && pkt, "missing received TOS/TCLASS or PKTINFO");
    println!("UDP{}_PMTU_CMSG_OK", if v6 { 6 } else { 4 });
}
fn poll_and_interfaces() {
    let mut pair = [-1; 2];
    assert_eq!(unsafe { socketpair(1, 1 | 0x800 | 0x80000, 0, pair.as_mut_ptr()) }, 0);
    let mut p = PollFd { fd: pair[1], events: 1, revents: 0 };
    let timeout = Timespec { sec: 0, nsec: 10_000_000 }; let mask = [0u64; 16];
    assert_eq!(unsafe { ppoll(&mut p, 1, &timeout, mask.as_ptr()) }, 0);
    let byte = 9u8; assert_eq!(unsafe { write(pair[0], (&byte as *const u8).cast(), 1) }, 1);
    assert_eq!(unsafe { ppoll(&mut p, 1, &timeout, mask.as_ptr()) }, 1);
    assert_eq!(p.events, 1); assert_eq!(p.revents & 1, 1);
    let mut byte = 0u8; assert_eq!(unsafe { read(pair[1], (&mut byte as *mut u8).cast(), 1) }, 1); assert_eq!(byte, 9);
    assert_eq!(unsafe { ppoll(null_mut(), 0, &timeout, null()) }, 0);
    unsafe { close(pair[0]); close(pair[1]); }
    let mut list = null_mut(); assert_eq!(unsafe { getifaddrs(&mut list) }, 0);
    let mut p = list; let mut loopback = false;
    while !p.is_null() {
        let a = unsafe { &*p };
        if !a.addr.is_null() && a.flags & 8 != 0 {
            let af = unsafe { *a.addr }; if af == 2 || af == 10 { loopback = true; }
        }
        p = a.next;
    }
    unsafe { freeifaddrs(list); }
    assert!(loopback); println!("PPOLL_GETIFADDRS_OK");
}
fn ticks() -> u64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap();
    let f: Vec<_> = s.rsplit(')').next().unwrap().split_whitespace().collect();
    f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap()
}
fn sleeping_futex() {
    let cpu = ticks(); let start = Instant::now();
    let pair = Arc::new((Mutex::new(false), Condvar::new())); let child = pair.clone();
    let h = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(600)); *child.0.lock().unwrap() = true; child.1.notify_all(); });
    let mut g = pair.0.lock().unwrap(); while !*g { g = pair.1.wait(g).unwrap(); } drop(g); h.join().unwrap();
    let this = std::thread::current();
    let h = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(400)); this.unpark(); });
    std::thread::park_timeout(Duration::from_secs(2)); h.join().unwrap();
    // Exercise timeout and mismatched-value errno paths, not just wakes.
    #[cfg(target_arch="x86_64")] const FUTEX: usize = 202;
    #[cfg(target_arch="aarch64")] const FUTEX: usize = 98;
    let word = 0i32; let rel = Timespec { sec: 0, nsec: 10_000_000 };
    assert_eq!(unsafe { syscall(FUTEX, &word, 128i32, 1i32, &rel) }, -1);
    assert_eq!(unsafe { *__errno_location() }, 11);
    assert_eq!(unsafe { syscall(FUTEX, &word, 128i32, 0i32, &rel) }, -1);
    assert_eq!(unsafe { *__errno_location() }, 110);
    let wall = start.elapsed(); let cpu = ticks() - cpu;
    assert!(wall >= Duration::from_millis(990)); assert!(cpu < 20, "wait busy-spins: {cpu} CPU ticks");
    println!("FUTEX_SLEEP_OK wall_ms={} cpu_ticks={cpu}", wall.as_millis());
}
fn main() {
    udp(false); udp(true); poll_and_interfaces(); sleeping_futex();
    assert!(std::io::Error::from_raw_os_error(2).to_string().contains("No such file"));
    println!("ZERO_DEPS_OK");
}
