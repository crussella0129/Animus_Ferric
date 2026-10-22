//! Where a Ferric service may listen.
//!
//! Loopback always. Otherwise only the host address of a Ferric tunnel, and
//! only when that address is private (RFC 1918, CGNAT, or IPv6 ULA). A
//! wildcard (`0.0.0.0`, `::`) or a public address is refused outright: the
//! service would be reachable from the LAN or the internet with no
//! encryption and no peer authentication.

use std::net::IpAddr;

/// Check a listen address. `tunnel` is the host address from the tunnel
/// plan, when one exists.
pub fn check_bind(ip: IpAddr, tunnel: Option<IpAddr>) -> Result<(), String> {
    if ip.is_loopback() {
        return Ok(());
    }
    if ip.is_unspecified() {
        return Err(format!(
            "refusing to listen on {ip}: a wildcard address exposes the service on every interface"
        ));
    }
    match tunnel {
        Some(tunnel_ip) if tunnel_ip == ip && is_private(ip) => Ok(()),
        Some(tunnel_ip) if tunnel_ip == ip => Err(format!(
            "refusing to listen on {ip}: tunnel addresses must be private (10/8, 172.16/12, 192.168/16, 100.64/10, fd00::/8)"
        )),
        _ => Err(format!(
            "refusing to listen on {ip}: Ferric services bind loopback, or the host address of a \
             Ferric tunnel (`ferric tunnel init`)"
        )),
    }
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_private() || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn loopback_is_always_allowed() {
        assert!(check_bind(ip("127.0.0.1"), None).is_ok());
        assert!(check_bind(ip("::1"), None).is_ok());
    }

    #[test]
    fn wildcards_and_public_addresses_are_refused() {
        assert!(check_bind(ip("0.0.0.0"), Some(ip("0.0.0.0"))).is_err());
        assert!(check_bind(ip("::"), None).is_err());
        assert!(check_bind(ip("192.0.2.10"), None).is_err());
        assert!(check_bind(ip("10.77.0.1"), None).is_err());
    }

    #[test]
    fn only_the_private_tunnel_address_is_allowed() {
        assert!(check_bind(ip("10.77.0.1"), Some(ip("10.77.0.1"))).is_ok());
        assert!(check_bind(ip("100.64.0.9"), Some(ip("100.64.0.9"))).is_ok());
        assert!(check_bind(ip("fd77::1"), Some(ip("fd77::1"))).is_ok());
        assert!(check_bind(ip("10.77.0.2"), Some(ip("10.77.0.1"))).is_err());
        assert!(check_bind(ip("192.0.2.10"), Some(ip("192.0.2.10"))).is_err());
    }
}
