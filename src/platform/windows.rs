use crate::browser::registry::BrowserRegistry;
use crate::context::Opener;
use anyhow::{Context as _, Result};
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

pub const APP_NAME: &str = "SuperSurfer";
pub const PROG_ID: &str = "SuperSurferURL";
pub const PROG_ID_HTML: &str = "SuperSurferHTML";

pub fn detect_opener() -> Option<Opener> {
    // Hot path: native APIs only — never spawn PowerShell or other shells.
    let name = parent_process_name()?;
    Some(Opener {
        name,
        bundle_id: None,
        path: None,
    })
}

fn parent_process_name() -> Option<String> {
    let pid = unsafe { GetCurrentProcessId() };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }

    let result = (|| {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..unsafe { std::mem::zeroed() }
        };

        if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
            return None;
        }

        let mut parent_pid = None;
        loop {
            if entry.th32ProcessID == pid {
                parent_pid = Some(entry.th32ParentProcessID);
                break;
            }
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }

        let parent_pid = parent_pid?;
        if parent_pid == 0 {
            return None;
        }

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
            return None;
        }

        loop {
            if entry.th32ProcessID == parent_pid {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                let name = name.trim_end_matches(".exe").to_ascii_lowercase();
                return if name.is_empty() { None } else { Some(name) };
            }
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }
        None
    })();

    unsafe { CloseHandle(snapshot) };
    result
}

pub fn exe_path() -> Result<PathBuf> {
    std::env::current_exe().context("could not resolve supersurfer.exe path")
}

pub fn register_default_browser() -> Result<()> {
    let exe = exe_path()?;
    write_registry(&exe)?;

    println!("Registered SuperSurfer in the Windows default-apps list.");
    println!(
        "Settings → Apps → Default apps → SuperSurfer → Set default \
         (or set HTTP, HTTPS, .htm, and .html individually)."
    );
    let _ = Command::new("cmd")
        .args([
            "/C",
            "start",
            "ms-settings:defaultapps?registeredAppUser=SuperSurfer",
        ])
        .status();
    Ok(())
}

pub fn system_default_browser_id(registry: &BrowserRegistry) -> Option<String> {
    let prog_id = system_default_prog_id("https").or_else(|| system_default_prog_id("http"))?;
    registry.id_for_prog_id(&prog_id)
}

fn system_default_prog_id(scheme: &str) -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice");
    hkcu.open_subkey(path).ok()?.get_value("ProgId").ok()
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

    let (capabilities, _) = clients.create_subkey("Capabilities")?;
    capabilities.set_value("ApplicationDescription", &"SuperSurfer browser router")?;
    capabilities.set_value("ApplicationIcon", &icon)?;
    capabilities.set_value("ApplicationName", &APP_NAME)?;
    capabilities.set_value("StartMenuInternet", &APP_NAME)?;

    let (url_assoc, _) = capabilities.create_subkey("URLAssociations")?;
    url_assoc.set_value("http", &PROG_ID)?;
    url_assoc.set_value("https", &PROG_ID)?;

    let (file_assoc, _) = capabilities.create_subkey("FileAssociations")?;
    file_assoc.set_value(".htm", &PROG_ID_HTML)?;
    file_assoc.set_value(".html", &PROG_ID_HTML)?;

    write_prog_id(&hkcu, PROG_ID, APP_NAME, &command, &icon, true)?;
    let html_display = format!("{APP_NAME} HTML Document");
    write_prog_id(&hkcu, PROG_ID_HTML, &html_display, &command, &icon, false)?;

    Ok(())
}

fn write_prog_id(
    hkcu: &RegKey,
    prog_id: &str,
    display_name: &str,
    command: &String,
    icon: &String,
    url_protocol: bool,
) -> Result<()> {
    let (classes, _) = hkcu.create_subkey(format!("Software\\Classes\\{prog_id}"))?;
    classes.set_value("", &display_name.to_string())?;
    if url_protocol {
        classes.set_value("URL Protocol", &"")?;
        classes.set_value("EditFlags", &0x00000002u32)?;
    }
    classes.set_value("FriendlyTypeName", &display_name.to_string())?;
    classes
        .create_subkey("DefaultIcon")?
        .0
        .set_value("", icon)?;
    classes
        .create_subkey("shell\\open\\command")?
        .0
        .set_value("", command)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_process_name_does_not_panic() {
        let _ = parent_process_name();
    }
}
