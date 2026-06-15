use crate::context::Opener;
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const BUNDLE_ID: &str = "dev.supersurfer.app";

pub fn detect_opener() -> Option<Opener> {
    let _ppid = std::process::id();
    let output = Command::new("ps")
        .args(["-o", "comm=", "-p"])
        .arg(parent_ppid()?.to_string())
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(Opener {
        name,
        bundle_id: None,
        path: None,
    })
}

fn parent_ppid() -> Option<u32> {
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p"])
        .arg(std::process::id().to_string())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse().ok()
}

pub fn app_bundle_path() -> Option<PathBuf> {
    if let Some(from_exe) = bundle_from_current_exe() {
        if from_exe.exists() {
            return Some(from_exe);
        }
    }

    for candidate in candidate_app_paths() {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn register_default_browser() -> Result<()> {
    let app = app_bundle_path().with_context(|| {
        format!(
            "SuperSurfer.app not found. Build it with `mise run package-macos`, then install to /Applications:\n  \
             cp -R dist/SuperSurfer.app /Applications/"
        )
    })?;

    register_with_launch_services(&app)?;

    if try_set_default_with_workspace(&app)? {
        println!("Set SuperSurfer as default browser for http and https.");
        return Ok(());
    }

    if try_register_with_duti()? {
        println!("Registered SuperSurfer as default browser for http and https (via duti).");
        return Ok(());
    }

    println!("Registered SuperSurfer.app with Launch Services.");
    open_default_browser_settings();
    println!("Choose SuperSurfer under Default web browser (quit and reopen System Settings if it is missing).");
    Ok(())
}

pub fn registration_status() -> String {
    let Some(app) = app_bundle_path() else {
        return "SuperSurfer.app not installed (run `mise run package-macos`)".to_string();
    };

    let mut parts = vec![format!("bundle: {}", app.display())];
    if let Ok(default_http) = default_handler_for("http") {
        parts.push(format!("default http handler: {default_http}"));
    }
    if let Ok(default_https) = default_handler_for("https") {
        parts.push(format!("default https handler: {default_https}"));
    }
    parts.join("; ")
}

fn bundle_from_current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos_dir.parent()?;
    let app = contents.parent()?;
    if app.extension()? == "app" {
        Some(app.to_path_buf())
    } else {
        None
    }
}

fn candidate_app_paths() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("dist/SuperSurfer.app"),
        PathBuf::from("/Applications/SuperSurfer.app"),
    ];
    if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
        paths.push(home.join("Applications/SuperSurfer.app"));
    }
    paths
}

fn register_with_launch_services(app: &Path) -> Result<()> {
    let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
    let status = Command::new(lsregister)
        .args(["-f", "-R", "-trusted"])
        .arg(app)
        .status()
        .with_context(|| format!("failed to run lsregister for {}", app.display()))?;

    if !status.success() {
        anyhow::bail!("lsregister exited with status {status}");
    }
    Ok(())
}

fn try_set_default_with_workspace(app: &Path) -> Result<bool> {
    let helper = app.join("Contents/MacOS/set-default");
    if !helper.exists() {
        return Ok(false);
    }
    let output = Command::new(&helper)
        .arg(app)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()?;
    Ok(output.status.success())
}

fn try_register_with_duti() -> Result<bool> {
    if Command::new("which").arg("duti").output()?.status.success() {
        for scheme in ["http", "https"] {
            let status = Command::new("duti")
                .args(["-s", BUNDLE_ID, scheme])
                .status()?;
            if !status.success() {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(false)
}

fn open_default_browser_settings() {
    let _ = Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.Desktop-Settings.extension")
        .status();
}

fn default_handler_for(scheme: &str) -> Result<String> {
    let output = Command::new("defaults")
        .args(["read", "com.apple.LaunchServices/com.apple.launchservices.secure"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("defaults read failed");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let marker = format!("\"{scheme}\" =");
    for line in text.lines() {
        if line.contains(&marker) && line.contains(BUNDLE_ID) {
            return Ok(BUNDLE_ID.to_string());
        }
    }
    Ok("not supersurfer".to_string())
}
