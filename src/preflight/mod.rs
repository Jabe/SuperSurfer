mod consent;
mod consent_guard;
mod ssrf;

pub use consent::{
    allow_host, consent_path, is_allowed, list_allowed_hosts, normalize_host, protection_status,
    revoke_host,
};
pub use consent_guard::ConsentProtection;

use crate::logging;
use anyhow::{Context as _, Result};
use std::fmt;
use std::time::{Duration, Instant};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};
use ureq::Agent;
use url::Url;

const USER_AGENT: &str = "SuperSurfer/0.1.0 (preflight)";
const TIMEOUT: Duration = Duration::from_secs(2);
const REDIRECT_STATUSES: &[u16] = &[301, 302, 303, 307, 308];

pub struct PreflightResult {
    pub resolved: Url,
    pub lookup_duration: Duration,
}

pub fn maybe_resolve(url: &Url, host_consented: bool, config_matched: bool) -> Result<Option<Url>> {
    if !config_matched || !host_consented {
        return Ok(None);
    }

    let host = url.host_str().unwrap_or_default();
    match resolve(url) {
        Ok(result) => {
            if let Err(err) = logging::append_preflight(&format!(
                "ok {} -> {} ({})",
                url.as_str(),
                result.resolved.as_str(),
                format_lookup_duration(result.lookup_duration)
            )) {
                eprintln!("warning: failed to append preflight log: {err}");
            }
            Ok(Some(result.resolved))
        }
        Err(err) => {
            if let Err(log_err) =
                logging::append_preflight(&format!("skip {} ({err})", url.as_str()))
            {
                eprintln!("warning: failed to append preflight log: {log_err}");
            }
            eprintln!("preflight skipped for {host}: {err}");
            Ok(None)
        }
    }
}

pub fn format_lookup_duration(duration: Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

/// DNS resolver that strips non-public IPs before ureq connects.
///
/// Closes the classic SSRF TOCTOU where a pre-check `to_socket_addrs` sees a
/// public A/AAAA record and a later connect-time resolve (or rebinding) yields
/// loopback/private/link-local. Filtering at resolve-for-connect time means
/// the TCP stack never receives a blocked address.
#[derive(Default)]
struct PublicOnlyResolver {
    inner: DefaultResolver,
}

impl fmt::Debug for PublicOnlyResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicOnlyResolver").finish()
    }
}

impl Resolver for PublicOnlyResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let resolved = self.inner.resolve(uri, config, timeout)?;
        let mut filtered = self.empty();
        for addr in resolved.iter() {
            if !ssrf::is_blocked_ip(addr.ip()) {
                filtered.push(*addr);
            }
        }
        if filtered.is_empty() {
            // Treat "only blocked addresses" like a failed lookup so callers
            // never connect to private/loopback targets.
            return Err(ureq::Error::HostNotFound);
        }
        Ok(filtered)
    }
}

fn preflight_agent() -> Agent {
    let config = Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(USER_AGENT)
        // Read the Location header ourselves rather than follow the chain. Every
        // further hop is a request the *destination* decides on, so following N of
        // them hands a hostile target N requests and up to N * TIMEOUT of latency
        // before the browser opens at all. Stopping after one keeps the worst case
        // fixed and knowable. Contrast `MAX_UNWRAP_DEPTH`, which may be generous
        // precisely because its layers cost no I/O and no remote say.
        .max_redirects(0)
        .build();
    Agent::with_parts(
        config,
        DefaultConnector::default(),
        PublicOnlyResolver::default(),
    )
}

pub fn resolve(url: &Url) -> Result<PreflightResult> {
    // Fast reject for obvious non-public literals / hostnames before any I/O.
    ssrf::ensure_request_target(url)?;

    let started = Instant::now();
    let agent = preflight_agent();

    let response = agent
        .head(url.as_str())
        .call()
        .with_context(|| format!("preflight HEAD failed for {}", url.as_str()))?;

    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("preflight redirect missing Location header"))?;

    let resolved = parse_redirect_target(url, response.status().as_u16(), location)?;
    Ok(PreflightResult {
        resolved,
        lookup_duration: started.elapsed(),
    })
}

fn parse_redirect_target(base: &Url, status: u16, location: &str) -> Result<Url> {
    if !REDIRECT_STATUSES.contains(&status) {
        anyhow::bail!("preflight expected redirect status, got {status}");
    }

    let mut resolved = Url::parse(location)
        .or_else(|_| base.join(location))
        .with_context(|| format!("preflight redirect Location is not a valid URL: {location}"))?;

    crate::input_url::normalize_host(&mut resolved);
    ssrf::ensure_redirect_target(&resolved)?;
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_redirect_target_accepts_https_location() {
        let base = Url::parse("https://redirect.example.com/r/abc").unwrap();
        let resolved =
            parse_redirect_target(&base, 301, "https://example.com/final").expect("redirect");
        assert_eq!(resolved.as_str(), "https://example.com/final");
    }

    #[test]
    fn parse_redirect_target_normalizes_root_dot() {
        let base = Url::parse("https://redirect.example.com/r/abc").unwrap();
        let resolved =
            parse_redirect_target(&base, 302, "https://example.com./final").expect("redirect");
        assert_eq!(resolved.as_str(), "https://example.com/final");
    }

    #[test]
    fn parse_redirect_target_rejects_http_location() {
        let base = Url::parse("https://redirect.example.com/r/abc").unwrap();
        assert!(parse_redirect_target(&base, 302, "http://example.com/").is_err());
    }

    #[test]
    fn parse_redirect_target_rejects_non_redirect_status() {
        let base = Url::parse("https://redirect.example.com/r/abc").unwrap();
        assert!(parse_redirect_target(&base, 200, "https://example.com/").is_err());
    }

    #[test]
    fn public_only_resolver_filters_blocked_ips() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        // Unit-level: is_blocked_ip is what the resolver applies to each addr.
        assert!(ssrf::is_blocked_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(ssrf::is_blocked_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(ssrf::is_blocked_ip(IpAddr::V4(Ipv4Addr::new(
            169, 254, 169, 254
        ))));
        assert!(!ssrf::is_blocked_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));

        let _ = SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 443));
        let _ = PublicOnlyResolver::default();
    }
}
