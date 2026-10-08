//! The Network page's address line: the primary interface's addresses, read locally,
//! and the public address, which only a server outside the Mac can see (D-093).

use std::net::IpAddr;
use std::process::Command;
use std::time::Duration;

use crate::ipc::NetworkAddresses;

/// The service asked for the public address: it answers a GET with the caller's address
/// as plain text and keeps no account. HTTPS, so the answer cannot be swapped in transit.
const PUBLIC_IP_URL: &str = "https://api.ipify.org";

/// How long the lookup may take before it is given up.
const PUBLIC_IP_TIMEOUT: Duration = Duration::from_secs(5);

/// The addresses of `iface`, the interface carrying the default route.
pub fn network_addresses(iface: Option<&str>) -> NetworkAddresses {
    let addrs = iface
        .map(kelvo_engine::interface_addresses)
        .unwrap_or_default();
    NetworkAddresses {
        iface: iface.map(str::to_owned),
        ipv4: addrs.ipv4.iter().map(ToString::to_string).collect(),
        ipv6: addrs.ipv6.iter().map(ToString::to_string).collect(),
        egress: kelvo_engine::egress_interface(),
    }
}

/// The address the internet sees this Mac at, from [`PUBLIC_IP_URL`] through the
/// system's `/usr/bin/curl`, which keeps Kelvo free of an HTTP and TLS stack for one
/// request. Blocking; run it off the main thread. `Err` carries what went wrong.
pub fn public_ip() -> Result<IpAddr, String> {
    let out = Command::new("/usr/bin/curl")
        .args(["--silent", "--show-error", "--fail", "--max-time"])
        .arg(PUBLIC_IP_TIMEOUT.as_secs().to_string())
        .arg(PUBLIC_IP_URL)
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.trim().to_owned());
    }
    parse_ip(&out.stdout)
}

/// The response body as an address; anything else is an error, so a captive portal's
/// HTML page never shows as an address.
fn parse_ip(body: &[u8]) -> Result<IpAddr, String> {
    let text = std::str::from_utf8(body).map_err(|_| "response is not text".to_owned())?;
    text.trim()
        .parse()
        .map_err(|_| format!("response is not an address: {:.40}", text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bare_address_parses() {
        assert_eq!(
            parse_ip(b"203.0.113.7\n"),
            Ok("203.0.113.7".parse().unwrap())
        );
        assert_eq!(parse_ip(b"2001:db8::1"), Ok("2001:db8::1".parse().unwrap()));
        assert!(parse_ip(b"<html>Sign in to Wi-Fi</html>").is_err());
        assert!(parse_ip(b"").is_err());
    }

    #[test]
    fn no_primary_interface_reads_no_addresses() {
        let a = network_addresses(None);
        assert_eq!(a.iface, None);
        assert!(a.ipv4.is_empty() && a.ipv6.is_empty());
    }
}
