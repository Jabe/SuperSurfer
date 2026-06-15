use crate::browser::registry::{is_chromium_browser, is_gecko_browser, BrowserRegistry};
use crate::routing::RouteDecision;
use anyhow::{Context as _, Result};
use std::process::Command;

pub fn launch_browser(_registry: &BrowserRegistry, decision: &RouteDecision) -> Result<()> {
    let app_path = decision
        .app_path
        .as_ref()
        .context("no application path resolved for browser launch")?;

    #[cfg(target_os = "macos")]
    {
        return launch_macos(app_path, decision);
    }
    #[cfg(target_os = "windows")]
    {
        return launch_windows(app_path, decision);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("browser launch is not supported on this platform yet")
    }
}

#[cfg(target_os = "macos")]
fn launch_macos(app_path: &str, decision: &RouteDecision) -> Result<()> {
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg(app_path);

    let mut args = Vec::new();
    if let Some(profile_dir) = chromium_profile_arg(decision.browser_id.as_str(), decision.profile_directory.as_deref())
    {
        args.push(profile_dir);
    } else if let Some(profile) = decision.profile.as_deref() {
        if is_gecko_browser(decision.browser_id.as_str()) {
            args.push("-P".to_string());
            args.push(profile.to_string());
        }
    }
    if decision.private {
        args.push("--private".to_string());
    }
    args.push(decision.cleaned_url.clone());

    if !args.is_empty() {
        cmd.arg("--args").args(&args);
    } else {
        cmd.arg(decision.cleaned_url.as_str());
    }

    cmd.status()
        .context("failed to launch browser")?
        .success()
        .then_some(())
        .context("browser launcher exited with failure")
}

#[cfg(target_os = "windows")]
fn launch_windows(exe_path: &str, decision: &RouteDecision) -> Result<()> {
    let mut cmd = Command::new(exe_path);
    if let Some(profile_dir) = chromium_profile_arg(decision.browser_id.as_str(), decision.profile_directory.as_deref())
    {
        cmd.arg(profile_dir);
    }
    if decision.private {
        cmd.arg("--inprivate");
    }
    cmd.arg(&decision.cleaned_url);
    cmd.status()
        .context("failed to launch browser")?
        .success()
        .then_some(())
        .context("browser launcher exited with failure")
}

fn chromium_profile_arg(browser_id: &str, profile: Option<&str>) -> Option<String> {
    if !is_chromium_browser(browser_id) {
        return None;
    }
    profile.map(|dir| format!("--profile-directory={dir}"))
}
