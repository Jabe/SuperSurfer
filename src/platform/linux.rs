use crate::browser::registry::BrowserRegistry;
use crate::context::Opener;
use anyhow::{Context as _, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub const DESKTOP_FILE_NAME: &str = "supersurfer.desktop";

pub fn detect_opener() -> Option<Opener> {
    let ppid = parent_ppid()?;
    let comm = fs::read_to_string(format!("/proc/{ppid}/comm")).ok()?;
    let name = comm.trim().to_string();
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
    // /proc/self/stat: "pid (comm) state ppid ...". comm may contain spaces and
    // parentheses, so locate the field after the final ')'.
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    let mut fields = after_comm.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

pub fn register_default_browser() -> Result<()> {
    let exe = std::env::current_exe().context("could not resolve supersurfer binary path")?;
    let desktop_path = desktop_file_path().context("could not resolve applications directory")?;

    if let Some(parent) = desktop_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    fs::write(
        &desktop_path,
        desktop_file_contents(&exe.display().to_string()),
    )
    .with_context(|| format!("could not write {}", desktop_path.display()))?;

    if let Some(apps_dir) = desktop_path.parent() {
        let _ = Command::new("update-desktop-database")
            .arg(apps_dir)
            .status();
    }

    let set_via_xdg_settings = Command::new("xdg-settings")
        .args(["set", "default-web-browser", DESKTOP_FILE_NAME])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    let mut mime_ok = true;
    for scheme in [
        "x-scheme-handler/http",
        "x-scheme-handler/https",
        "text/html",
    ] {
        let ok = Command::new("xdg-mime")
            .args(["default", DESKTOP_FILE_NAME, scheme])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        mime_ok = mime_ok && ok;
    }

    if set_via_xdg_settings || mime_ok {
        println!(
            "Registered SuperSurfer as default browser for http and https.\n  desktop entry: {}",
            desktop_path.display()
        );
    } else {
        println!(
            "Installed desktop entry at {} but could not set it as default automatically.\n  \
             Set it manually with: xdg-settings set default-web-browser {DESKTOP_FILE_NAME}",
            desktop_path.display()
        );
    }
    Ok(())
}

pub fn system_default_browser_id(registry: &BrowserRegistry) -> Option<String> {
    let desktop_id = default_web_browser_desktop_id()?;
    if desktop_id == DESKTOP_FILE_NAME {
        return None;
    }
    registry.id_for_desktop_id(&desktop_id)
}

pub fn registration_status() -> String {
    let Some(desktop_path) = desktop_file_path() else {
        return "could not resolve applications directory".to_string();
    };
    if !desktop_path.exists() {
        return "not registered (run `supersurfer register`)".to_string();
    }

    let mut parts = vec![format!("desktop entry: {}", desktop_path.display())];
    if let Some(current) = default_web_browser_desktop_id() {
        parts.push(format!("default-web-browser: {current}"));
    }
    if let Some(http) = mime_default_handler("x-scheme-handler/http") {
        parts.push(format!("http handler: {http}"));
    }
    if let Some(https) = mime_default_handler("x-scheme-handler/https") {
        parts.push(format!("https handler: {https}"));
    }
    parts.join("; ")
}

pub fn desktop_file_path() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(
        base.data_local_dir()
            .join("applications")
            .join(DESKTOP_FILE_NAME),
    )
}

fn default_web_browser_desktop_id() -> Option<String> {
    let output = Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn mime_default_handler(mime: &str) -> Option<String> {
    let output = Command::new("xdg-mime")
        .args(["query", "default", mime])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn desktop_file_contents(exec_path: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=SuperSurfer\n\
         Comment=Browser router\n\
         Exec={exec_path} %u\n\
         Terminal=false\n\
         Categories=Network;WebBrowser;\n\
         MimeType=x-scheme-handler/http;x-scheme-handler/https;text/html;\n\
         NoDisplay=false\n\
         StartupNotify=false\n"
    )
}
