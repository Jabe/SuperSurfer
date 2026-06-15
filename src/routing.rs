use crate::browser::{launch::launch_browser, registry::BrowserRegistry};
use crate::config::loader::{load_default_config, LoadedConfig};
use crate::context::Context;
use crate::logging;
use crate::script::runtime::BrowserTarget;
use crate::url_clean;
use anyhow::{Context as _, Result};
use std::path::PathBuf;
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
pub struct RouteDecision {
    pub input_url: String,
    pub cleaned_url: String,
    pub browser_id: String,
    pub browser: String,
    pub profile: Option<String>,
    pub profile_directory: Option<String>,
    pub private: bool,
    pub matched_handler: bool,
    pub fallback: bool,
    pub app_path: Option<String>,
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
        let raw_url = crate::input_url::normalize_input_url(raw_url)?;
        let mut url = Url::parse(&raw_url).with_context(|| format!("invalid URL: {raw_url}"))?;
        let input_url = url.to_string();

        let cleaning_mode = self.config.runtime.url_cleaning_mode()?;
        url_clean::clean_url(&mut url, &cleaning_mode)?;

        let (target, routed_url) = match self.config.runtime.route(&url, context)? {
            (Some(target), rewritten) => (target, rewritten),
            (None, rewritten) => (
                BrowserTarget {
                    name: Some(self.config.runtime.default_browser()?),
                    private: false,
                    app: None,
                    exe: None,
                },
                rewritten,
            ),
        };

        let (
            browser_id,
            browser,
            profile,
            profile_directory,
            private,
            app_path,
            matched_handler,
            fallback,
        ) = self.resolve_target(target)?;

        let decision = RouteDecision {
            input_url,
            cleaned_url: routed_url.to_string(),
            browser_id,
            browser,
            profile,
            profile_directory,
            private,
            matched_handler,
            fallback,
            app_path,
        };

        logging::append_decision(&format!(
            "{} -> {} ({})",
            decision.input_url, decision.cleaned_url, decision.browser
        ))?;

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

    fn resolve_target(&self, target: BrowserTarget) -> Result<ResolvedTarget> {
        if let Some(app) = target.app {
            let name = target.name.unwrap_or_else(|| "custom-app".to_string());
            return Ok((
                name.clone(),
                name,
                None,
                None,
                target.private,
                Some(app),
                true,
                false,
            ));
        }

        #[cfg(target_os = "windows")]
        if let Some(exe) = target.exe {
            let name = target.name.unwrap_or_else(|| "custom-exe".to_string());
            return Ok((
                name.clone(),
                name,
                None,
                None,
                target.private,
                Some(exe),
                true,
                false,
            ));
        }

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
