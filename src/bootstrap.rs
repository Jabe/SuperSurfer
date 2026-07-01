use crate::config;
use crate::platform;
use anyhow::{Context as _, Result};
use std::fs;
use std::path::PathBuf;

pub const LANDING_URL: &str = "https://github.com/Jabe/SuperSurfer/blob/main/docs/manual.md";

const MARKER_FILE: &str = "bootstrapped";

pub fn ensure_ready() -> Result<bool> {
    let marker = marker_path()?;
    let current_version = env!("CARGO_PKG_VERSION");
    let stored_version = marker
        .exists()
        .then(|| fs::read_to_string(&marker).ok())
        .flatten()
        .map(|s| s.trim().to_string());

    // Marker present and matches this binary's version → already bootstrapped
    // for this release. A version mismatch (upgrade) re-runs the scaffold step
    // so users get refreshed types/d.ts after an update, even when the config
    // dir is synced across machines (the marker travels with it).
    if stored_version.as_deref() == Some(current_version) {
        return Ok(false);
    }

    let config = config::config_path()?;
    let types = config::types_path()?;

    if !config.exists() {
        config::write_scaffold(false)?;
    } else if !types.exists() || stored_version.is_some() {
        // On upgrade (stored_version is Some but mismatched), refresh types so
        // editor IntelliSense tracks the installed SuperSurfer version.
        fs::write(&types, config::types_stub())?;
    }

    fs::write(&marker, current_version)
        .with_context(|| format!("failed to write bootstrap marker at {}", marker.display()))?;

    Ok(true)
}

pub fn welcome(fresh_bootstrap: bool) -> Result<()> {
    if fresh_bootstrap {
        platform::open_url_in_default_browser(LANDING_URL)?;
        println!("SuperSurfer is ready.");
        println!("Opened setup guide: {LANDING_URL}");
        println!("Next: edit config.js, run `supersurfer doctor`, then `supersurfer register`.");
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
    use std::sync::Mutex;

    // Both bootstrap tests mutate XDG_CONFIG_HOME / HOME, which are process-wide
    // env vars. Serialize them so parallel test execution can't clobber each
    // other's config dir.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn landing_url_points_at_repo_manual() {
        assert!(LANDING_URL.contains("SuperSurfer"));
        assert!(LANDING_URL.ends_with("docs/manual.md"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn ensure_ready_is_idempotent_in_temp_config_home() {
        let _guard = ENV_LOCK.lock().unwrap();
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

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn ensure_ready_reruns_on_version_mismatch() {
        let _guard = ENV_LOCK.lock().unwrap();
        let temp = env::temp_dir().join(format!(
            "supersurfer-bootstrap-upgrade-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();
        env::set_var("XDG_CONFIG_HOME", &temp);
        #[cfg(target_os = "macos")]
        env::set_var("HOME", &temp);

        // First bootstrap writes the current version.
        assert!(ensure_ready().expect("first bootstrap"));
        let marker = marker_path().unwrap();
        assert_eq!(
            fs::read_to_string(&marker).unwrap().trim(),
            env!("CARGO_PKG_VERSION")
        );

        // Simulate an upgrade from an older version: stale marker, types present.
        fs::write(&marker, "0.0.1").unwrap();
        let types_path = config::types_path().unwrap();
        let types_before = fs::read_to_string(&types_path).unwrap();
        let rerun = ensure_ready().expect("upgrade bootstrap");
        assert!(rerun, "version mismatch should re-run bootstrap");
        assert_eq!(
            fs::read_to_string(&marker).unwrap().trim(),
            env!("CARGO_PKG_VERSION"),
            "marker should be updated to current version"
        );
        // Types should be refreshed (rewritten with the stub).
        let types_after = fs::read_to_string(&types_path).unwrap();
        assert_eq!(
            types_before, types_after,
            "types content should be refreshed"
        );

        env::remove_var("XDG_CONFIG_HOME");
        #[cfg(target_os = "macos")]
        env::remove_var("HOME");
        let _ = fs::remove_dir_all(temp);
    }
}
