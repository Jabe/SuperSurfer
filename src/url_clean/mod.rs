use regex::Regex;
use std::sync::OnceLock;
use url::Url;

static UTM_RE: OnceLock<Regex> = OnceLock::new();
static TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "utm_id",
    "fbclid",
    "gclid",
    "mc_cid",
    "mc_eid",
    "msclkid",
];

/// Host suffix → query params to try in order.
const HOST_SUFFIX_RULES: &[(&str, &[&str])] = &[
    ("safelinks.protection.outlook.com", &["url"]),
    ("slack-redir.net", &["url"]),
    (".check.trendmicro.com", &["url"]),
    ("linkprotect.cudasvc.com", &["a"]),
    (".protection.sophos.com", &["d"]),
    ("redirect-url.email", &["link"]),
];

const MAX_UNWRAP_DEPTH: usize = 8;

pub fn clean_url(url: &mut Url, mode: &str) -> anyhow::Result<()> {
    if url.scheme() == "file" {
        return Ok(());
    }
    match mode {
        "off" => return Ok(()),
        _ => {
            unwrap_redirects(url)?;
            strip_tracking_params(url);
        }
    }
    Ok(())
}

fn unwrap_redirects(url: &mut Url) -> anyhow::Result<()> {
    for _ in 0..MAX_UNWRAP_DEPTH {
        let before = url.to_string();
        if !unwrap_once(url) {
            break;
        }
        if url.to_string() == before {
            break;
        }
    }
    Ok(())
}

fn unwrap_once(url: &mut Url) -> bool {
    if is_teams_safelinks(url) {
        return unwrap_query_param(url, "url");
    }

    if is_facebook_linkshim(url) {
        return unwrap_query_params(url, &["u"]);
    }

    if is_linkedin_safety(url) {
        return unwrap_query_param(url, "url");
    }

    if is_google_redirect(url) {
        return unwrap_query_params(url, &["q", "url"]);
    }

    let host = url.host_str().unwrap_or_default().to_lowercase();
    for (suffix, params) in HOST_SUFFIX_RULES {
        if host.ends_with(suffix) {
            if *suffix == ".protection.sophos.com" {
                return unwrap_sophos(url);
            }
            return unwrap_query_params(url, params);
        }
    }

    false
}

fn is_teams_safelinks(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_lowercase();
    let path = url.path().to_lowercase();
    let teams_host =
        host.ends_with(".teams.cdn.office.net") || host == "teams.public.onecdn.static.microsoft";
    let safelink_path = path.contains("/safelinks/") || path.contains("atp-safelinks");
    teams_host && safelink_path
}

fn is_facebook_linkshim(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_lowercase();
    let path = url.path();
    matches!(
        host.as_str(),
        "l.facebook.com"
            | "lm.facebook.com"
            | "m.facebook.com"
            | "www.facebook.com"
            | "facebook.com"
    ) && path.starts_with("/l.php")
}

fn is_linkedin_safety(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_lowercase();
    matches!(host.as_str(), "www.linkedin.com" | "linkedin.com")
        && url.path().starts_with("/safety/go")
}

fn is_google_redirect(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_lowercase();
    matches!(host.as_str(), "www.google.com" | "google.com") && url.path() == "/url"
}

fn unwrap_sophos(url: &mut Url) -> bool {
    let Some(dest) = query_param_value(url, "d") else {
        return false;
    };
    for candidate in [
        dest.clone(),
        format!("https://{dest}"),
        format!("http://{dest}"),
    ] {
        if let Ok(parsed) = Url::parse(&candidate) {
            if !is_web_scheme(&parsed) {
                continue;
            }
            *url = parsed;
            return true;
        }
    }
    false
}

fn unwrap_query_params(url: &mut Url, params: &[&str]) -> bool {
    for param in params {
        if unwrap_query_param(url, param) {
            return true;
        }
    }
    false
}

fn unwrap_query_param(url: &mut Url, param: &str) -> bool {
    let Some(dest) = query_param_value(url, param) else {
        return false;
    };
    match Url::parse(&dest) {
        Ok(parsed) if is_web_scheme(&parsed) => {
            *url = parsed;
            true
        }
        _ => false,
    }
}

/// Only unwrap to web destinations. Wrapper hosts are attacker-controllable via
/// email/chat, so we must never rewrite a "safe" link into a `file:`, `javascript:`,
/// or other local/dangerous scheme that the browser would then open.
fn is_web_scheme(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

fn query_param_value(url: &Url, param: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == param)
        .map(|(_, v)| v.into_owned())
}

fn strip_tracking_params(url: &mut Url) {
    let Some(query) = url.query() else {
        return;
    };
    let utm = UTM_RE.get_or_init(|| Regex::new(r"^utm_").unwrap());

    // Operate on the raw `&`-separated segments so the encoding of kept params is
    // preserved byte-for-byte. Tracking parameter names are plain ASCII, so the raw
    // key (before `=`) is sufficient to identify them. Rebuilding the query via
    // form-encoding would corrupt signature-sensitive URLs (pre-signed S3, OAuth,
    // HMAC params): `?q=a%20b` -> `?q=a+b`, `?flag` -> `?flag=`.
    let mut kept: Vec<&str> = Vec::new();
    let mut removed = false;
    for segment in query.split('&') {
        let key = segment.split('=').next().unwrap_or(segment);
        if TRACKING_PARAMS.contains(&key) || utm.is_match(key) {
            removed = true;
        } else {
            kept.push(segment);
        }
    }

    if !removed {
        return;
    }
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.set_query(Some(&kept.join("&")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unwrap(input: &str) -> String {
        let mut url = Url::parse(input).unwrap();
        unwrap_redirects(&mut url).unwrap();
        url.to_string()
    }

    #[test]
    fn strips_utm_params() {
        let mut url = Url::parse("https://example.com/page?utm_source=x&ok=1").unwrap();
        strip_tracking_params(&mut url);
        assert_eq!(url.as_str(), "https://example.com/page?ok=1");
    }

    #[test]
    fn unwraps_google_redirect() {
        assert_eq!(
            unwrap("https://www.google.com/url?q=https%3A%2F%2Fexample.org%2F&sa=U"),
            "https://example.org/"
        );
    }

    #[test]
    fn unwraps_outlook_safelinks() {
        assert_eq!(
            unwrap("https://safelinks.protection.outlook.com/?url=https%3A%2F%2Fexample.org%2Fpage&data=1"),
            "https://example.org/page"
        );
    }

    #[test]
    fn unwraps_teams_onecdn_host() {
        assert_eq!(
            unwrap("https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html?url=https%3A%2F%2Fexample.com%2Fpath%2F&locale=en-gb"),
            "https://example.com/path/"
        );
    }

    #[test]
    fn unwraps_teams_cdn_safelinks() {
        assert_eq!(
            unwrap("https://statics.teams.cdn.office.net/foo/atp-safelinks/bar?url=https%3A%2F%2Fexample.com%2Fpage"),
            "https://example.com/page"
        );
    }

    #[test]
    fn unwraps_redirect_url_email() {
        assert_eq!(
            unwrap("https://redirect-url.email/?link=https%3A%2F%2Fexample.com%2Ftarget"),
            "https://example.com/target"
        );
    }

    #[test]
    fn unwraps_slack_redir() {
        assert_eq!(
            unwrap("https://slack-redir.net/link?url=https%3A%2F%2Fexample.com%2Fpath"),
            "https://example.com/path"
        );
    }

    #[test]
    fn unwraps_facebook_linkshim() {
        assert_eq!(
            unwrap("https://l.facebook.com/l.php?u=https%3A%2F%2Fexample.com%2F&h=abc&s=1"),
            "https://example.com/"
        );
    }

    #[test]
    fn unwraps_linkedin_safety() {
        assert_eq!(
            unwrap("https://www.linkedin.com/safety/go?url=https%3A%2F%2Fexample.com%2Fjob"),
            "https://example.com/job"
        );
    }

    #[test]
    fn unwraps_barracuda_link_protect() {
        assert_eq!(
            unwrap("https://linkprotect.cudasvc.com/url?a=https%3A%2F%2Fexample.com%2Fdocs"),
            "https://example.com/docs"
        );
    }

    #[test]
    fn unwraps_trend_micro() {
        assert_eq!(
            unwrap("https://ca-1234.check.trendmicro.com/?url=https%3A%2F%2Fexample.com%2F"),
            "https://example.com/"
        );
    }

    #[test]
    fn unwraps_sophos_protection() {
        assert_eq!(
            unwrap("https://eu01.safelinks.protection.sophos.com/?d=example.com%2Fpage"),
            "https://example.com/page"
        );
    }

    #[test]
    fn refuses_to_unwrap_to_file_scheme() {
        // A weaponized "safe" link must not be rewritten into a local file:// URL.
        let wrapped = "https://safelinks.protection.outlook.com/?url=file%3A%2F%2F%2Fetc%2Fpasswd";
        assert_eq!(unwrap(wrapped), wrapped);
    }

    #[test]
    fn refuses_to_unwrap_sophos_to_file_scheme() {
        // Whatever Sophos produces, it must never be a local file:// URL.
        let wrapped =
            "https://eu01.safelinks.protection.sophos.com/?d=file%3A%2F%2F%2Fetc%2Fpasswd";
        let out = Url::parse(&unwrap(wrapped)).unwrap();
        assert_ne!(out.scheme(), "file");
    }

    #[test]
    fn preserves_query_encoding_when_nothing_stripped() {
        let mut url = Url::parse("https://example.com/page?q=a%20b&flag&sig=AbC%2Fd").unwrap();
        strip_tracking_params(&mut url);
        assert_eq!(
            url.as_str(),
            "https://example.com/page?q=a%20b&flag&sig=AbC%2Fd"
        );
    }

    #[test]
    fn preserves_kept_param_encoding_when_stripping() {
        let mut url =
            Url::parse("https://example.com/page?utm_source=x&sig=AbC%2Fd&q=a%20b").unwrap();
        strip_tracking_params(&mut url);
        assert_eq!(url.as_str(), "https://example.com/page?sig=AbC%2Fd&q=a%20b");
    }

    #[test]
    fn chains_nested_wrappers() {
        let wrapped = "https://safelinks.protection.outlook.com/?url=https%3A%2F%2Fslack-redir.net%2Flink%3Furl%3Dhttps%253A%252F%252Fexample.com";
        assert_eq!(unwrap(wrapped), "https://example.com/");
    }
}
