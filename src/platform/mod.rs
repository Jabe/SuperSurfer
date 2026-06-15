use crate::context::Opener;
use crate::routing::Router;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::app_bundle_path;
#[cfg(target_os = "windows")]
pub use windows::{attach_parent_console, exe_path};

pub fn system_default_browser_id(registry: &crate::browser::registry::BrowserRegistry) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return macos::system_default_browser_id(registry);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::system_default_browser_id(registry);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = registry;
        None
    }
}

pub fn detect_opener() -> Option<Opener> {
    #[cfg(target_os = "macos")]
    {
        return macos::detect_opener();
    }
    #[cfg(target_os = "windows")]
    {
        return windows::detect_opener();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

pub fn register_default_browser() -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return macos::register_default_browser();
    }
    #[cfg(target_os = "windows")]
    {
        return windows::register_default_browser();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("default browser registration is not supported on this platform yet")
    }
}

pub fn registration_status() -> String {
    #[cfg(target_os = "macos")]
    {
        return macos::registration_status();
    }
    #[cfg(target_os = "windows")]
    {
        return windows::registration_status();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "unsupported platform".to_string()
    }
}

pub fn handle_url_arg(url: &str) -> anyhow::Result<()> {
    let router = Router::new()?;
    let mut context = crate::context::Context::default();
    if router.references_opener() {
        context.opener = detect_opener();
    }
    let decision = router.route_and_launch(url, &context, false)?;
    eprintln!(
        "routed {} -> {} via {}",
        decision.input_url, decision.cleaned_url, decision.browser
    );
    Ok(())
}
