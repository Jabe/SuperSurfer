use crate::browser::registry::BrowserRegistry;
use crate::platform;
use anyhow::Result;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct ScaffoldPlan {
    pub default_browser: String,
    pub default_source: ScaffoldSource,
    pub github_browser: String,
    pub meeting_browser: String,
    pub slack_browser: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaffoldSource {
    SystemDefault,
    InstalledFallback,
    PlatformFallback,
}

pub fn plan() -> Result<ScaffoldPlan> {
    let registry = BrowserRegistry::discover()?;
    let installed: HashSet<String> = registry.list().into_iter().map(|b| b.id.clone()).collect();

    let (default_browser, default_source) =
        choose_default(&registry, &installed);

    let github_browser = pick_installed(&installed, &["chrome", "brave", "edge", "firefox"])
        .unwrap_or_else(|| default_browser.clone());

    let meeting_browser = meeting_browser_target(&registry, &installed, &github_browser);

    let slack_browser = pick_installed(&installed, &["firefox", "chrome", "brave", "edge"])
        .filter(|id| id != &default_browser)
        .or_else(|| pick_installed(&installed, &["firefox", "chrome", "brave", "edge"]))
        .unwrap_or_else(|| default_browser.clone());

    Ok(ScaffoldPlan {
        default_browser,
        default_source,
        github_browser,
        meeting_browser,
        slack_browser,
    })
}

pub fn render(plan: &ScaffoldPlan) -> String {
    format!(
        r#"import type {{ RouterConfig }} from "./supersurfer";

export default {{
  defaultBrowser: "{default}",
  urlCleaning: "default",
  handlers: [
    {{ match: domain("github.com"), browser: "{github}" }},
    {{
      match: [host("meet.google.com"), suffix(".zoom.us")],
      browser: "{meeting}",
    }},
    {{
      match: (url, ctx) => ctx.opener?.name === "Slack",
      browser: "{slack}",
    }},
  ],
}} satisfies RouterConfig;
"#,
        default = plan.default_browser,
        github = plan.github_browser,
        meeting = plan.meeting_browser,
        slack = plan.slack_browser,
    )
}

pub fn summary(plan: &ScaffoldPlan) -> String {
    let source = match plan.default_source {
        ScaffoldSource::SystemDefault => "current system default",
        ScaffoldSource::InstalledFallback => "first installed browser",
        ScaffoldSource::PlatformFallback => "platform fallback",
    };
    format!(
        "defaultBrowser: {} ({source}); github -> {}; meetings -> {}; Slack opener -> {}",
        plan.default_browser, plan.github_browser, plan.meeting_browser, plan.slack_browser
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

fn meeting_browser_target(
    registry: &BrowserRegistry,
    installed: &HashSet<String>,
    github_browser: &str,
) -> String {
    if !installed.contains("chrome") {
        return github_browser.to_string();
    }

    let has_work = registry
        .list()
        .iter()
        .find(|b| b.id == "chrome")
        .is_some_and(|chrome| {
            chrome
                .profiles
                .iter()
                .any(|profile| profile.name.eq_ignore_ascii_case("work"))
        });

    if has_work {
        "chrome:work".to_string()
    } else {
        "chrome".to_string()
    }
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
    fn render_includes_planned_browser_ids() {
        let plan = ScaffoldPlan {
            default_browser: "brave".to_string(),
            default_source: ScaffoldSource::SystemDefault,
            github_browser: "chrome".to_string(),
            meeting_browser: "chrome:work".to_string(),
            slack_browser: "firefox".to_string(),
        };
        let config = render(&plan);
        assert!(config.contains(r#"defaultBrowser: "brave""#));
        assert!(config.contains(r#"browser: "chrome:work""#));
        assert!(config.contains(r#"browser: "firefox""#));
    }
}
