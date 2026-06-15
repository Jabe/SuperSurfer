use crate::context::Opener;
use std::process::Command;

pub fn detect_opener() -> Option<Opener> {
    // Best-effort parent process inspection on Windows.
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_Process -Filter \"ProcessId=$PID\").ParentProcessId",
        ])
        .output()
        .ok()?;
    let ppid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ppid.is_empty() {
        return None;
    }
    let name_out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "(Get-CimInstance Win32_Process -Filter \"ProcessId={ppid}\").Name"
            ),
        ])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&name_out.stdout)
        .trim()
        .trim_end_matches(".exe")
        .to_string();
    if name.is_empty() {
        return None;
    }
    Some(Opener {
        name,
        bundle_id: None,
        path: None,
    })
}

pub fn register_default_browser() -> anyhow::Result<()> {
    anyhow::bail!(
        "automatic default-browser registration on Windows is not yet implemented.\n\
         Use Settings → Apps → Default apps → Web browser and select SuperSurfer after installing."
    )
}

pub fn registration_status() -> String {
    "not registered (automatic registration not yet implemented on Windows)".to_string()
}
