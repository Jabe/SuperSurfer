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

pub fn clean_url(url: &mut Url, mode: &str) -> anyhow::Result<()> {
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
    let host = url.host_str().unwrap_or_default().to_lowercase();

    if host.ends_with("safelinks.protection.outlook.com") {
        if let Some(dest) = url
            .query_pairs()
            .find(|(k, _)| k == "url")
            .map(|(_, v)| v.into_owned())
        {
            if let Ok(parsed) = Url::parse(&dest) {
                *url = parsed;
            }
        }
    }

    if host == "www.google.com" || host == "google.com" {
        if url.path() == "/url" {
            if let Some(dest) = url
                .query_pairs()
                .find(|(k, _)| k == "q" || k == "url")
                .map(|(_, v)| v.into_owned())
            {
                if let Ok(parsed) = Url::parse(&dest) {
                    *url = parsed;
                }
            }
        }
    }

    Ok(())
}

fn strip_tracking_params(url: &mut Url) {
    let utm = UTM_RE.get_or_init(|| Regex::new(r"^utm_").unwrap());
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| {
            !TRACKING_PARAMS.contains(&k.as_ref())
                && !utm.is_match(k.as_ref())
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

    #[test]
    fn strips_utm_params() {
        let mut url = Url::parse("https://example.com/page?utm_source=x&ok=1").unwrap();
        strip_tracking_params(&mut url);
        assert_eq!(url.as_str(), "https://example.com/page?ok=1");
    }

    #[test]
    fn unwraps_google_redirect() {
        let mut url =
            Url::parse("https://www.google.com/url?q=https%3A%2F%2Fgithub.com%2F&sa=U").unwrap();
        unwrap_redirects(&mut url).unwrap();
        assert_eq!(url.as_str(), "https://github.com/");
    }
}
