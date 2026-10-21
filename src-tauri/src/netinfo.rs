//! Cross-platform LAN helpers for Agent API (Mac / Win / Linux).

use std::net::{SocketAddr, UdpSocket};

/// Prefer a routable IPv4 for Hermes bots on the office LAN.
/// Uses a UDP connect trick (no packets need to leave) so it works without
/// enumerating interfaces — same behavior on macOS, Windows, and Linux.
pub fn primary_lan_ipv4() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Destination need not be reachable; OS only picks the egress iface.
    sock.connect("8.8.8.8:80").ok()?;
    match sock.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_unspecified() => {
            Some(v4.to_string())
        }
        _ => None,
    }
}

pub fn agent_api_urls(port: u16) -> (String, String, Option<String>) {
    let loopback = format!("http://127.0.0.1:{port}");
    let lan_ip = primary_lan_ipv4();
    let lan = lan_ip
        .as_ref()
        .map(|ip| format!("http://{ip}:{port}"))
        .unwrap_or_else(|| loopback.clone());
    (lan, loopback, lan_ip)
}

/// Constant-time-ish compare for Bearer tokens (fixed-length `dc_` + uuid).
pub fn token_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn lan_bind_addr(port: u16) -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_eq_ok() {
        assert!(token_eq("dc_abc", "dc_abc"));
        assert!(!token_eq("dc_abc", "dc_abd"));
        assert!(!token_eq("dc_abc", "dc_ab"));
    }

    #[test]
    fn bind_is_all_interfaces() {
        let a = lan_bind_addr(8765);
        assert_eq!(a.ip().to_string(), "0.0.0.0");
        assert_eq!(a.port(), 8765);
    }
}
