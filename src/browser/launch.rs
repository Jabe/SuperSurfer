#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use crate::browser::registry::is_chromium_browser;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use crate::browser::registry::is_gecko_browser;
use crate::browser::registry::BrowserRegistry;
use crate::routing::RouteDecision;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use anyhow::Context as _;
use anyhow::Result;
#[cfg(target_os = "macos")]
use std::fs;
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use std::process::Command;

pub fn launch_browser(_registry: &BrowserRegistry, decision: &RouteDecision) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let app_path = decision
            .app_path
            .as_ref()
            .context("no application path resolved for browser launch")?;
        launch_macos(app_path, decision)
    }
    #[cfg(target_os = "windows")]
    {
        let app_path = decision
            .app_path
            .as_ref()
            .context("no application path resolved for browser launch")?;
        launch_windows(app_path, decision)
    }
    #[cfg(target_os = "linux")]
    {
        let app_path = decision
            .app_path
            .as_ref()
            .context("no application path resolved for browser launch")?;
        launch_linux(app_path, decision)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = decision;
        anyhow::bail!("browser launch is not supported on this platform yet")
    }
}

#[cfg(target_os = "linux")]
fn launch_linux(exec: &str, decision: &RouteDecision) -> Result<()> {
    let mut cmd = Command::new(exec);
    if let Some(profile_dir) = chromium_profile_arg(
        decision.browser_id.as_str(),
        decision.profile_directory.as_deref(),
    ) {
        cmd.arg(profile_dir);
    } else if let Some(profile) = decision.profile.as_deref() {
        if is_gecko_browser(decision.browser_id.as_str()) {
            cmd.arg("-P");
            cmd.arg(profile);
        }
    }
    if decision.private {
        if let Some(flag) = private_window_flag(decision.browser_id.as_str()) {
            cmd.arg(flag);
        }
    }
    cmd.arg(&decision.cleaned_url);
    cmd.status()
        .context("failed to launch browser")?
        .success()
        .then_some(())
        .context("browser launcher exited with failure")
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
        // Safari and other non-Chromium/Gecko browsers have no documented
        // command-line private-mode flag, so only pass one where it works.
        if let Some(flag) = private_window_flag(decision.browser_id.as_str()) {
            browser_args.push(flag.to_string());
        }
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
    } else if let Some(profile) = decision.profile.as_deref() {
        if is_gecko_browser(decision.browser_id.as_str()) {
            cmd.arg("-P");
            cmd.arg(profile);
        }
    }
    if decision.private {
        if let Some(flag) = private_window_flag(decision.browser_id.as_str()) {
            cmd.arg(flag);
        }
    }
    cmd.arg(browser_launch_arg(&decision.cleaned_url));
    cmd.status()
        .context("failed to launch browser")?
        .success()
        .then_some(())
        .context("browser launcher exited with failure")
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn chromium_profile_arg(browser_id: &str, profile: Option<&str>) -> Option<String> {
    if !is_chromium_browser(browser_id) {
        return None;
    }
    profile.map(|dir| format!("--profile-directory={dir}"))
}

/// The command-line flag that opens a private/incognito window for the given
/// browser, or `None` if the browser has no documented one. Chrome/Brave/Vivaldi
/// use `--incognito`, Edge uses `--inprivate`, Firefox/Gecko use `--private-window`.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn private_window_flag(browser_id: &str) -> Option<&'static str> {
    if is_gecko_browser(browser_id) {
        Some("--private-window")
    } else if browser_id == "edge" {
        Some("--inprivate")
    } else if is_chromium_browser(browser_id) {
        Some("--incognito")
    } else {
        None
    }
}

#[cfg(all(
    test,
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
mod tests {
    use super::private_window_flag;

    #[test]
    fn private_flag_is_browser_specific() {
        assert_eq!(private_window_flag("chrome"), Some("--incognito"));
        assert_eq!(private_window_flag("brave"), Some("--incognito"));
        assert_eq!(private_window_flag("edge"), Some("--inprivate"));
        assert_eq!(private_window_flag("firefox"), Some("--private-window"));
        // Safari has no CLI private flag — must not pass a bogus one.
        assert_eq!(private_window_flag("safari"), None);
    }
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
