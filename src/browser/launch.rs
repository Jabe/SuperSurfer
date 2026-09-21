#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use crate::browser::registry::is_chromium_browser;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use crate::browser::registry::is_gecko_browser;
use crate::browser::registry::BrowserRegistry;
use crate::routing::RouteDecision;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use anyhow::Context as _;
use anyhow::Result;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use std::process::Command;
#[cfg(any(test, target_os = "windows", target_os = "linux"))]
use std::process::Stdio;

pub fn launch_browser(_registry: &BrowserRegistry, decision: &RouteDecision) -> Result<()> {
    ensure_launchable_url(&decision.launch_url)?;
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

/// Final defense-in-depth gate before handing a URL to the browser: whatever
/// routing, config rewrite rules, or URL cleaning produced, only ever launch
/// web URLs and local files. Earlier layers already filter their own inputs,
/// but a hostile rewrite returning e.g. `javascript:` must die here too.
fn ensure_launchable_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url)
        .map_err(|err| anyhow::anyhow!("refusing to launch unparseable URL {url:?}: {err}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        // Local files only. `file://host/share` is a remote filesystem; handing
        // it to the browser (or converting it to a UNC path on Windows) makes
        // the machine authenticate to that host.
        "file" if crate::input_url::is_local_file_url(&parsed) => Ok(()),
        "file" => anyhow::bail!("refusing to launch remote file URL: {url}"),
        other => anyhow::bail!(
            "refusing to launch URL with scheme {other:?} (allowed: http, https, file): {url}"
        ),
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
    cmd.arg(&decision.launch_url);
    spawn_gui_process(cmd)
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

    // Always go through Launch Services (`open`), never the inner binary.
    // Spawning Edge/Chrome directly makes SuperSurfer the TCC responsible
    // process, so macOS reports "was prevented from modifying apps" when the
    // browser updater touches its own bundle. `open` is short-lived, so
    // waiting on it is fine.
    let status = Command::new("open")
        .args(macos_open_args(
            app_path,
            &decision.launch_url,
            &browser_args,
        ))
        .status()
        .context("failed to launch browser")?;
    status
        .success()
        .then_some(())
        .context("browser launcher exited with failure")
}

/// argv for `/usr/bin/open`. Extra flags need `-n`: without it, Launch Services
/// drops `--args` when the app is already running. `-n` starts a short-lived
/// new instance that Chromium/Gecko fold into the existing process via the
/// singleton lock. The URL stays after `--args` so it is not opened as a
/// document in the last-used profile.
#[cfg(any(test, target_os = "macos"))]
fn macos_open_args(app_path: &str, url: &str, browser_args: &[String]) -> Vec<String> {
    if browser_args.is_empty() {
        return vec!["-a".to_string(), app_path.to_string(), url.to_string()];
    }
    let mut args = vec![
        "-n".to_string(),
        "-a".to_string(),
        app_path.to_string(),
        "--args".to_string(),
    ];
    args.extend(browser_args.iter().cloned());
    args.push(url.to_string());
    args
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
    cmd.arg(browser_launch_arg(&decision.launch_url));
    spawn_gui_process(cmd)
}

/// Start a long-lived GUI browser and return immediately.
///
/// Waiting on the child (`.status()`) hangs SuperSurfer for the lifetime of
/// the browser whenever this process *is* the browser rather than a stub that
/// hands off to an already-running instance. That pins the macOS launcher on
/// its main thread (`waitUntilExit`), so subsequent link clicks do nothing.
#[cfg(any(test, target_os = "windows", target_os = "linux"))]
fn spawn_gui_process(mut cmd: Command) -> Result<()> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    drop(cmd.spawn().context("failed to launch browser")?);
    Ok(())
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
    use super::{ensure_launchable_url, macos_open_args, private_window_flag, spawn_gui_process};
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn private_flag_is_browser_specific() {
        assert_eq!(private_window_flag("chrome"), Some("--incognito"));
        assert_eq!(private_window_flag("brave"), Some("--incognito"));
        assert_eq!(private_window_flag("edge"), Some("--inprivate"));
        assert_eq!(private_window_flag("firefox"), Some("--private-window"));
        // Safari has no CLI private flag — must not pass a bogus one.
        assert_eq!(private_window_flag("safari"), None);
    }

    #[test]
    fn launch_gate_allows_web_and_file_urls() {
        assert!(ensure_launchable_url("https://example.com/path?x=1").is_ok());
        assert!(ensure_launchable_url("http://example.com/").is_ok());
        assert!(ensure_launchable_url("file:///tmp/report.html").is_ok());
        assert!(ensure_launchable_url("file://localhost/tmp/report.html").is_ok());
    }

    #[test]
    fn launch_gate_rejects_remote_file_urls() {
        assert!(ensure_launchable_url("file://evil.example/share/a.html").is_err());
        assert!(ensure_launchable_url("file://192.168.1.5/share/a.html").is_err());
    }

    #[test]
    fn launch_gate_rejects_dangerous_schemes() {
        for url in [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)", // Url::parse lowercases the scheme
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "chrome://settings",
            "ms-msdt:/id PCWDiagnostic",
            "not a url at all",
        ] {
            assert!(ensure_launchable_url(url).is_err(), "{url} must be refused");
        }
    }

    #[test]
    fn macos_open_passes_url_as_document_without_extra_args() {
        assert_eq!(
            macos_open_args(
                "/Applications/Brave Browser.app",
                "https://example.com",
                &[]
            ),
            [
                "-a",
                "/Applications/Brave Browser.app",
                "https://example.com"
            ]
        );
    }

    #[test]
    fn macos_open_uses_new_instance_so_profile_args_are_not_dropped() {
        assert_eq!(
            macos_open_args(
                "/Applications/Microsoft Edge.app",
                "https://example.com",
                &["--profile-directory=Profile 1".to_string()],
            ),
            [
                "-n",
                "-a",
                "/Applications/Microsoft Edge.app",
                "--args",
                "--profile-directory=Profile 1",
                "https://example.com",
            ]
        );
    }

    #[test]
    fn macos_open_keeps_url_after_args_not_as_document() {
        let args = macos_open_args(
            "/Applications/Firefox.app",
            "https://example.com",
            &["-P".to_string(), "Work".to_string()],
        );
        let split = args.iter().position(|a| a == "--args").expect("--args");
        assert!(
            !args[..split].iter().any(|a| a == "https://example.com"),
            "URL before --args would open in the last-used profile"
        );
        assert_eq!(args.last().map(String::as_str), Some("https://example.com"));
    }

    #[test]
    fn spawn_gui_process_does_not_wait_for_child() {
        let cmd = if cfg!(windows) {
            let mut c = Command::new("ping");
            c.args(["-n", "8", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("8");
            c
        };
        let start = Instant::now();
        spawn_gui_process(cmd).expect("spawn");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "must not wait for the child to exit"
        );
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
