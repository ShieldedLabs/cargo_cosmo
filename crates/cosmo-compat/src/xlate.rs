//! Constant translation between Linux numbering (what std was compiled with)
//! and the running host's numbering (what cosmopolitan resolved at load time).
//!
//! Two shapes cover every group: an ENUM (one value, looked up) and a MASK (a
//! set of bits, translated bit by bit). Values not described by the table pass
//! through unchanged, so a constant cosmo does not know about behaves exactly
//! as it would in a C program built with cosmocc.
//!
//! No heap, no std synchronization anywhere here: `--wrap` also routes
//! cosmopolitan's OWN startup calls (its first open/mmap) through the wrappers,
//! before std's runtime exists and before cosmo has filled its constant tables.
//! Until the canary (EINVAL, nonzero on every host) reads nonzero, every group
//! is the identity.

use core::sync::atomic::{AtomicU8, Ordering::*};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Enum, Mask }

#[derive(Clone, Copy)]
pub struct Group {
    pub kind: Kind,
    pub names: &'static [&'static str],
    pub linux: &'static [i64],
    pub host: &'static [i64],
    /// Host and Linux agree on every value (the common case on Windows, where
    /// cosmo keeps Linux numbering for most groups), or the tables are not ready
    /// yet: every translate is the identity.
    pub identity: bool,
}

#[cfg(not(test))]
unsafe extern "C" { static EINVAL: i32; }
#[inline]
fn tables_ready() -> bool {
    #[cfg(not(test))]
    { unsafe { EINVAL != 0 } }
    #[cfg(test)]
    { true }
}

/// Build a group over its lazily filled host table. The fill happens once, on
/// the first call after cosmo's constants are ready, with a 0 -> 1 -> 2 state
/// word: readers only touch the table at state 2, after which it is never
/// written again, so the shared borrow of the `static mut` is sound. A caller
/// that arrives while another thread is filling (state 1) gets the identity
/// for that one call rather than waiting — there is nothing to wait with.
pub fn group<const N: usize>(kind: Kind, names: &'static [&'static str], linux: &'static [i64],
                             state: &'static AtomicU8, host: *mut [i64; N], fill: fn() -> [i64; N]) -> Group {
    let identity_group = Group { kind, names, linux, host: linux, identity: true };
    if state.load(Acquire) != 2 {
        if !tables_ready() { return identity_group; }
        if state.compare_exchange(0, 1, AcqRel, Acquire).is_ok() {
            unsafe { *host = fill(); }
            state.store(2, Release);
        } else if state.load(Acquire) != 2 {
            return identity_group;
        }
    }
    let host: &'static [i64] = unsafe { &*host };
    let identity = linux.iter().zip(host).all(|(l, h)| l == h);
    Group { kind, names, linux, host, identity }
}

impl Group {
    /// A Linux-numbered value as the host wants it.
    pub fn to_host(&self, v: i64) -> i64 {
        if self.identity { return v; }
        match self.kind {
            Kind::Enum => match self.linux.iter().position(|&l| l == v) { Some(i) => self.host[i], None => v },
            Kind::Mask => mask(v, self.linux, self.host, false),
        }
    }

    /// A host-numbered value as std expects it. Host values of 0 or -1 mark a
    /// constant the host lacks and are never matched, except an exact 0 -> 0.
    pub fn to_linux(&self, v: i64) -> i64 {
        if self.identity { return v; }
        match self.kind {
            Kind::Enum => {
                if v == 0 { return 0; }
                match self.host.iter().position(|&h| h == v && h != -1) { Some(i) => self.linux[i], None => v }
            }
            Kind::Mask => mask(v, self.host, self.linux, true),
        }
    }

    pub fn name_of_linux(&self, v: i64) -> Option<&'static str> {
        self.linux.iter().position(|&l| l == v).map(|i| self.names[i])
    }
}

/// Bit-set translation from `from` numbering to `to` numbering. Multi-bit
/// constants are consumed before their component bits (O_TMPFILE contains
/// O_DIRECTORY's bit), by walking popcounts from high to low; bits the table
/// does not describe pass through. `skip_absent` ignores source entries of 0/-1
/// (a constant the host lacks) so they cannot match everything.
fn mask(v: i64, from: &[i64], to: &[i64], skip_absent: bool) -> i64 {
    let mut rest = v;
    let mut out = 0i64;
    for pc in (1..=64u32).rev() {
        for (i, &f) in from.iter().enumerate() {
            if f.count_ones() != pc { continue; }
            if skip_absent && f <= 0 { continue; }
            if f != 0 && rest & f == f { out |= to[i]; rest &= !f; }
        }
        if rest == 0 { break; }
    }
    out | rest
}

// ---- errno: three table groups, and priorities for shared host values -------------
// Several Linux errnos land on one host value (EAGAIN/EWOULDBLOCK both 10035 on
// Windows; ETIME/ETIMEDOUT both 1460). The reverse map must pick the one std
// matches on, so std's own vocabulary is consulted first.
const ERRNO_PREFERRED: &[&str] = &[
    "EINVAL", "EINTR", "ETIMEDOUT", "ENOSYS", "EBADF", "EAGAIN", "EPERM", "ENOMEM", "ENOENT",
    "EINPROGRESS", "EEXIST", "EXDEV", "ERANGE", "EPIPE", "ENOTTY", "ENOTSUP", "ENOTDIR",
    "ENOTCONN", "ENFILE", "ENAMETOOLONG", "EMFILE", "ELOOP", "ECONNRESET", "ECONNREFUSED",
    "ECONNABORTED", "ECHILD", "EADDRNOTAVAIL", "EADDRINUSE", "EACCES", "ETXTBSY", "ESTALE",
    "ESRCH", "ESPIPE", "EROFS", "ENOTEMPTY", "ENOSPC", "ENETUNREACH", "ENETDOWN", "EMLINK",
    "EISDIR", "EISCONN", "EIO", "EHOSTUNREACH", "EFBIG", "EFAULT", "EDQUOT", "EDEADLK",
    "EBUSY", "E2BIG", "EDOM",
];

/// Host errno -> Linux errno.
pub fn errno_to_linux(host: i64) -> i64 {
    if host == 0 { return 0; }
    let g = crate::gen::errno();
    if g.identity { return host; }
    for want in ERRNO_PREFERRED {
        if let Some(i) = g.names.iter().position(|n| n == want) {
            if g.host[i] == host { return g.linux[i]; }
        }
    }
    let l = g.to_linux(host);
    if l != host { return l; }
    let l = crate::gen::compat().to_linux(host);
    if l != host { return l; }
    crate::gen::junkerr().to_linux(host)
}

/// Linux errno -> host errno (for the few places std passes one in).
pub fn errno_to_host(linux: i64) -> i64 {
    let h = crate::gen::errno().to_host(linux);
    if h != linux { return h; }
    crate::gen::compat().to_host(linux)
}

/// The test harness's stand-in for cosmo's load-time symbols: the Windows
/// column by default, the XNU column with COSMO_TEST_HOST=xnu.
#[cfg(test)]
pub fn test_host<const N: usize>(windows: &[i64; N], xnu: &[i64; N]) -> [i64; N] {
    if std::env::var("COSMO_TEST_HOST").map(|v| v == "xnu").unwrap_or(false) { *xnu } else { *windows }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen;

    fn w(g: &Group, name: &str) -> i64 { g.host[g.names.iter().position(|n| *n == name).unwrap()] }
    fn l(g: &Group, name: &str) -> i64 { g.linux[g.names.iter().position(|n| *n == name).unwrap()] }

    #[test]
    fn errno_round_trips_the_host_column() {
        let g = gen::errno();
        for (i, name) in g.names.iter().enumerate() {
            let host = g.host[i];
            if host <= 0 { continue; }
            let back = errno_to_linux(host);
            assert!(back == g.linux[i] || g.host.iter().zip(g.linux).any(|(h, l)| *h == host && *l == back),
                    "{name}: host {host} -> {back}, expected {}", g.linux[i]);
        }
        assert_eq!(errno_to_linux(0), 0);
        assert_eq!(errno_to_linux(w(&g, "EAGAIN")), l(&g, "EAGAIN"));
        assert_eq!(errno_to_linux(w(&g, "ETIMEDOUT")), l(&g, "ETIMEDOUT"));
        assert_eq!(errno_to_linux(w(&g, "EINVAL")), 22);
        assert_eq!(errno_to_linux(w(&g, "ENOENT")), 2);
        assert_eq!(errno_to_host(11), w(&g, "EAGAIN"));
    }

    #[test]
    fn open_flags_translate_bitwise() {
        let g = gen::open();
        let got = g.to_host(l(&g, "O_CREAT") | l(&g, "O_TRUNC") | l(&g, "O_CLOEXEC") | 2 /* O_RDWR: not in the table */);
        assert_eq!(got, w(&g, "O_CREAT") | w(&g, "O_TRUNC") | w(&g, "O_CLOEXEC") | 2);
        assert_eq!(g.to_linux(g.to_host(l(&g, "O_APPEND") | l(&g, "O_NONBLOCK"))), l(&g, "O_APPEND") | l(&g, "O_NONBLOCK"));
    }

    #[test]
    fn poll_events_translate_both_ways() {
        let g = gen::poll();
        assert_eq!(g.to_host(l(&g, "POLLIN") | l(&g, "POLLOUT")), w(&g, "POLLIN") | w(&g, "POLLOUT"));
        assert_eq!(g.to_linux(w(&g, "POLLHUP") | w(&g, "POLLIN")), l(&g, "POLLHUP") | l(&g, "POLLIN"));
    }

    #[test]
    fn socket_layer_enums() {
        let af = gen::af(); let so = gen::so(); let msg = gen::msg();
        assert_eq!(af.to_host(l(&af, "AF_INET6")), w(&af, "AF_INET6"));
        assert_eq!(af.to_linux(w(&af, "AF_INET6")), l(&af, "AF_INET6"));
        assert_eq!(af.to_host(2), 2);   // AF_INET is 2 everywhere
        assert_eq!(so.to_host(l(&so, "SOL_SOCKET")), w(&so, "SOL_SOCKET"));
        assert_eq!(so.to_host(l(&so, "SO_ERROR")), w(&so, "SO_ERROR"));
        assert_eq!(msg.to_host(l(&msg, "MSG_NOSIGNAL")), w(&msg, "MSG_NOSIGNAL"));
    }

    #[test]
    fn unknown_values_pass_through() {
        assert_eq!(gen::af().to_host(12345), 12345);
        assert_eq!(gen::open().to_host(0), 0);
        assert_eq!(errno_to_linux(999_999), 999_999);
    }
}
