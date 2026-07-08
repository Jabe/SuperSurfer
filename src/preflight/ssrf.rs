use anyhow::{bail, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use url::Url;

pub fn ensure_request_target(url: &Url) -> Result<()> {
    if url.scheme() != "https" {
        bail!("preflight only supports https URLs");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL missing host"))?;
    ensure_public_host(host)
}

pub fn ensure_redirect_target(url: &Url) -> Result<()> {
    if url.scheme() != "https" {
        bail!("preflight redirect target must use https");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("redirect URL missing host"))?;
    ensure_public_host(host)
}

pub fn ensure_public_host(host: &str) -> Result<()> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if is_blocked_hostname(&host) {
        bail!("blocked preflight host: {host}");
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            bail!("blocked preflight address: {ip}");
        }
        return Ok(());
    }

    let addrs = (host.as_str(), 443)
        .to_socket_addrs()
        .map_err(|err| anyhow::anyhow!("failed to resolve {host}: {err}"))?;
    let mut saw_addr = false;
    for addr in addrs {
        saw_addr = true;
        if is_blocked_ip(addr.ip()) {
            bail!("blocked preflight address for {host}: {}", addr.ip());
        }
    }
    if !saw_addr {
        bail!("no addresses resolved for {host}");
    }
    Ok(())
}

fn is_blocked_hostname(host: &str) -> bool {
    matches!(
        host,
        "localhost" | "localhost.localdomain" | "0.0.0.0" | "[::]" | "::1"
    ) || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".localhost")
        || host == "metadata.google.internal"
        || host == "metadata.goog"
}

/// True for loopback, private, link-local, CGNAT, ULA, and other non-public ranges.
/// Used by both static host checks and the preflight DNS resolver pin.
pub(crate) fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_ipv4(v4),
        IpAddr::V6(v6) => is_blocked_ipv6(v6),
    }
}

fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.octets()[0] == 0
        // RFC 6598 CGNAT: 100.64.0.0/10
        || {
            let o = ip.octets();
            o[0] == 100 && (64..128).contains(&o[1])
        }
}

fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    // IPv4-mapped (::ffff:x.x.x.x) and deprecated IPv4-compatible (::x.x.x.x)
    // must inherit the IPv4 blocklist (loopback, private, CGNAT, metadata, …).
    if let Some(v4) = ip.to_ipv4() {
        return is_blocked_ipv4(v4);
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.segments()[0] & 0xfe00 == 0xfc00 // unique local
        || ip.segments()[0] & 0xffc0 == 0xfe80 // link-local
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost_hostname() {
        assert!(ensure_public_host("localhost").is_err());
    }

    #[test]
    fn blocks_loopback_ip() {
        assert!(ensure_public_host("127.0.0.1").is_err());
    }

    #[test]
    fn blocks_private_ip_literal() {
        assert!(ensure_public_host("192.168.1.1").is_err());
    }

    #[test]
    fn allows_public_hostname() {
        assert!(ensure_public_host("example.com").is_ok());
    }

    #[test]
    fn blocks_full_cgnat_range() {
        assert!(ensure_public_host("100.64.0.1").is_err());
        assert!(ensure_public_host("100.65.0.1").is_err());
        assert!(ensure_public_host("100.127.255.255").is_err());
        assert!(is_blocked_ipv4(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_blocked_ipv4(Ipv4Addr::new(100, 127, 1, 1)));
        assert!(!is_blocked_ipv4(Ipv4Addr::new(100, 63, 0, 1)));
        assert!(!is_blocked_ipv4(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_blocked_ipv4(Ipv4Addr::new(8, 8, 8, 8)));
    }

    #[test]
    fn blocks_ipv4_mapped_loopback_and_private() {
        assert!(ensure_public_host("::ffff:127.0.0.1").is_err());
        assert!(ensure_public_host("::ffff:192.168.1.1").is_err());
        assert!(ensure_public_host("::ffff:169.254.169.254").is_err());
        assert!(ensure_public_host("::ffff:100.64.1.1").is_err());
        assert!(is_blocked_ip(IpAddr::V6(
            "::ffff:127.0.0.1".parse::<Ipv6Addr>().unwrap()
        )));
    }
}
