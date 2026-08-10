use crate::browser::{launch::launch_browser, registry::BrowserRegistry};
use crate::config::loader::{load_default_config, LoadedConfig};
use crate::context::Context;
use crate::logging;
use crate::preflight;
use crate::script::runtime::BrowserTarget;
use crate::url_clean;
use anyhow::{Context as _, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use url::Url;

type ResolvedTarget = (
    String,
    String,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
    bool,
    bool,
);

#[derive(Debug, Clone)]
pub struct PreflightProbe {
    pub input_url: String,
    pub prepared_url: String,
    pub host: String,
    pub config_matched: bool,
    pub host_consented: bool,
    pub resolved_url: Option<String>,
    pub preflight_error: Option<String>,
    /// Wall time for the HTTP HEAD lookup when config and consent allowed it.
    pub lookup_duration: Option<Duration>,
    pub browser: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RouteDecision {
    pub input_url: String,
    /// URL the handlers matched against — decoded unless cleaning is `off`.
    pub routed_url: String,
    /// URL actually handed to the browser. Equal to `routed_url` except in
    /// `route` mode, where the wrapper is deliberately left intact.
    pub launch_url: String,
    pub browser_id: String,
    pub browser: String,
    pub profile: Option<String>,
    pub profile_directory: Option<String>,
    pub private: bool,
    pub matched_handler: bool,
    pub fallback: bool,
    pub app_path: Option<String>,
}

impl RouteDecision {
    /// One-line summary for the decision log and the launcher trace. The routed
    /// URL is named separately only when it differs from what was opened, so
    /// `route` mode stays debuggable without adding noise to the other modes.
    pub fn log_line(&self) -> String {
        if self.routed_url == self.launch_url {
            format!(
                "{} -> {} ({})",
                self.input_url, self.launch_url, self.browser
            )
        } else {
            format!(
                "{} -> {} [matched as {}] ({})",
                self.input_url, self.launch_url, self.routed_url, self.browser
            )
        }
    }
}

pub struct Router {
    config: LoadedConfig,
    registry: BrowserRegistry,
}

impl Router {
    pub fn new() -> Result<Self> {
        Ok(Self {
            config: load_default_config()?,
            registry: BrowserRegistry::discover()?,
        })
    }

    pub fn with_config_path(path: PathBuf) -> Result<Self> {
        Ok(Self {
            config: crate::config::loader::load_config(&path)?,
            registry: BrowserRegistry::discover_fresh()?,
        })
    }

    pub fn decide(&self, raw_url: &str, context: &Context) -> Result<RouteDecision> {
        let _process_guard = crate::process::RouteProcessGuard::new();
        let raw_url = crate::input_url::normalize_input_url(raw_url)?;
        let mut url = Url::parse(&raw_url).with_context(|| format!("invalid URL: {raw_url}"))?;
        let input_url = url.to_string();
        crate::input_url::normalize_host(&mut url);

        let cleaning_mode = self.config.runtime.url_cleaning_mode()?;
        let cleaned = url_clean::clean_url(&url, &cleaning_mode);

        let (target, routed_url, script_fallback) =
            match self.route_script(&cleaned.routed, context) {
                Ok(result) => result,
                Err(err) => {
                    eprintln!("routing script error: {err}. Falling back to defaultBrowser.");
                    (None, cleaned.routed.clone(), true)
                }
            };

        let launch_url = pick_launch_url(&cleaned, &routed_url);

        let target = match target {
            Some(t) if t.name.is_some() => t,
            _ => BrowserTarget {
                name: Some(self.config.runtime.default_browser()?),
                private: false,
            },
        };

        let (
            browser_id,
            browser,
            profile,
            profile_directory,
            private,
            app_path,
            matched_handler,
            mut fallback,
        ) = self.resolve_target(target)?;
        if script_fallback {
            fallback = true;
        }

        let decision = RouteDecision {
            input_url,
            routed_url: routed_url.to_string(),
            launch_url: launch_url.to_string(),
            browser_id,
            browser,
            profile,
            profile_directory,
            private,
            matched_handler,
            fallback,
            app_path,
        };

        // Never gate browser launch on log I/O (permissions, full disk, etc.).
        if let Err(err) = logging::append_decision(&decision.log_line()) {
            eprintln!("warning: failed to append decision log: {err}");
        }

        Ok(decision)
    }

    pub fn route_and_launch(
        &self,
        raw_url: &str,
        context: &Context,
        dry_run: bool,
    ) -> Result<RouteDecision> {
        let decision = self.decide(raw_url, context)?;
        if !dry_run {
            launch_browser(&self.registry, &decision)?;
        }
        Ok(decision)
    }

    pub fn registry(&self) -> &BrowserRegistry {
        &self.registry
    }

    pub fn config_path(&self) -> &PathBuf {
        &self.config.source_path
    }

    pub fn references_opener(&self) -> bool {
        self.config.references_opener
    }

    /// Dry-run preflight resolve: url cleaning, rewrite, HEAD probe, and post-resolve routing.
    pub fn probe_preflight(&self, raw_url: &str, context: &Context) -> Result<PreflightProbe> {
        let _process_guard = crate::process::RouteProcessGuard::new();
        let raw_url = crate::input_url::normalize_input_url(raw_url)?;
        let mut url = Url::parse(&raw_url).with_context(|| format!("invalid URL: {raw_url}"))?;
        let input_url = url.to_string();
        crate::input_url::normalize_host(&mut url);

        let cleaning_mode = self.config.runtime.url_cleaning_mode()?;
        let cleaned = url_clean::clean_url(&url, &cleaning_mode);

        let prepared = self.config.runtime.prepare_url(&cleaned.routed, context)?;
        let host = prepared.host_str().unwrap_or_default().to_string();
        let config_matched = self.config.runtime.should_resolve(&prepared, context)?;
        let host_consented = preflight::is_allowed(&host)?;

        let (resolved_url, preflight_error, lookup_duration) = if config_matched && host_consented {
            let started = Instant::now();
            match preflight::resolve(&prepared) {
                Ok(result) => (
                    Some(result.resolved.to_string()),
                    None,
                    Some(result.lookup_duration),
                ),
                Err(err) => (None, Some(err.to_string()), Some(started.elapsed())),
            }
        } else {
            (None, None, None)
        };

        let route_url = resolved_url
            .as_deref()
            .and_then(|s| Url::parse(s).ok())
            .unwrap_or(prepared.clone());

        let (browser, profile) = match self.config.runtime.match_handlers(&route_url, context) {
            Ok(Some(target)) => {
                let spec = target.name.unwrap_or_default();
                let (browser_id, prof) = parse_browser_spec(&spec);
                match self.registry.resolve(&browser_id, prof.as_deref()) {
                    Ok(resolved) => (Some(resolved.display_name), resolved.profile),
                    Err(_) => (Some(spec), prof),
                }
            }
            Ok(None) => {
                let default = self.config.runtime.default_browser()?;
                let (browser_id, prof) = parse_browser_spec(&default);
                match self.registry.resolve(&browser_id, prof.as_deref()) {
                    Ok(resolved) => (Some(resolved.display_name), resolved.profile),
                    Err(_) => (Some(default), prof),
                }
            }
            Err(_) => (None, None),
        };

        Ok(PreflightProbe {
            input_url,
            prepared_url: prepared.to_string(),
            host,
            config_matched,
            host_consented,
            resolved_url,
            preflight_error,
            lookup_duration,
            browser,
            profile,
        })
    }

    fn route_script(
        &self,
        url: &Url,
        context: &Context,
    ) -> Result<(Option<BrowserTarget>, Url, bool)> {
        let prepared = self.config.runtime.prepare_url(url, context)?;
        let routed_url = self
            .maybe_preflight(&prepared, context)?
            .unwrap_or(prepared);
        let target = self.config.runtime.match_handlers(&routed_url, context)?;
        Ok((target, routed_url, false))
    }

    fn maybe_preflight(&self, url: &Url, context: &Context) -> Result<Option<Url>> {
        let host = match url.host_str() {
            Some(host) => host,
            None => return Ok(None),
        };

        let config_matched = self.config.runtime.should_resolve(url, context)?;
        let host_consented = preflight::is_allowed(host)?;
        preflight::maybe_resolve(url, host_consented, config_matched)
    }

    fn resolve_target(&self, target: BrowserTarget) -> Result<ResolvedTarget> {
        let spec = target
            .name
            .context("browser target did not specify a browser name")?;
        let (browser_id, profile) = parse_browser_spec(&spec);
        let resolved = match self.registry.resolve(&browser_id, profile.as_deref()) {
            Ok(resolved) => resolved,
            Err(err) => {
                let fallback_spec = self.config.runtime.default_browser()?;
                let (fb_browser, fb_profile) = parse_browser_spec(&fallback_spec);
                if fb_browser == browser_id {
                    let installed = self
                        .registry
                        .list()
                        .iter()
                        .map(|b| b.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let hint = if installed.is_empty() {
                        "No browsers detected.".to_string()
                    } else {
                        format!("Detected browsers: {installed}")
                    };
                    anyhow::bail!(
                        "{err}. '{browser_id}' is configured as defaultBrowser but is not installed. {hint}"
                    );
                }
                eprintln!("browser resolution failed: {err}. Falling back to defaultBrowser.");
                let resolved = self.registry.resolve(&fb_browser, fb_profile.as_deref())?;
                return Ok((
                    resolved.id.clone(),
                    resolved.display_name.clone(),
                    resolved.profile.clone(),
                    resolved.profile_directory.clone(),
                    target.private,
                    resolved.app_path.clone(),
                    false,
                    true,
                ));
            }
        };

        Ok((
            resolved.id.clone(),
            resolved.display_name.clone(),
            resolved.profile.clone(),
            resolved.profile_directory.clone(),
            target.private,
            resolved.app_path.clone(),
            true,
            false,
        ))
    }
}

/// Decide which URL the browser gets.
///
/// Cleaning already picked one (in `route` mode deliberately the untouched
/// wrapper), but a `rewrite` rule or a preflight resolve may have transformed the
/// URL afterwards. Those are explicit user intent, so they win — otherwise both
/// features would be silently inert whenever cleaning wanted to keep the original.
fn pick_launch_url(cleaned: &url_clean::CleanOutcome, routed: &Url) -> Url {
    if *routed == cleaned.routed {
        cleaned.launch.clone()
    } else {
        routed.clone()
    }
}

fn parse_browser_spec(spec: &str) -> (String, Option<String>) {
    if let Some((browser, profile)) = spec.split_once(':') {
        (
            crate::browser::registry::normalize_browser_id(browser).to_string(),
            Some(profile.to_string()),
        )
    } else {
        (
            crate::browser::registry::normalize_browser_id(spec).to_string(),
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRAPPER: &str =
        "https://safelinks.protection.outlook.com/?url=https%3A%2F%2Fexample.org%2Fpage&sdata=sig";

    fn route_outcome() -> url_clean::CleanOutcome {
        url_clean::clean_url(&Url::parse(WRAPPER).unwrap(), "route")
    }

    #[test]
    fn untouched_route_keeps_the_wrapper_for_the_browser() {
        let cleaned = route_outcome();
        let routed = cleaned.routed.clone();
        assert_eq!(pick_launch_url(&cleaned, &routed).as_str(), WRAPPER);
    }

    #[test]
    fn rewrite_result_overrides_the_preserved_wrapper() {
        // A rewrite rule moved the URL somewhere else; launching the original
        // wrapper anyway would make `rewrite` a no-op under `route`.
        let cleaned = route_outcome();
        let rewritten = Url::parse("https://intranet.example/page").unwrap();
        assert_eq!(
            pick_launch_url(&cleaned, &rewritten).as_str(),
            "https://intranet.example/page"
        );
    }

    #[test]
    fn direct_mode_launches_what_it_routed() {
        let cleaned = url_clean::clean_url(&Url::parse(WRAPPER).unwrap(), "direct");
        let routed = cleaned.routed.clone();
        assert_eq!(
            pick_launch_url(&cleaned, &routed).as_str(),
            "https://example.org/page"
        );
    }

    #[test]
    fn log_line_names_the_routed_url_only_when_it_differs() {
        let mut decision = RouteDecision {
            input_url: WRAPPER.to_string(),
            routed_url: "https://example.org/page".to_string(),
            launch_url: WRAPPER.to_string(),
            browser_id: "brave".to_string(),
            browser: "Brave Browser".to_string(),
            profile: None,
            profile_directory: None,
            private: false,
            matched_handler: true,
            fallback: false,
            app_path: None,
        };
        assert!(decision
            .log_line()
            .contains("[matched as https://example.org/page]"));

        decision.launch_url = decision.routed_url.clone();
        assert!(!decision.log_line().contains("[matched as"));
    }
}
