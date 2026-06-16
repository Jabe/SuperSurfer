use crate::config;
use crate::platform;
use anyhow::{Context as _, Result};
use std::fs;
use std::path::PathBuf;

pub const LANDING_URL: &str = "https://github.com/Jabe/SuperSurfer/blob/main/docs/manual.md";

const MARKER_FILE: &str = "bootstrapped";

pub fn ensure_ready() -> Result<bool> {
    let marker = marker_path()?;
    if marker.exists() {
        return Ok(false);
    }

    let config = config::config_path()?;
    let types = config::types_path()?;

    if !config.exists() {
        config::write_scaffold(false)?;
    } else if !types.exists() {
        fs::write(&types, config::types_stub())?;
    }

    fs::write(&marker, env!("CARGO_PKG_VERSION"))
        .with_context(|| format!("failed to write bootstrap marker at {}", marker.display()))?;

    Ok(true)
}

pub fn welcome(fresh_bootstrap: bool) -> Result<()> {
    if fresh_bootstrap {
        platform::open_url_in_default_browser(LANDING_URL)?;
        println!("SuperSurfer is ready.");
        println!("Opened setup guide: {LANDING_URL}");
        println!("Next: edit config.ts, run `supersurfer doctor`, then `supersurfer register`.");
    } else {
        println!("SuperSurfer — cross-platform browser router");
        println!("  supersurfer doctor          show config and browsers");
        println!("  supersurfer test <url>      dry-run routing");
        println!("  supersurfer register        set as default browser");
        println!("Manual: {LANDING_URL}");
    }
    Ok(())
}

fn marker_path() -> Result<PathBuf> {
    Ok(config::config_dir()?.join(MARKER_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn landing_url_points_at_repo_manual() {
        assert!(LANDING_URL.contains("SuperSurfer"));
        assert!(LANDING_URL.ends_with("docs/manual.md"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn ensure_ready_is_idempotent_in_temp_config_home() {
        let temp = env::temp_dir().join(format!("supersurfer-bootstrap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();
        env::set_var("XDG_CONFIG_HOME", &temp);
        #[cfg(target_os = "macos")]
        env::set_var("HOME", &temp);

        let first = ensure_ready().expect("first bootstrap");
        assert!(first);
        assert!(config::config_path().unwrap().exists());
        assert!(marker_path().unwrap().exists());

        let second = ensure_ready().expect("second bootstrap");
        assert!(!second);

        env::remove_var("XDG_CONFIG_HOME");
        #[cfg(target_os = "macos")]
        env::remove_var("HOME");
        let _ = fs::remove_dir_all(temp);
    }
}
