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
        "default" | _ => {
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
    let path = url.path();
    (host == "statics.teams.cdn.office.net" || host == "teams.public.onecdn.static.microsoft")
        && path.contains("atp-safelinks")
}

fn is_facebook_linkshim(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_lowercase();
    let path = url.path();
    matches!(
        host.as_str(),
        "l.facebook.com" | "lm.facebook.com" | "m.facebook.com" | "www.facebook.com" | "facebook.com"
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
    for candidate in [dest.clone(), format!("https://{dest}"), format!("http://{dest}")] {
        if let Ok(parsed) = Url::parse(&candidate) {
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
    if let Ok(parsed) = Url::parse(&dest) {
        *url = parsed;
        true
    } else {
        false
    }
}

fn query_param_value(url: &Url, param: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == param)
        .map(|(_, v)| v.into_owned())
}

fn strip_tracking_params(url: &mut Url) {
    let utm = UTM_RE.get_or_init(|| Regex::new(r"^utm_").unwrap());
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| {
            !TRACKING_PARAMS.contains(&k.as_ref()) && !utm.is_match(k.as_ref())
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    url.set_query(None);
    if !pairs.is_empty() {
        let mut qp = url.query_pairs_mut();
        for (k, v) in pairs {
            qp.append_pair(&k, &v);
        }
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
            unwrap("https://www.google.com/url?q=https%3A%2F%2Fgithub.com%2F&sa=U"),
            "https://github.com/"
        );
    }

    #[test]
    fn unwraps_outlook_safelinks() {
        assert_eq!(
            unwrap("https://safelinks.protection.outlook.com/?url=https%3A%2F%2Fgitlab.realmjoin.com%2F&data=1"),
            "https://gitlab.realmjoin.com/"
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
            unwrap("https://redirect-url.email/?link=https%3A%2F%2Fkeepersecurity.com%2Fvault"),
            "https://keepersecurity.com/vault"
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
    fn chains_nested_wrappers() {
        let wrapped = "https://safelinks.protection.outlook.com/?url=https%3A%2F%2Fslack-redir.net%2Flink%3Furl%3Dhttps%253A%252F%252Fexample.com";
        assert_eq!(unwrap(wrapped), "https://example.com/");
    }
}
