use crate::context::Opener;
use anyhow::{Context as _, Result};
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

pub const APP_NAME: &str = "SuperSurfer";
pub const PROG_ID: &str = "SuperSurferURL";

pub fn detect_opener() -> Option<Opener> {
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

pub fn exe_path() -> Result<PathBuf> {
    std::env::current_exe().context("could not resolve supersurfer.exe path")
}

pub fn register_default_browser() -> Result<()> {
    let exe = exe_path()?;
    write_registry(&exe)?;

    println!("Registered SuperSurfer in the Windows browser list.");
    println!("Set it as default in Settings → Apps → Default apps → Web browser → SuperSurfer.");
    let _ = Command::new("cmd")
        .args(["/C", "start", "ms-settings:defaultapps"])
        .status();
    Ok(())
}

pub fn registration_status() -> String {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let registered = hkcu
        .open_subkey("Software\\RegisteredApplications")
        .and_then(|k| k.get_value::<String, _>(APP_NAME))
        .is_ok();
    if registered {
        format!("{APP_NAME} is registered (check Default apps for http/https handler)")
    } else {
        "not registered (run `supersurfer init --register`)".to_string()
    }
}

fn write_registry(exe: &PathBuf) -> Result<()> {
    let command = format!("\"{}\" \"%1\"", exe.display());
    let icon = format!("{},0", exe.display());

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    hkcu.create_subkey("Software\\RegisteredApplications")?
        .0
        .set_value(APP_NAME, &format!("Software\\Clients\\StartMenuInternet\\{APP_NAME}\\Capabilities"))?;

    let (clients, _) = hkcu.create_subkey(format!("Software\\Clients\\StartMenuInternet\\{APP_NAME}"))?;
    clients.set_value("", &APP_NAME)?;
    clients
        .create_subkey("shell\\open\\command")?
        .0
        .set_value("", &command)?;

    let (capabilities, _) =
        clients.create_subkey("Capabilities")?;
    capabilities.set_value("ApplicationDescription", &"SuperSurfer browser router")?;
    capabilities.set_value("ApplicationIcon", &icon)?;
    capabilities.set_value("ApplicationName", &APP_NAME)?;
    capabilities.set_value("StartMenuInternet", &APP_NAME)?;

    let (url_assoc, _) = capabilities.create_subkey("URLAssociations")?;
    url_assoc.set_value("http", &PROG_ID)?;
    url_assoc.set_value("https", &PROG_ID)?;

    let (classes, _) = hkcu.create_subkey(format!("Software\\Classes\\{PROG_ID}"))?;
    classes.set_value("", &APP_NAME)?;
    classes.set_value("URL Protocol", &"")?;
    classes.set_value("EditFlags", &0x00000002u32)?;
    classes.set_value("FriendlyTypeName", &APP_NAME)?;

    classes
        .create_subkey("DefaultIcon")?
        .0
        .set_value("", &icon)?;
    classes
        .create_subkey("shell\\open\\command")?
        .0
        .set_value("", &command)?;

    Ok(())
}
