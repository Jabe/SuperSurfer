use crate::browser::registry::BrowserRegistry;
use crate::platform;
use anyhow::Result;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct ScaffoldPlan {
    pub default_browser: String,
    pub default_source: ScaffoldSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaffoldSource {
    SystemDefault,
    InstalledFallback,
    PlatformFallback,
}

pub fn plan() -> Result<ScaffoldPlan> {
    let registry = BrowserRegistry::discover_fresh()?;
    let installed: HashSet<String> = registry.list().into_iter().map(|b| b.id.clone()).collect();
    let (default_browser, default_source) = choose_default(&registry, &installed);

    Ok(ScaffoldPlan {
        default_browser,
        default_source,
    })
}

pub fn render(plan: &ScaffoldPlan) -> String {
    format!(
        r#"import type {{ RouterConfig }} from "./supersurfer";

export default {{
  defaultBrowser: "{default}",
  urlCleaning: "default",
  handlers: [],
}} satisfies RouterConfig;
"#,
        default = plan.default_browser,
    )
}

pub fn summary(plan: &ScaffoldPlan) -> String {
    let source = match plan.default_source {
        ScaffoldSource::SystemDefault => "current system default",
        ScaffoldSource::InstalledFallback => "first installed browser",
        ScaffoldSource::PlatformFallback => "platform fallback",
    };
    format!(
        "defaultBrowser: {} ({source})",
        plan.default_browser,
    )
}

fn choose_default(
    registry: &BrowserRegistry,
    installed: &HashSet<String>,
) -> (String, ScaffoldSource) {
    if let Some(id) = platform::system_default_browser_id(registry) {
        if installed.contains(&id) {
            return (id, ScaffoldSource::SystemDefault);
        }
    }

    if let Some(id) = pick_installed(installed, platform_preference_order()) {
        return (id, ScaffoldSource::InstalledFallback);
    }

    (
        platform_fallback_browser().to_string(),
        ScaffoldSource::PlatformFallback,
    )
}

fn pick_installed(installed: &HashSet<String>, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|id| installed.contains(**id))
        .map(|id| (*id).to_string())
}

fn platform_preference_order() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["safari", "chrome", "brave", "edge", "firefox"]
    }
    #[cfg(target_os = "windows")]
    {
        &["chrome", "edge", "brave", "firefox"]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        &["chrome", "firefox"]
    }
}

fn platform_fallback_browser() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "safari"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "chrome"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_includes_default_browser_only() {
        let plan = ScaffoldPlan {
            default_browser: "brave".to_string(),
            default_source: ScaffoldSource::SystemDefault,
        };
        let config = render(&plan);
        assert!(config.contains(r#"defaultBrowser: "brave""#));
        assert!(config.contains("handlers: []"));
        assert!(!config.contains("github.com"));
        assert!(!config.contains("zoom.us"));
        assert!(!config.contains("Slack"));
    }
}
