//! An interface's IP addresses from `getifaddrs`, for the Network page's address line.
//! Public API, no entitlement; read on request, never per tick.

use std::net::{Ipv4Addr, Ipv6Addr};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_has_its_address_and_an_unknown_interface_has_none() {
        let lo = interface_addresses("lo0");
        assert!(lo.ipv4.contains(&Ipv4Addr::LOCALHOST), "{lo:?}");
        assert!(lo.ipv6.iter().all(|a| !a.is_unicast_link_local()));
        assert_eq!(interface_addresses("nope9"), IfaceAddrs::default());
    }
}
