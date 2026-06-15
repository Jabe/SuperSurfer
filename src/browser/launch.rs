#[cfg(target_os = "macos")]
use crate::browser::registry::is_gecko_browser;
use crate::browser::registry::{is_chromium_browser, BrowserRegistry};
use crate::routing::RouteDecision;
use anyhow::{Context as _, Result};
use std::process::Command;

#[cfg(target_os = "macos")]
use std::fs;
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

pub fn launch_browser(_registry: &BrowserRegistry, decision: &RouteDecision) -> Result<()> {
    let app_path = decision
        .app_path
        .as_ref()
        .context("no application path resolved for browser launch")?;

    #[cfg(target_os = "macos")]
    {
        launch_macos(app_path, decision)
    }
    #[cfg(target_os = "windows")]
    {
        launch_windows(app_path, decision)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("browser launch is not supported on this platform yet")
    }
}

#[cfg(target_os = "macos")]
fn launch_macos(app_path: &str, decision: &RouteDecision) -> Result<()> {
    let mut browser_args = Vec::new();
    if let Some(profile_dir) = chromium_profile_arg(
        decision.browser_id.as_str(),
        decision.profile_directory.as_deref(),
    ) {
        browser_args.push(profile_dir);
    } else if let Some(profile) = decision.profile.as_deref() {
        if is_gecko_browser(decision.browser_id.as_str()) {
            browser_args.push("-P".to_string());
            browser_args.push(profile.to_string());
        }
    }
    if decision.private {
        browser_args.push("--private".to_string());
    }

    let status = if browser_args.is_empty() {
        // Pass the URL as a document so macOS delivers it to a running browser instance.
        Command::new("open")
            .arg("-a")
            .arg(app_path)
            .arg(&decision.cleaned_url)
            .status()
    } else if is_chromium_browser(decision.browser_id.as_str())
        || is_gecko_browser(decision.browser_id.as_str())
    {
        let exe = macos_app_executable(app_path)?;
        Command::new(&exe)
            .args(&browser_args)
            .arg(&decision.cleaned_url)
            .status()
    } else {
        browser_args.push(decision.cleaned_url.clone());
        Command::new("open")
            .arg("-a")
            .arg(app_path)
            .arg("--args")
            .args(&browser_args)
            .status()
    }
    .context("failed to launch browser")?;

    status
        .success()
        .then_some(())
        .context("browser launcher exited with failure")
}

#[cfg(target_os = "macos")]
fn macos_app_executable(app_path: &str) -> Result<PathBuf> {
    let app = Path::new(app_path);
    let plist_path = app.join("Contents/Info.plist");
    let file = fs::File::open(&plist_path)
        .with_context(|| format!("could not open {}", plist_path.display()))?;
    let value: plist::Value = plist::from_reader(file)
        .with_context(|| format!("could not parse {}", plist_path.display()))?;
    let name = value
        .as_dictionary()
        .and_then(|dict| dict.get("CFBundleExecutable"))
        .and_then(|value| value.as_string())
        .context("CFBundleExecutable missing from Info.plist")?;
    Ok(app.join("Contents/MacOS").join(name))
}

#[cfg(target_os = "windows")]
fn launch_windows(exe_path: &str, decision: &RouteDecision) -> Result<()> {
    let mut cmd = Command::new(exe_path);
    if let Some(profile_dir) = chromium_profile_arg(
        decision.browser_id.as_str(),
        decision.profile_directory.as_deref(),
    ) {
        cmd.arg(profile_dir);
    }
    if decision.private {
        cmd.arg("--inprivate");
    }
    cmd.arg(browser_launch_arg(&decision.cleaned_url));
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

#[cfg(target_os = "windows")]
fn browser_launch_arg(url: &str) -> String {
    if !url.starts_with("file://") {
        return url.to_string();
    }
    if let Ok(parsed) = url::Url::parse(url) {
        if let Ok(path) = parsed.to_file_path() {
            return path.to_string_lossy().into_owned();
        }
    }
    url.to_string()
}
