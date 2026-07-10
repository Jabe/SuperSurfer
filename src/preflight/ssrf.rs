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
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast() // 224.0.0.0/4
        || ip.is_documentation() // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24
        || o[0] == 0 // 0.0.0.0/8 "this network"
        || o[0] >= 240 // 240.0.0.0/4 reserved (includes broadcast)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 IETF protocol assignments
        || (o[0] == 198 && o[1] & 0xfe == 18) // 198.18.0.0/15 benchmarking
        // RFC 6598 CGNAT: 100.64.0.0/10
        || (o[0] == 100 && (64..128).contains(&o[1]))
}

fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    // IPv4-mapped (::ffff:x.x.x.x) and deprecated IPv4-compatible (::x.x.x.x)
    // must inherit the IPv4 blocklist (loopback, private, CGNAT, metadata, …).
    if let Some(v4) = ip.to_ipv4() {
        return is_blocked_ipv4(v4);
    }
    let seg = ip.segments();
    // NAT64 (64:ff9b::/32): the well-known /96 embeds an IPv4 target which must
    // inherit the IPv4 blocklist; everything else in the /32 (e.g. the local-use
    // 64:ff9b:1::/48) is not public.
    if seg[0] == 0x64 && seg[1] == 0xff9b {
        if seg[2..6] == [0, 0, 0, 0] {
            let [a, b] = seg[6].to_be_bytes();
            let [c, d] = seg[7].to_be_bytes();
            return is_blocked_ipv4(Ipv4Addr::new(a, b, c, d));
        }
        return true;
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast() // ff00::/8
        || seg[0] & 0xfe00 == 0xfc00 // fc00::/7 unique local
        || seg[0] & 0xffc0 == 0xfe80 // fe80::/10 link-local
        || seg[0] & 0xffc0 == 0xfec0 // fec0::/10 deprecated site-local
        // 2001::/23 IETF protocol assignments (Teredo, benchmarking, ORCHID, …)
        || (seg[0] == 0x2001 && seg[1] < 0x0200)
        || (seg[0] == 0x2001 && seg[1] == 0x0db8) // 2001:db8::/32 documentation
        || (seg[0] == 0x3fff && seg[1] & 0xf000 == 0) // 3fff::/20 documentation
        || seg[0] == 0x2002 // 2002::/16 deprecated 6to4 (embeds an IPv4 address)
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

    #[test]
    fn blocks_remaining_non_global_ipv4_ranges() {
        // 198.18.0.0/15 benchmarking
        assert!(is_blocked_ipv4(Ipv4Addr::new(198, 18, 0, 1)));
        assert!(is_blocked_ipv4(Ipv4Addr::new(198, 19, 255, 255)));
        assert!(!is_blocked_ipv4(Ipv4Addr::new(198, 17, 0, 1)));
        assert!(!is_blocked_ipv4(Ipv4Addr::new(198, 20, 0, 1)));
        // 240.0.0.0/4 reserved
        assert!(is_blocked_ipv4(Ipv4Addr::new(240, 0, 0, 1)));
        assert!(is_blocked_ipv4(Ipv4Addr::new(255, 255, 255, 254)));
        // 224.0.0.0/4 multicast
        assert!(is_blocked_ipv4(Ipv4Addr::new(224, 0, 0, 251)));
        assert!(is_blocked_ipv4(Ipv4Addr::new(239, 255, 255, 250)));
        // 192.0.0.0/24 protocol assignments; documentation nets
        assert!(is_blocked_ipv4(Ipv4Addr::new(192, 0, 0, 8)));
        assert!(is_blocked_ipv4(Ipv4Addr::new(192, 0, 2, 1)));
        assert!(is_blocked_ipv4(Ipv4Addr::new(198, 51, 100, 1)));
        assert!(is_blocked_ipv4(Ipv4Addr::new(203, 0, 113, 1)));
        // Nearby public space stays reachable.
        assert!(!is_blocked_ipv4(Ipv4Addr::new(223, 255, 255, 255)));
        assert!(!is_blocked_ipv4(Ipv4Addr::new(192, 0, 1, 1)));
    }

    #[test]
    fn blocks_remaining_non_global_ipv6_ranges() {
        let blocked = [
            "ff02::1",             // multicast
            "fec0::1",             // deprecated site-local
            "2001:db8::1",         // documentation
            "3fff::1",             // documentation
            "2001::1",             // Teredo (2001::/32)
            "2001:2::1",           // benchmarking (within 2001::/23)
            "2002:7f00:1::1",      // 6to4
            "64:ff9b::7f00:1",     // NAT64 embedding 127.0.0.1
            "64:ff9b::a00:1",      // NAT64 embedding 10.0.0.1
            "64:ff9b:1::c0a8:101", // local-use NAT64
        ];
        for addr in blocked {
            assert!(
                is_blocked_ip(IpAddr::V6(addr.parse::<Ipv6Addr>().unwrap())),
                "{addr} should be blocked"
            );
        }
        let allowed = [
            "2606:4700:4700::1111", // Cloudflare DNS — plainly public
            "2001:4860:4860::8888", // Google DNS — outside 2001::/23
            "64:ff9b::808:808",     // NAT64 embedding public 8.8.8.8
        ];
        for addr in allowed {
            assert!(
                !is_blocked_ip(IpAddr::V6(addr.parse::<Ipv6Addr>().unwrap())),
                "{addr} should be allowed"
            );
        }
    }
}
