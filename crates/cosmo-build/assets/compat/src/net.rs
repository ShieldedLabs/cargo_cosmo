//! Socket-option and ancillary-data (cmsg) policy for non-Linux hosts.
//!
//! Callers on the cosmo target speak Linux: Linux option numbers, Linux
//! `struct cmsghdr` (16-byte header, 8-byte alignment, `cmsg_len` a u64). The
//! syscon table only covers names cosmopolitan itself knows, and a name it does
//! not know used to reach the host kernel raw, where it can mean something else
//! entirely (Linux IP_MTU_DISCOVER = 10 is IP_MULTICAST_TTL on XNU). The rule
//! here: on a non-Linux host an option is either translated to a host option
//! with the same meaning or refused with ENOPROTOOPT -- never passed through.
//! On a Linux host everything is the identity.
//!
//! Ancillary data: cosmopolitan converts `struct msghdr` for XNU but hands the
//! control buffer to the kernel raw, and on Windows refuses any msghdr with a
//! control buffer (EINVAL). So:
//!   XNU      cmsgs are converted Linux <-> BSD layout and numbering; a send-side
//!            cmsg with no XNU equivalent fails EINVAL, a received one with no
//!            Linux equivalent is dropped and reported with MSG_CTRUNC.
//!   Windows  send with non-empty control fails EINVAL; receive succeeds with
//!            msg_controllen = 0 (the options that would enable ancillary data
//!            are refused with ENOPROTOOPT, so none is ever pending).
//!   other    (FreeBSD/OpenBSD/NetBSD, untested) send with control fails
//!            EINVAL; anything received in control is dropped with MSG_CTRUNC.

use core::ffi::c_int;
use crate::gen;
use crate::xlate::{hostos, HOST_LINUX, HOST_WINDOWS, HOST_XNU};

pub const SOL_SOCKET: c_int = 1;
pub const IPPROTO_IP: c_int = 0;
pub const IPPROTO_TCP: c_int = 6;
pub const IPPROTO_IPV6: c_int = 41;

// Linux option numbers handled specially.
const IP_PKTINFO: c_int = 8;
const IP_MTU_DISCOVER: c_int = 10;
const IP_RECVTTL: c_int = 12;
const IP_RECVTOS: c_int = 13;
const IPV6_MTU_DISCOVER: c_int = 23;
const IPV6_RECVPKTINFO: c_int = 49;
const IPV6_PKTINFO: c_int = 50;
const IPV6_RECVHOPLIMIT: c_int = 51;
const IPV6_HOPLIMIT: c_int = 52;
const IPV6_DONTFRAG: c_int = 62;
const IPV6_RECVTCLASS: c_int = 66;
const IPV6_TCLASS: c_int = 67;
const SCM_RIGHTS: c_int = 1;

// Linux IP_PMTUDISC_* values.
const PMTU_DONT: c_int = 0;
const PMTU_WANT: c_int = 1;
const PMTU_DO: c_int = 2;
const PMTU_PROBE: c_int = 3;

/// How an option's int value must be rewritten.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Val {
    Same,
    /// Linux IP_PMTUDISC_* -> Windows PMTUD_STATE (NOT_SET 0, DO 1, DONT 2, PROBE 3).
    PmtuWindows,
    /// Linux IP_PMTUDISC_* -> BSD boolean IP_DONTFRAG / IPV6_DONTFRAG.
    PmtuDontfrag,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostOpt { pub level: c_int, pub name: c_int, pub val: Val }

/// Look a Linux option number up in a group, considering only names with the
/// given prefix (groups also hold levels and values: SOL_SOCKET is in `so`).
fn lookup(g: &crate::xlate::Group, prefix: &str, name: c_int) -> Option<i64> {
    // group() temporarily returns the Linux backing slice while the host
    // table is initializing. Off Linux that is not a safe sockopt mapping.
    if core::ptr::eq(g.host, g.linux) { return None; }
    for (i, n) in g.names.iter().enumerate() {
        if n.starts_with(prefix) && g.linux[i] == name as i64 {
            let h = g.host[i];
            return if h > 0 { Some(h) } else { None };
        }
    }
    None
}

#[derive(Clone, Copy)]
pub enum Tbl { So, Tcp, Ip, Ipv6 }

/// (level, name) as the host wants them, or None for ENOPROTOOPT.
pub fn sockopt(level: c_int, name: c_int) -> Option<HostOpt> {
    sockopt_on(hostos(), level, name, |t| match t {
        Tbl::So => gen::so(), Tbl::Tcp => gen::tcp(), Tbl::Ip => gen::ip(), Tbl::Ipv6 => gen::ipv6(),
    })
}

/// `sockopt` for an explicit host and table source (tests pass the extracted
/// Windows/XNU columns directly).
pub fn sockopt_on(host: i32, level: c_int, name: c_int, tbl: impl Fn(Tbl) -> crate::xlate::Group) -> Option<HostOpt> {
    if host == HOST_LINUX { return Some(HostOpt { level, name, val: Val::Same }); }
    let same = |l: c_int, n: c_int| Some(HostOpt { level: l, name: n, val: Val::Same });
    match (host, level, name) {
        // Path-MTU discovery. Callers that do their own PLPMTUD set PROBE: DF on,
        // no kernel clamp. Windows has the same option and values (reordered);
        // the BSDs have only a DF boolean.
        (HOST_WINDOWS, IPPROTO_IP, IP_MTU_DISCOVER) | (HOST_WINDOWS, IPPROTO_IPV6, IPV6_MTU_DISCOVER) =>
            return Some(HostOpt { level, name: 71, val: Val::PmtuWindows }),
        (HOST_XNU, IPPROTO_IP, IP_MTU_DISCOVER) =>
            return Some(HostOpt { level, name: 28, val: Val::PmtuDontfrag }),
        (HOST_XNU, IPPROTO_IPV6, IPV6_MTU_DISCOVER) =>
            return Some(HostOpt { level, name: 62, val: Val::PmtuDontfrag }),
        (_, IPPROTO_IP, IP_MTU_DISCOVER) | (_, IPPROTO_IPV6, IPV6_MTU_DISCOVER) => return None,
        // Windows: cosmopolitan cannot deliver ancillary data at all, so the
        // options that ask for it are refused rather than silently ineffective.
        (HOST_WINDOWS, IPPROTO_IP, IP_PKTINFO | IP_RECVTTL | IP_RECVTOS) => return None,
        (HOST_WINDOWS, IPPROTO_IPV6, IPV6_RECVPKTINFO | IPV6_RECVHOPLIMIT | IPV6_RECVTCLASS) => return None,
        // XNU's RFC 3542 numbers, which cosmopolitan's table lacks or zeroes.
        (HOST_XNU, IPPROTO_IPV6, IPV6_RECVPKTINFO) => return same(level, 61),
        (HOST_XNU, IPPROTO_IPV6, IPV6_PKTINFO) => return same(level, 46),
        (HOST_XNU, IPPROTO_IPV6, IPV6_RECVHOPLIMIT) => return same(level, 37),
        (HOST_XNU, IPPROTO_IPV6, IPV6_HOPLIMIT) => return same(level, 47),
        (HOST_XNU, IPPROTO_IPV6, IPV6_DONTFRAG) => return same(level, 62),
        _ => {}
    }
    match level {
        SOL_SOCKET => {
            let so = tbl(Tbl::So);
            let l = lookup(&so, "SOL_", SOL_SOCKET)?;
            Some(HostOpt { level: l as c_int, name: lookup(&so, "SO_", name)? as c_int, val: Val::Same })
        }
        IPPROTO_TCP => same(level, lookup(&tbl(Tbl::Tcp), "TCP_", name)? as c_int),
        IPPROTO_IP => same(level, lookup(&tbl(Tbl::Ip), "IP_", name)? as c_int),
        IPPROTO_IPV6 => same(level, lookup(&tbl(Tbl::Ipv6), "IPV6_", name)? as c_int),
        _ => None,
    }
}

/// Linux value -> host value for setsockopt; None for EINVAL.
pub fn val_to_host(v: Val, x: c_int) -> Option<c_int> {
    match v {
        Val::Same => Some(x),
        Val::PmtuWindows => match x { PMTU_DONT => Some(2), PMTU_DO => Some(1), PMTU_PROBE => Some(3), _ => None },
        Val::PmtuDontfrag => match x { PMTU_DONT => Some(0), PMTU_DO | PMTU_PROBE => Some(1), _ => None },
    }
}

/// Host value -> Linux value for getsockopt. Lossy where the host is coarser:
/// a BSD DF boolean reads back as DO, Windows NOT_SET as WANT (Linux's default).
pub fn val_to_linux(v: Val, x: c_int) -> c_int {
    match v {
        Val::Same => x,
        Val::PmtuWindows => match x { 0 => PMTU_WANT, 1 => PMTU_DO, 2 => PMTU_DONT, 3 => PMTU_PROBE, o => o },
        Val::PmtuDontfrag => if x != 0 { PMTU_DO } else { PMTU_DONT },
    }
}

// ---- cmsg layouts ------------------------------------------------------------------
pub const LINUX_HDR: usize = 16;
#[inline] pub const fn linux_align(n: usize) -> usize { (n + 7) & !7 }
pub const BSD_HDR: usize = 12;
#[inline] pub const fn bsd_align(n: usize) -> usize { (n + 3) & !3 }

/// (level, type) Linux -> XNU for a cmsg; None if XNU has no equivalent.
pub fn cmsg_to_xnu(level: c_int, ty: c_int) -> Option<(c_int, c_int)> {
    match (level, ty) {
        (SOL_SOCKET, SCM_RIGHTS) => Some((0xffff, 1)),
        (IPPROTO_IPV6, IPV6_TCLASS) => Some((IPPROTO_IPV6, 36)),
        (IPPROTO_IPV6, IPV6_PKTINFO) => Some((IPPROTO_IPV6, 46)),
        (IPPROTO_IPV6, IPV6_HOPLIMIT) => Some((IPPROTO_IPV6, 47)),
        (IPPROTO_IPV6, IPV6_DONTFRAG) => Some((IPPROTO_IPV6, 62)),
        _ => None,
    }
}

/// (level, type) XNU -> Linux; None if Linux has no equivalent.
pub fn cmsg_from_xnu(level: c_int, ty: c_int) -> Option<(c_int, c_int)> {
    match (level, ty) {
        (0xffff, 1) => Some((SOL_SOCKET, SCM_RIGHTS)),
        (IPPROTO_IPV6, 36) => Some((IPPROTO_IPV6, IPV6_TCLASS)),
        (IPPROTO_IPV6, 46) => Some((IPPROTO_IPV6, IPV6_PKTINFO)),
        (IPPROTO_IPV6, 47) => Some((IPPROTO_IPV6, IPV6_HOPLIMIT)),
        _ => None,
    }
}

#[inline] unsafe fn rd_u32(p: *const u8) -> u32 { unsafe { core::ptr::read_unaligned(p as *const u32) } }
#[inline] unsafe fn rd_u64(p: *const u8) -> u64 { unsafe { core::ptr::read_unaligned(p as *const u64) } }
#[inline] unsafe fn wr_u32(p: *mut u8, v: u32) { unsafe { core::ptr::write_unaligned(p as *mut u32, v) } }
#[inline] unsafe fn wr_u64(p: *mut u8, v: u64) { unsafe { core::ptr::write_unaligned(p as *mut u64, v) } }

/// Linux cmsgs in `src[..srclen]` -> XNU cmsgs in `dst`. Returns the XNU
/// control length, or None (EINVAL) for a malformed buffer, a cmsg XNU cannot
/// express, or a `dst` too small.
pub unsafe fn linux_to_xnu(src: *const u8, srclen: usize, dst: *mut u8, dstcap: usize) -> Option<usize> {
    if srclen == 0 { return Some(0); }
    if src.is_null() || dst.is_null() || srclen > isize::MAX as usize || dstcap > isize::MAX as usize || srclen < LINUX_HDR { return None; }
    let mut off = 0usize;
    let mut out = 0usize;
    while off + LINUX_HDR <= srclen {
        let len = unsafe { rd_u64(src.add(off)) } as usize;
        if len < LINUX_HDR || len > srclen - off { return None; }
        let level = unsafe { rd_u32(src.add(off + 8)) } as c_int;
        let ty = unsafe { rd_u32(src.add(off + 12)) } as c_int;
        let n = len - LINUX_HDR;
        let (hl, ht) = cmsg_to_xnu(level, ty)?;
        let space = BSD_HDR + bsd_align(n);
        if space > dstcap - out { return None; }
        unsafe {
            core::ptr::write_bytes(dst.add(out), 0, space);
            wr_u32(dst.add(out), (BSD_HDR + n) as u32);
            wr_u32(dst.add(out + 4), hl as u32);
            wr_u32(dst.add(out + 8), ht as u32);
            core::ptr::copy_nonoverlapping(src.add(off + LINUX_HDR), dst.add(out + BSD_HDR), n);
        }
        out += space;
        off += linux_align(len);
    }
    Some(out)
}

/// XNU cmsgs in `src[..srclen]` -> Linux cmsgs in `dst`. Returns (Linux
/// control length, truncated): a cmsg with no Linux equivalent, or one that
/// does not fit, is dropped and reported as truncation (MSG_CTRUNC).
/// Each Linux header is written in full: the 8-byte cmsg_len and zeroed padding.
pub unsafe fn xnu_to_linux(src: *const u8, srclen: usize, dst: *mut u8, dstcap: usize, mut close_dropped_fd: impl FnMut(c_int)) -> (usize, bool) {
    if srclen == 0 { return (0, false); }
    if src.is_null() || srclen > isize::MAX as usize || dstcap > isize::MAX as usize || srclen < BSD_HDR { return (0, true); }
    let dstcap = if dst.is_null() { 0 } else { dstcap };
    let mut off = 0usize;
    let mut out = 0usize;
    let mut trunc = false;
    while off + BSD_HDR <= srclen {
        let len = unsafe { rd_u32(src.add(off)) } as usize;
        if len < BSD_HDR || len > srclen - off { trunc = true; break; }
        let level = unsafe { rd_u32(src.add(off + 4)) } as c_int;
        let ty = unsafe { rd_u32(src.add(off + 8)) } as c_int;
        let n = len - BSD_HDR;
        match cmsg_from_xnu(level, ty) {
            Some((ll, lt)) if LINUX_HDR + n <= dstcap - out => {
                let space = core::cmp::min(LINUX_HDR + linux_align(n), dstcap - out);
                unsafe {
                    core::ptr::write_bytes(dst.add(out), 0, space);
                    wr_u64(dst.add(out), (LINUX_HDR + n) as u64);
                    wr_u32(dst.add(out + 8), ll as u32);
                    wr_u32(dst.add(out + 12), lt as u32);
                    core::ptr::copy_nonoverlapping(src.add(off + BSD_HDR), dst.add(out + LINUX_HDR), n);
                }
                out += space;
            }
            _ => {
                trunc = true;
                // Receiving SCM_RIGHTS creates owned fds even if conversion
                // cannot fit the Linux header. Do not leak discarded rights.
                if level == 0xffff && ty == 1 {
                    for i in (0..n / 4).map(|i| i * 4) {
                        close_dropped_fd(unsafe { rd_u32(src.add(off + BSD_HDR + i)) } as c_int);
                    }
                }
            },
        }
        off += bsd_align(len);
    }
    (out, trunc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xlate::{Group, Kind};
    // Explicit host + the extracted table columns: no process-wide env, no
    // lazily filled host tables.
    fn on(h: &str, level: c_int, name: c_int) -> Option<HostOpt> {
        let (host, xnu) = match h { "linux" => (1, false), "xnu" => (8, true), "windows" => (4, false), _ => (32, true) };
        sockopt_on(host, level, name, |t| {
            let (names, linux, w, x): (&'static [&'static str], &'static [i64], &'static [i64], &'static [i64]) = match t {
                Tbl::So => (&gen::NAMES_so, &gen::LINUX_so, &gen::WINDOWS_so, &gen::XNU_so),
                Tbl::Tcp => (&gen::NAMES_tcp, &gen::LINUX_tcp, &gen::WINDOWS_tcp, &gen::XNU_tcp),
                Tbl::Ip => (&gen::NAMES_ip, &gen::LINUX_ip, &gen::WINDOWS_ip, &gen::XNU_ip),
                Tbl::Ipv6 => (&gen::NAMES_ipv6, &gen::LINUX_ipv6, &gen::WINDOWS_ipv6, &gen::XNU_ipv6),
            };
            Group { kind: Kind::Enum, names, linux, host: if xnu { x } else { w }, identity: false }
        })
    }
    fn with_host<R>(_h: &str, f: impl FnOnce() -> R) -> R { f() }

    #[test]
    fn linux_host_is_identity() {
        with_host("linux", || { let h = "linux";
            for (l, n) in [(0, 10), (41, 23), (1, 33), (17, 103), (41, 49)] {
                assert_eq!(on(h, l, n), Some(HostOpt { level: l, name: n, val: Val::Same }));
            }
        });
    }

    #[test]
    fn xnu_pmtu_and_pktinfo() {
        with_host("xnu", || { let h = "xnu";
            assert_eq!(on(h, 0, 10), Some(HostOpt { level: 0, name: 28, val: Val::PmtuDontfrag }));
            assert_eq!(on(h, 41, 23), Some(HostOpt { level: 41, name: 62, val: Val::PmtuDontfrag }));
            assert_eq!(on(h, 41, 49).map(|o| o.name), Some(61));
            assert_eq!(on(h, 41, 66).map(|o| o.name), Some(35)); // IPV6_RECVTCLASS from the table
            assert_eq!(on(h, 41, 67).map(|o| o.name), Some(36)); // IPV6_TCLASS
            assert_eq!(on(h, 41, 26).map(|o| o.name), Some(27)); // IPV6_V6ONLY
            assert_eq!(val_to_host(Val::PmtuDontfrag, 3), Some(1));
            assert_eq!(val_to_host(Val::PmtuDontfrag, 1), None);
        });
    }

    #[test]
    fn absent_and_unknown_are_refused_off_linux() {
        for h in ["xnu", "windows", "freebsd"] {
            with_host(h, || {
                assert_eq!(on(h, 1, 33), None, "{h} SO_RCVBUFFORCE");
                assert_eq!(on(h, 1, 32), None, "{h} SO_SNDBUFFORCE");
                assert_eq!(on(h, 1, 55), None, "{h} SO_MEMINFO");
                assert_eq!(on(h, 17, 103), None, "{h} UDP_SEGMENT");
                assert_eq!(on(h, 0, 9999), None, "{h} unknown IP option");
            });
        }
        assert_eq!(on("xnu", 0, 13), None, "XNU IP_RECVTOS is absent");
        assert_eq!(on("freebsd", 0, 10), None);
    }

    #[test]
    fn so_options_do_not_hit_sol_socket() {
        // SO_DEBUG is 1, the same number as SOL_SOCKET: the name must not be
        // translated as the level.
        with_host("xnu", || { let h = "xnu";
            let o = on(h, 1, 1).unwrap();
            assert_eq!(o.level, 0xffff);
            assert_eq!(o.name, 1);
            assert_eq!(on(h, 1, 8).unwrap().name, 0x1002); // SO_RCVBUF
            assert_eq!(on(h, 1, 15).unwrap().name, 512);   // SO_REUSEPORT
        });
    }

    #[test]
    fn windows_pmtu_and_ancillary() {
        with_host("windows", || { let h = "windows";
            assert_eq!(on(h, 0, 10), Some(HostOpt { level: 0, name: 71, val: Val::PmtuWindows }));
            assert_eq!(on(h, 41, 23), Some(HostOpt { level: 41, name: 71, val: Val::PmtuWindows }));
            assert_eq!(val_to_host(Val::PmtuWindows, 3), Some(3));
            assert_eq!(val_to_host(Val::PmtuWindows, 2), Some(1));
            assert_eq!(val_to_host(Val::PmtuWindows, 0), Some(2));
            assert_eq!(val_to_linux(Val::PmtuWindows, 3), 3);
            for (l, n) in [(41, 49), (41, 66), (0, 13), (0, 8)] { assert_eq!(on(h, l, n), None); }
        });
    }

    #[repr(C, align(8))]
    struct B([u8; 256]);

    fn linux_cmsg(buf: &mut [u8], off: usize, level: i32, ty: i32, data: &[u8]) -> usize {
        buf[off..off + 8].copy_from_slice(&((16 + data.len()) as u64).to_ne_bytes());
        buf[off + 8..off + 12].copy_from_slice(&level.to_ne_bytes());
        buf[off + 12..off + 16].copy_from_slice(&ty.to_ne_bytes());
        buf[off + 16..off + 16 + data.len()].copy_from_slice(data);
        off + linux_align(16 + data.len())
    }

    #[test]
    fn cmsg_round_trip_through_xnu_layout() {
        let mut src = B([0; 256]);
        let tclass = 0x2e_i32.to_ne_bytes();
        let pktinfo: [u8; 20] = core::array::from_fn(|i| i as u8 + 1);
        let mut n = linux_cmsg(&mut src.0, 0, 41, 67, &tclass);
        n = linux_cmsg(&mut src.0, n, 41, 50, &pktinfo);
        let mut bsd = B([0xaa; 256]);
        let bl = unsafe { linux_to_xnu(src.0.as_ptr(), n, bsd.0.as_mut_ptr(), 256) }.unwrap();
        assert_eq!(bl, (12 + 4) + (12 + 20));
        assert_eq!(u32::from_ne_bytes(bsd.0[0..4].try_into().unwrap()), 16);
        assert_eq!(i32::from_ne_bytes(bsd.0[8..12].try_into().unwrap()), 36);
        assert_eq!(i32::from_ne_bytes(bsd.0[16 + 8..16 + 12].try_into().unwrap()), 46);
        let mut back = B([0xaa; 256]);
        let (ll, trunc) = unsafe { xnu_to_linux(bsd.0.as_ptr(), bl, back.0.as_mut_ptr(), 256, |_| {}) };
        assert!(!trunc);
        assert_eq!(ll, n);
        assert_eq!(&back.0[..n], &src.0[..n], "full 8-byte cmsg_len, zeroed padding");
    }

    #[test]
    fn cmsg_untranslatable() {
        let mut src = B([0; 256]);
        let n = linux_cmsg(&mut src.0, 0, 0, 1, &1i32.to_ne_bytes()); // IP_TOS
        let mut bsd = B([0; 256]);
        assert_eq!(unsafe { linux_to_xnu(src.0.as_ptr(), n, bsd.0.as_mut_ptr(), 256) }, None);
        // An XNU cmsg Linux has no name for (IP_RECVDSTADDR) is dropped as truncation,
        // and the ones around it survive.
        let mut x = [0u8; 64];
        x[0..4].copy_from_slice(&16u32.to_ne_bytes()); x[4..8].copy_from_slice(&0i32.to_ne_bytes()); x[8..12].copy_from_slice(&7i32.to_ne_bytes());
        x[16..20].copy_from_slice(&16u32.to_ne_bytes()); x[20..24].copy_from_slice(&41i32.to_ne_bytes()); x[24..28].copy_from_slice(&36i32.to_ne_bytes());
        x[28..32].copy_from_slice(&5i32.to_ne_bytes());
        let mut out = B([0; 256]);
        let (ll, trunc) = unsafe { xnu_to_linux(x.as_ptr(), 32, out.0.as_mut_ptr(), 256, |_| {}) };
        assert!(trunc);
        assert_eq!(ll, 24);
        assert_eq!(i32::from_ne_bytes(out.0[12..16].try_into().unwrap()), 67);
        assert_eq!(i32::from_ne_bytes(out.0[16..20].try_into().unwrap()), 5);
        // Too small a Linux buffer: dropped, truncated.
        let (ll, trunc) = unsafe { xnu_to_linux(x.as_ptr() .add(16), 16, out.0.as_mut_ptr(), 16, |_| {}) };
        assert_eq!((ll, trunc), (0, true));
    }
    #[test]
    fn cmsg_bad_lengths_and_dropped_rights() {
        let mut dst = B([0; 256]);
        for len in [0u64, 15, 257, u64::MAX] {
            let mut src = B([0; 256]);
            src.0[..8].copy_from_slice(&len.to_ne_bytes());
            assert_eq!(unsafe { linux_to_xnu(src.0.as_ptr(), 256, dst.0.as_mut_ptr(), 256) }, None);
        }
        assert_eq!(unsafe { linux_to_xnu(core::ptr::null(), 16, dst.0.as_mut_ptr(), 256) }, None);
        assert_eq!(unsafe { linux_to_xnu(dst.0.as_ptr(), 1, dst.0.as_mut_ptr(), 256) }, None);
        let mut rights = [0u8; 16];
        rights[..4].copy_from_slice(&16u32.to_ne_bytes());
        rights[4..8].copy_from_slice(&0xffffu32.to_ne_bytes());
        rights[8..12].copy_from_slice(&1u32.to_ne_bytes());
        rights[12..16].copy_from_slice(&123i32.to_ne_bytes());
        let mut dropped = 0;
        assert_eq!(unsafe { xnu_to_linux(rights.as_ptr(), 16, dst.0.as_mut_ptr(), 16, |fd| dropped = fd) }, (0, true));
        assert_eq!(dropped, 123);
    }

    #[test]
    fn initializing_table_fails_closed() {
        assert_eq!(sockopt_on(HOST_XNU, IPPROTO_IP, 1, |_| Group {
            kind: Kind::Enum, names: &gen::NAMES_ip, linux: &gen::LINUX_ip,
            host: &gen::LINUX_ip, identity: true,
        }), None);
    }

}
