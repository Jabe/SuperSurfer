use crate::context::Opener;
use crate::routing::Router;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::desktop_file_path;
#[cfg(target_os = "macos")]
pub use macos::app_bundle_path;
#[cfg(target_os = "windows")]
pub use windows::{attach_parent_console, exe_path};

pub fn system_default_browser_id(
    registry: &crate::browser::registry::BrowserRegistry,
) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos::system_default_browser_id(registry)
    }
    #[cfg(target_os = "windows")]
    {
        windows::system_default_browser_id(registry)
    }
    #[cfg(target_os = "linux")]
    {
        linux::system_default_browser_id(registry)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = registry;
        None
    }
}

pub fn detect_opener() -> Option<Opener> {
    #[cfg(target_os = "macos")]
    {
        macos::detect_opener()
    }
    #[cfg(target_os = "windows")]
    {
        windows::detect_opener()
    }
    #[cfg(target_os = "linux")]
    {
        linux::detect_opener()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

pub fn register_default_browser() -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::register_default_browser()
    }
    #[cfg(target_os = "windows")]
    {
        windows::register_default_browser()
    }
    #[cfg(target_os = "linux")]
    {
        linux::register_default_browser()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        anyhow::bail!("default browser registration is not supported on this platform yet")
    }
}

pub fn registration_status() -> String {
    #[cfg(target_os = "macos")]
    {
        macos::registration_status()
    }
    #[cfg(target_os = "windows")]
    {
        windows::registration_status()
    }
    #[cfg(target_os = "linux")]
    {
        linux::registration_status()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unsupported platform".to_string()
    }
}

pub fn open_url_in_default_browser(url: &str) -> anyhow::Result<()> {
    use anyhow::Context as _;
    use std::process::Command;

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .status()
            .context("failed to run open")?
            .success()
            .then_some(())
            .context("open exited with failure")?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(url)
            .status()
            .context("failed to run xdg-open")?
            .success()
            .then_some(())
            .context("xdg-open exited with failure")?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .context("failed to run start")?
            .success()
            .then_some(())
            .context("start exited with failure")?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = url;
        anyhow::bail!("opening URLs is not supported on this platform yet");
    }
    Ok(())
}

pub fn handle_url_arg(url: &str, opener: Option<Opener>) -> anyhow::Result<()> {
    let router = Router::new()?;
    let mut context = crate::context::Context::default();
    if opener.is_some() {
        // The launcher already identified the originating app (e.g. on macOS, where
        // parent-process detection would only ever see SuperSurfer itself).
        context.opener = opener;
    } else if router.references_opener() {
        context.opener = detect_opener();
    }
    let decision = router.route_and_launch(url, &context, false)?;
    eprintln!(
        "routed {} -> {} via {}",
        decision.input_url, decision.cleaned_url, decision.browser
    );
    Ok(())
}
