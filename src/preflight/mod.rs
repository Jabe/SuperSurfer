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
use std::time::{Duration, Instant};
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
            logging::append_preflight(&format!(
                "ok {} -> {} ({})",
                url.as_str(),
                result.resolved.as_str(),
                format_lookup_duration(result.lookup_duration)
            ))?;
            Ok(Some(result.resolved))
        }
        Err(err) => {
            logging::append_preflight(&format!("skip {} ({err})", url.as_str()))?;
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

pub fn resolve(url: &Url) -> Result<PreflightResult> {
    ssrf::ensure_request_target(url)?;

    let started = Instant::now();
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(USER_AGENT)
        .max_redirects(0)
        .build()
        .new_agent();

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

    let resolved = Url::parse(location)
        .or_else(|_| base.join(location))
        .with_context(|| format!("preflight redirect Location is not a valid URL: {location}"))?;

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
    fn parse_redirect_target_rejects_http_location() {
        let base = Url::parse("https://redirect.example.com/r/abc").unwrap();
        assert!(parse_redirect_target(&base, 302, "http://example.com/").is_err());
    }

    #[test]
    fn parse_redirect_target_rejects_non_redirect_status() {
        let base = Url::parse("https://redirect.example.com/r/abc").unwrap();
        assert!(parse_redirect_target(&base, 200, "https://example.com/").is_err());
    }
}
