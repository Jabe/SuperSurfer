use crate::context::Opener;
use std::process::Command;

pub fn detect_opener() -> Option<Opener> {
    // Best-effort: inspect parent process name via ps.
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

pub fn register_default_browser() -> anyhow::Result<()> {
    anyhow::bail!(
        "automatic default-browser registration on macOS requires the SuperSurfer.app bundle.\n\
         Build the app bundle, then run:\n\
         open -a SuperSurfer --args supersurfer init --register\n\
         Or set SuperSurfer as default in System Settings → Desktop & Dock → Default web browser."
    )
}

pub fn registration_status() -> String {
    "manual registration via System Settings or SuperSurfer.app (see supersurfer doctor)".to_string()
}
