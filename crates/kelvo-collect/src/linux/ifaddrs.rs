//! An interface's IP addresses from `getifaddrs`, and the interface internet traffic
//! leaves through, for the Network page's address line. Read on request, never per tick.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket};

use crate::IfaceAddrs;

/// Every (interface, address) pair `getifaddrs` lists, IPv4 and IPv6 only. Empty when
/// the call fails.
fn all_addresses() -> Vec<(String, IpAddr)> {
    let mut out = Vec::new();
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
        // SAFETY: `ifa_addr` is non-null and points at a sockaddr owned by the list.
        let family = i32::from(unsafe { (*ifa.ifa_addr).sa_family });
        let addr = match family {
            libc::AF_INET => {
                // SAFETY: the family says the sockaddr is a `sockaddr_in`.
                let sin = unsafe { &*ifa.ifa_addr.cast::<libc::sockaddr_in>() };
                IpAddr::V4(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)))
            }
            libc::AF_INET6 => {
                // SAFETY: the family says the sockaddr is a `sockaddr_in6`.
                let sin6 = unsafe { &*ifa.ifa_addr.cast::<libc::sockaddr_in6>() };
                IpAddr::V6(Ipv6Addr::from(sin6.sin6_addr.s6_addr))
            }
            _ => continue,
        };
        out.push((name.to_string_lossy().into_owned(), addr));
    }
    // SAFETY: `head` came from a successful `getifaddrs` and is freed once.
    unsafe { libc::freeifaddrs(head) };
    out
}

/// The addresses assigned to `iface`, IPv6 link-local ones (`fe80::/10`) left out since
/// they only mean something on the local link.
pub fn interface_addresses(iface: &str) -> IfaceAddrs {
    let mut out = IfaceAddrs::default();
    for (name, addr) in all_addresses() {
        if name != iface {
            continue;
        }
        match addr {
            IpAddr::V4(a) => out.ipv4.push(a),
            IpAddr::V6(a) if !a.is_unicast_link_local() => out.ipv6.push(a),
            IpAddr::V6(_) => {}
        }
    }
    out
}

/// The interface the kernel routes IPv4 internet traffic through. Connecting a UDP
/// socket only runs the route lookup (policy routing included, so a Tailscale exit node
/// or a full-tunnel VPN shows as its tunnel); no packet is sent. The chosen source
/// address names the interface. `None` without a route.
pub fn egress_interface() -> Option<String> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    sock.connect((Ipv4Addr::new(1, 1, 1, 1), 53)).ok()?;
    let local = sock.local_addr().ok()?.ip();
    all_addresses()
        .into_iter()
        .find(|(_, a)| *a == local)
        .map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_has_its_address_and_an_unknown_interface_has_none() {
        let lo = interface_addresses("lo");
        assert!(lo.ipv4.contains(&Ipv4Addr::LOCALHOST), "{lo:?}");
        assert!(lo.ipv6.iter().all(|a| !a.is_unicast_link_local()));
        assert_eq!(interface_addresses("nope9"), IfaceAddrs::default());
    }

    #[test]
    fn egress_names_an_interface_holding_the_source_address() {
        // Containers without a default route have no egress; that is a valid answer.
        if let Some(iface) = egress_interface() {
            assert!(!interface_addresses(&iface).ipv4.is_empty(), "{iface}");
        }
    }
}
