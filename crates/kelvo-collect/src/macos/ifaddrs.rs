//! An interface's IP addresses from `getifaddrs`, and the interface internet traffic
//! leaves through, for the Network page's address line. Public API, no entitlement; read
//! on request, never per tick.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicI32, Ordering};

use crate::IfaceAddrs;

/// The addresses assigned to `iface`, IPv6 link-local ones (`fe80::/10`) left out since
/// they only mean something on the local link. Empty when the interface has none or the
/// call fails.
pub fn interface_addresses(iface: &str) -> IfaceAddrs {
    let mut out = IfaceAddrs::default();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `head` is a valid out-pointer; on success the list is freed below.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return out;
    }
    let mut cur = head;
    while !cur.is_null() {
        // SAFETY: `cur` is a node of the list `getifaddrs` returned, not yet freed.
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        if ifa.ifa_name.is_null() || ifa.ifa_addr.is_null() {
            continue;
        }
        // SAFETY: `ifa_name` is a NUL-terminated C string owned by the list.
        let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) };
        if name.to_bytes() != iface.as_bytes() {
            continue;
        }
        // SAFETY: `ifa_addr` is non-null and points at a sockaddr owned by the list.
        let family = i32::from(unsafe { (*ifa.ifa_addr).sa_family });
        match family {
            libc::AF_INET => {
                // SAFETY: the family says the sockaddr is a `sockaddr_in`.
                let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
                out.ipv4
                    .push(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
            }
            libc::AF_INET6 => {
                // SAFETY: the family says the sockaddr is a `sockaddr_in6`.
                let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
                let addr = Ipv6Addr::from(sin6.sin6_addr.s6_addr);
                if !addr.is_unicast_link_local() {
                    out.ipv6.push(addr);
                }
            }
            _ => {}
        }
    }
    // SAFETY: `head` came from a successful `getifaddrs` and is freed once.
    unsafe { libc::freeifaddrs(head) };
    out
}

/// The interface the kernel routes IPv4 internet traffic through: what `route get`
/// reports for a public address. A full-tunnel VPN moves it to its `utun` while
/// SystemConfiguration's primary interface stays on Wi-Fi or Ethernet. A table lookup on
/// a routing socket; no packet is sent. `None` when there is no IPv4 route or the lookup
/// fails.
pub fn egress_interface() -> Option<String> {
    /// Any public address: only the routing table is consulted.
    const PUBLIC: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);
    /// Tells this process's concurrent lookups' replies apart.
    static SEQ: AtomicI32 = AtomicI32::new(1);

    #[repr(C)]
    struct Request {
        hdr: libc::rt_msghdr,
        dst: libc::sockaddr_in,
    }

    // SAFETY: plain socket(2); the descriptor is owned (and closed) by `fd` below.
    let raw = unsafe { libc::socket(libc::PF_ROUTE, libc::SOCK_RAW, libc::AF_INET) };
    if raw < 0 {
        return None;
    }
    // SAFETY: `raw` is a fresh descriptor nothing else owns.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // A reply that never comes must not hang the command.
    let timeout = libc::timeval {
        tv_sec: 1,
        tv_usec: 0,
    };
    // SAFETY: `timeout` is a valid timeval for the duration of the call.
    unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            std::ptr::from_ref(&timeout).cast(),
            size_of::<libc::timeval>() as libc::socklen_t,
        )
    };

    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() as libc::pid_t;
    // SAFETY: both structs are plain C data for which all-zero bytes are valid.
    let mut req: Request = unsafe { std::mem::zeroed() };
    req.hdr.rtm_msglen = size_of::<Request>() as u16;
    req.hdr.rtm_version = libc::RTM_VERSION as u8;
    req.hdr.rtm_type = libc::RTM_GET as u8;
    req.hdr.rtm_addrs = libc::RTA_DST;
    req.hdr.rtm_pid = pid;
    req.hdr.rtm_seq = seq;
    req.dst.sin_len = size_of::<libc::sockaddr_in>() as u8;
    req.dst.sin_family = libc::AF_INET as u8;
    req.dst.sin_addr.s_addr = u32::from(PUBLIC).to_be();
    // SAFETY: `req` is `rtm_msglen` readable bytes.
    let sent = unsafe {
        libc::write(
            fd.as_raw_fd(),
            std::ptr::from_ref(&req).cast(),
            size_of::<Request>(),
        )
    };
    if sent != size_of::<Request>() as isize {
        return None; // ESRCH: no route
    }

    // Every routing socket also hears the system's other routing messages; skip those.
    let mut buf = [0u8; 1024];
    for _ in 0..32 {
        // SAFETY: `buf` has `buf.len()` writable bytes.
        let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n < size_of::<libc::rt_msghdr>() as isize {
            return None; // timeout or error
        }
        // SAFETY: at least a header's worth of bytes was read; unaligned read of plain data.
        let hdr = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<libc::rt_msghdr>()) };
        if hdr.rtm_type == libc::RTM_GET as u8 && hdr.rtm_pid == pid && hdr.rtm_seq == seq {
            return (hdr.rtm_errno == 0)
                .then(|| interface_name(hdr.rtm_index))
                .flatten();
        }
    }
    None
}

/// The BSD name of interface `index`, `None` when there is no such interface.
fn interface_name(index: u16) -> Option<String> {
    let mut name = [0 as libc::c_char; libc::IF_NAMESIZE];
    // SAFETY: `name` has the IF_NAMESIZE bytes if_indextoname may write.
    let out = unsafe { libc::if_indextoname(u32::from(index), name.as_mut_ptr()) };
    if out.is_null() {
        return None;
    }
    // SAFETY: on success `name` holds a NUL-terminated string.
    let name = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) };
    name.to_str().ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egress_is_the_interface_route_get_reports() {
        let out = std::process::Command::new("/sbin/route")
            .args(["-n", "get", "1.1.1.1"])
            .output()
            .unwrap();
        let expected = String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| l.trim().strip_prefix("interface: ").map(str::to_owned));
        assert_eq!(egress_interface(), expected);
    }

    #[test]
    fn loopback_has_its_address_and_an_unknown_interface_has_none() {
        let lo = interface_addresses("lo0");
        assert!(lo.ipv4.contains(&Ipv4Addr::LOCALHOST), "{lo:?}");
        assert!(lo.ipv6.iter().all(|a| !a.is_unicast_link_local()));
        assert_eq!(interface_addresses("nope9"), IfaceAddrs::default());
    }
}
