use super::{
    discover_chromium_profiles, discover_gecko_profiles, BrowserInstall, BrowserProfile,
    KnownBrowser, ProfileKind,
};
use anyhow::Result;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::RegKey;

struct WindowsHints {
    exe_names: &'static [&'static str],
    program_files_paths: &'static [&'static str],
    local_appdata_paths: &'static [&'static str],
    start_menu_names: &'static [&'static str],
    /// Lowercased path fragment; at least one must appear in the resolved exe path.
    path_markers: &'static [&'static str],
    /// Lowercased path fragments that disqualify a match.
    path_excludes: &'static [&'static str],
}

pub(super) fn discover_one(
    spec: &KnownBrowser,
    start_menu: &[(String, String, String)],
    load_profiles: bool,
) -> Result<Option<BrowserInstall>> {
    let hints = windows_hints(spec.id);
    if let Some((exe_path, display_name)) = find_in_start_menu_internet(spec, hints, start_menu)? {
        return Ok(Some(build_install(
            spec,
            exe_path,
            display_name,
            load_profiles,
        )?));
    }
    if let Some(exe_path) = find_on_disk(hints) {
        return Ok(Some(build_install(spec, exe_path, None, load_profiles)?));
    }
    Ok(None)
}

pub(super) fn discover_profiles_for(spec: &KnownBrowser) -> Result<Vec<BrowserProfile>> {
    discover_profiles(spec)
}

fn build_install(
    spec: &KnownBrowser,
    exe_path: String,
    display_name: Option<String>,
    load_profiles: bool,
) -> Result<BrowserInstall> {
    Ok(BrowserInstall {
        id: spec.id.to_string(),
        display_name: display_name.unwrap_or_else(|| spec.display_name.to_string()),
        app_path: Some(exe_path),
        bundle_id: None,
        profiles: if load_profiles {
            discover_profiles(spec)?
        } else {
            vec![]
        },
    })
}

fn find_in_start_menu_internet(
    spec: &KnownBrowser,
    hints: Option<&WindowsHints>,
    start_menu: &[(String, String, String)],
) -> Result<Option<(String, Option<String>)>> {
    for (key_name, app_name, command) in start_menu {
        if matches_spec(spec, hints, &key_name, &app_name, &command) {
            if let Some(exe) = parse_command_path(&command) {
                if path_matches_hints(hints, &exe) && Path::new(&exe).exists() {
                    return Ok(Some((exe, Some(app_name.clone()))));
                }
            }
        }
    }
    Ok(None)
}

fn find_on_disk(hints: Option<&WindowsHints>) -> Option<String> {
    let hints = hints?;
    for path in candidate_paths(hints) {
        if path.is_file() && path_matches_hints(Some(hints), &path.to_string_lossy()) {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

fn candidate_paths(hints: &WindowsHints) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(base) = std::env::var(var) {
            for rel in hints.program_files_paths {
                paths.push(PathBuf::from(&base).join(rel));
            }
        }
    }
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        for rel in hints.local_appdata_paths {
            paths.push(PathBuf::from(&base).join(rel));
        }
    }
    paths
}

pub(super) fn enumerate_start_menu_browsers() -> Result<Vec<(String, String, String)>> {
    let mut found = Vec::new();
    let roots = [
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Clients\\StartMenuInternet"),
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node\\Clients\\StartMenuInternet",
        ),
        (HKEY_CURRENT_USER, "SOFTWARE\\Clients\\StartMenuInternet"),
    ];

    for (hive, subkey) in roots {
        let Ok(key) = RegKey::predef(hive).open_subkey(subkey) else {
            continue;
        };
        for key_name in key.enum_keys().filter_map(Result::ok) {
            let Ok(browser_key) = key.open_subkey(&key_name) else {
                continue;
            };
            let app_name = browser_key
                .open_subkey("Capabilities")
                .ok()
                .and_then(|caps| caps.get_value::<String, _>("ApplicationName").ok())
                .or_else(|| browser_key.get_value::<String, _>("").ok())
                .unwrap_or_else(|| key_name.clone());
            let command = browser_key
                .open_subkey("shell\\open\\command")
                .ok()
                .and_then(|cmd| cmd.get_value::<String, _>("").ok())
                .unwrap_or_default();
            if !command.is_empty() {
                found.push((key_name, app_name, command));
            }
        }
    }
    Ok(found)
}

fn matches_spec(
    spec: &KnownBrowser,
    hints: Option<&WindowsHints>,
    key_name: &str,
    app_name: &str,
    command: &str,
) -> bool {
    for name in [spec.display_name]
        .into_iter()
        .chain(spec.aliases.iter().copied())
        .chain(
            hints
                .map(|h| h.start_menu_names)
                .unwrap_or(&[])
                .iter()
                .copied(),
        )
    {
        if key_name.eq_ignore_ascii_case(name) || app_name.eq_ignore_ascii_case(name) {
            return true;
        }
    }

    if let (Some(hints), Some(exe)) = (hints, parse_command_path(command)) {
        if let Some(file_name) = Path::new(&exe).file_name().and_then(|s| s.to_str()) {
            if hints
                .exe_names
                .iter()
                .any(|candidate| file_name.eq_ignore_ascii_case(candidate))
            {
                return path_matches_hints(Some(hints), &exe);
            }
        }
    }

    false
}

fn path_matches_hints(hints: Option<&WindowsHints>, exe: &str) -> bool {
    let Some(hints) = hints else {
        return false;
    };
    let lower = exe.to_ascii_lowercase();

    for exclude in hints.path_excludes {
        if lower.contains(&exclude.to_ascii_lowercase()) {
            return false;
        }
    }

    if !hints.path_markers.is_empty() {
        return hints
            .path_markers
            .iter()
            .any(|marker| lower.contains(&marker.to_ascii_lowercase()));
    }

    hints
        .program_files_paths
        .iter()
        .chain(hints.local_appdata_paths.iter())
        .any(|rel| lower.contains(&rel.replace('/', "\\").to_ascii_lowercase()))
}

fn parse_command_path(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    trimmed.split_whitespace().next().map(str::to_string)
}

fn discover_profiles(spec: &KnownBrowser) -> Result<Vec<BrowserProfile>> {
    match spec.profile_kind {
        ProfileKind::None => Ok(vec![]),
        ProfileKind::Gecko => {
            let Some(path) = gecko_profiles_ini(spec) else {
                return Ok(vec![]);
            };
            discover_gecko_profiles(&path)
        }
        ProfileKind::Chromium => {
            let Some(path) = chromium_user_data_dir(spec) else {
                return Ok(vec![]);
            };
            discover_chromium_profiles(&path)
        }
    }
}

fn chromium_user_data_dir(spec: &KnownBrowser) -> Option<PathBuf> {
    let relative = spec.chromium_data_dir?;
    let local = std::env::var("LOCALAPPDATA").ok()?;
    Some(PathBuf::from(local).join(relative).join("User Data"))
}

fn gecko_profiles_ini(spec: &KnownBrowser) -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(match spec.id {
        "firefox" | "firefox-developer-edition" => {
            PathBuf::from(&appdata).join("Mozilla/Firefox/profiles.ini")
        }
        "waterfox" => PathBuf::from(&appdata).join("Waterfox/profiles.ini"),
        "zen" => PathBuf::from(&appdata).join("zen/profiles.ini"),
        "tor" => {
            PathBuf::from(&appdata).join("Tor Browser/Browser/TorBrowser/Data/Browser/profiles.ini")
        }
        _ => {
            let relative = spec.gecko_profiles_ini?;
            PathBuf::from(&appdata).join(relative)
        }
    })
}

fn windows_hints(id: &str) -> Option<&'static WindowsHints> {
    Some(match id {
        "safari" => return None,
        "chrome" => &HINTS_CHROME,
        "chrome-canary" => &HINTS_CHROME_CANARY,
        "chromium" => &HINTS_CHROMIUM,
        "firefox" => &HINTS_FIREFOX,
        "firefox-developer-edition" => &HINTS_FIREFOX_DEV,
        "zen" => &HINTS_ZEN,
        "waterfox" => &HINTS_WATERFOX,
        "tor" => &HINTS_TOR,
        "edge" => &HINTS_EDGE,
        "edge-beta" => &HINTS_EDGE_BETA,
        "edge-canary" => &HINTS_EDGE_CANARY,
        "brave" => &HINTS_BRAVE,
        "brave-beta" => &HINTS_BRAVE_BETA,
        "brave-nightly" => &HINTS_BRAVE_NIGHTLY,
        "arc" => &HINTS_ARC,
        "dia" => &HINTS_DIA,
        "vivaldi" => &HINTS_VIVALDI,
        "opera" => &HINTS_OPERA,
        "opera-gx" => &HINTS_OPERA_GX,
        "orion" => return None,
        "sigmaos" => return None,
        "sidekick" => &HINTS_SIDEKICK,
        "helium" => return None,
        "wavebox" => &HINTS_WAVEBOX,
        "ungoogled-chromium" => &HINTS_CHROMIUM,
        _ => return None,
    })
}

macro_rules! hints {
    ($exe:expr, $pf:expr, $local:expr, $names:expr, $markers:expr, $excludes:expr) => {
        WindowsHints {
            exe_names: $exe,
            program_files_paths: $pf,
            local_appdata_paths: $local,
            start_menu_names: $names,
            path_markers: $markers,
            path_excludes: $excludes,
        }
    };
}

const HINTS_CHROME: WindowsHints = hints!(
    &["chrome.exe"],
    &["Google/Chrome/Application/chrome.exe"],
    &["Google/Chrome/Application/chrome.exe"],
    &["Google Chrome"],
    &["google\\chrome\\application\\"],
    &["google\\chrome sxS\\"]
);
const HINTS_CHROME_CANARY: WindowsHints = hints!(
    &["chrome.exe"],
    &["Google/Chrome SxS/Application/chrome.exe"],
    &["Google/Chrome SxS/Application/chrome.exe"],
    &["Google Chrome Canary"],
    &["google\\chrome sxS\\"],
    &[]
);
const HINTS_CHROMIUM: WindowsHints = hints!(
    &["chromium.exe", "chrome.exe"],
    &["Chromium/Application/chrome.exe"],
    &[],
    &["Chromium"],
    &["chromium\\application\\"],
    &[]
);
const HINTS_FIREFOX: WindowsHints = hints!(
    &["firefox.exe"],
    &["Mozilla Firefox/firefox.exe"],
    &[],
    &["Firefox"],
    &["mozilla firefox\\"],
    &["developer edition"]
);
const HINTS_FIREFOX_DEV: WindowsHints = hints!(
    &["firefox.exe"],
    &["Firefox Developer Edition/firefox.exe"],
    &[],
    &["Firefox Developer Edition"],
    &["firefox developer edition\\"],
    &[]
);
const HINTS_ZEN: WindowsHints = hints!(
    &["zen.exe"],
    &["Zen Browser/zen.exe", "Zen/zen.exe"],
    &[],
    &["Zen", "Zen Browser"],
    &["zen browser\\", "\\zen\\zen.exe"],
    &[]
);
const HINTS_WATERFOX: WindowsHints = hints!(
    &["waterfox.exe"],
    &["Waterfox/waterfox.exe"],
    &[],
    &["Waterfox"],
    &["waterfox\\"],
    &[]
);
const HINTS_TOR: WindowsHints = hints!(
    &["firefox.exe"],
    &["Tor Browser/Browser/firefox.exe"],
    &[],
    &["Tor Browser"],
    &["tor browser\\"],
    &[]
);
const HINTS_EDGE: WindowsHints = hints!(
    &["msedge.exe"],
    &["Microsoft/Edge/Application/msedge.exe"],
    &[],
    &["Microsoft Edge", "MSEdge"],
    &["microsoft\\edge\\application\\"],
    &["edge beta\\", "edge sxs\\", "edge dev\\"]
);
const HINTS_EDGE_BETA: WindowsHints = hints!(
    &["msedge.exe"],
    &["Microsoft/Edge Beta/Application/msedge.exe"],
    &[],
    &["Microsoft Edge Beta", "MSEdgeBETA"],
    &["microsoft\\edge beta\\"],
    &[]
);
const HINTS_EDGE_CANARY: WindowsHints = hints!(
    &["msedge.exe"],
    &["Microsoft/Edge SxS/Application/msedge.exe"],
    &[],
    &["Microsoft Edge Canary", "MSEdgeCanary"],
    &["microsoft\\edge sxs\\"],
    &[]
);
const HINTS_BRAVE: WindowsHints = hints!(
    &["brave.exe"],
    &["BraveSoftware/Brave-Browser/Application/brave.exe"],
    &["BraveSoftware/Brave-Browser/Application/brave.exe"],
    &["Brave", "Brave Browser"],
    &["bravesoftware\\brave-browser\\"],
    &["brave-browser-beta\\", "brave-browser-nightly\\"]
);
const HINTS_BRAVE_BETA: WindowsHints = hints!(
    &["brave.exe"],
    &["BraveSoftware/Brave-Browser-Beta/Application/brave.exe"],
    &["BraveSoftware/Brave-Browser-Beta/Application/brave.exe"],
    &["Brave Beta"],
    &["brave-browser-beta\\"],
    &[]
);
const HINTS_BRAVE_NIGHTLY: WindowsHints = hints!(
    &["brave.exe"],
    &["BraveSoftware/Brave-Browser-Nightly/Application/brave.exe"],
    &["BraveSoftware/Brave-Browser-Nightly/Application/brave.exe"],
    &["Brave Nightly"],
    &["brave-browser-nightly\\"],
    &[]
);
const HINTS_ARC: WindowsHints = hints!(
    &["Arc.exe"],
    &["Arc/Application/Arc.exe"],
    &[],
    &["Arc"],
    &["\\arc\\application\\"],
    &[]
);
const HINTS_DIA: WindowsHints = hints!(
    &["Dia.exe"],
    &["Dia/Application/Dia.exe"],
    &[],
    &["Dia"],
    &["\\dia\\application\\"],
    &[]
);
const HINTS_VIVALDI: WindowsHints = hints!(
    &["vivaldi.exe"],
    &["Vivaldi/Application/vivaldi.exe"],
    &[],
    &["Vivaldi"],
    &["vivaldi\\application\\"],
    &[]
);
const HINTS_OPERA: WindowsHints = hints!(
    &["opera.exe"],
    &["Opera/launcher.exe", "Opera/opera.exe"],
    &[],
    &["Opera"],
    &["\\opera\\"],
    &["opera gx\\"]
);
const HINTS_OPERA_GX: WindowsHints = hints!(
    &["opera.exe"],
    &["Opera GX/opera.exe", "Opera GX/launcher.exe"],
    &[],
    &["Opera GX"],
    &["opera gx\\"],
    &[]
);
const HINTS_SIDEKICK: WindowsHints = hints!(
    &["sidekick.exe"],
    &["Sidekick/Application/sidekick.exe"],
    &[],
    &["Sidekick"],
    &["sidekick\\application\\"],
    &[]
);
const HINTS_WAVEBOX: WindowsHints = hints!(
    &["wavebox.exe"],
    &["Wavebox/wavebox.exe"],
    &[],
    &["Wavebox"],
    &["wavebox\\"],
    &[]
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_command_paths() {
        assert_eq!(
            parse_command_path("\"C:\\\\Program Files\\\\Chrome\\\\chrome.exe\" -- \"%1\""),
            Some("C:\\Program Files\\Chrome\\chrome.exe".to_string())
        );
    }

    #[test]
    fn parses_unquoted_command_paths() {
        assert_eq!(
            parse_command_path("C:\\Firefox\\firefox.exe -osint -url \"%1\""),
            Some("C:\\Firefox\\firefox.exe".to_string())
        );
    }

    #[test]
    fn brave_variants_do_not_cross_match() {
        let stable =
            r"C:\Users\jan\AppData\Local\BraveSoftware\Brave-Browser\Application\brave.exe";
        assert!(path_matches_hints(Some(&HINTS_BRAVE), stable));
        assert!(!path_matches_hints(Some(&HINTS_BRAVE_BETA), stable));
        assert!(!path_matches_hints(Some(&HINTS_BRAVE_NIGHTLY), stable));
    }

    #[test]
    fn brave_beta_requires_beta_directory() {
        let beta =
            r"C:\Users\jan\AppData\Local\BraveSoftware\Brave-Browser-Beta\Application\brave.exe";
        assert!(path_matches_hints(Some(&HINTS_BRAVE_BETA), beta));
        assert!(!path_matches_hints(Some(&HINTS_BRAVE), beta));
    }
}
