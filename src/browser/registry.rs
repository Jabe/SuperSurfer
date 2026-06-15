use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::fs;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[cfg(target_os = "windows")]
#[path = "discover_windows.rs"]
mod discover_windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileKind {
    None,
    Gecko,
    Chromium,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserProfile {
    pub name: String,
    pub directory: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserInstall {
    pub id: String,
    pub display_name: String,
    pub app_path: Option<String>,
    pub bundle_id: Option<String>,
    pub profiles: Vec<BrowserProfile>,
}

#[derive(Debug, Clone)]
pub struct ResolvedBrowser {
    pub id: String,
    pub display_name: String,
    pub app_path: Option<String>,
    pub profile: Option<String>,
    pub profile_directory: Option<String>,
    pub private: bool,
}

#[derive(Debug)]
pub struct BrowserRegistry {
    browsers: HashMap<String, BrowserInstall>,
}

impl BrowserRegistry {
    pub fn discover() -> Result<Self> {
        let mut browsers = HashMap::new();
        for spec in known_browsers() {
            if let Some(install) = discover_browser(&spec)? {
                browsers.insert(spec.id.to_string(), install);
            }
        }
        Ok(Self { browsers })
    }

    pub fn list(&self) -> Vec<&BrowserInstall> {
        let mut items: Vec<_> = self.browsers.values().collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items
    }

    pub fn id_for_bundle_id(&self, bundle_id: &str) -> Option<String> {
        self.browsers.values().find_map(|install| {
            install
                .bundle_id
                .as_deref()
                .filter(|id| id.eq_ignore_ascii_case(bundle_id))
                .map(|_| install.id.clone())
        })
        .or_else(|| browser_id_for_bundle_id(bundle_id).map(str::to_string))
    }

    pub fn id_for_prog_id(&self, prog_id: &str) -> Option<String> {
        browser_id_for_prog_id(prog_id)
            .map(str::to_string)
            .filter(|id| self.browsers.contains_key(id))
    }

    pub fn resolve(&self, id: &str, profile: Option<&str>) -> Result<ResolvedBrowser> {
        let install = self
            .browsers
            .get(id)
            .with_context(|| format!("unknown browser '{id}'"))?;

        let profile_match = profile.and_then(|wanted| {
            install
                .profiles
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(wanted))
        });

        Ok(ResolvedBrowser {
            id: install.id.clone(),
            display_name: install.display_name.clone(),
            app_path: install.app_path.clone(),
            profile: profile.map(str::to_string),
            profile_directory: profile_match
                .and_then(|p| p.directory.clone())
                .or_else(|| profile_match.and_then(|p| p.path.clone())),
            private: false,
        })
    }
}

pub fn is_chromium_browser(id: &str) -> bool {
    known_browsers()
        .into_iter()
        .find(|b| b.id == id)
        .is_some_and(|b| b.profile_kind == ProfileKind::Chromium)
}

pub fn is_gecko_browser(id: &str) -> bool {
    known_browsers()
        .into_iter()
        .find(|b| b.id == id)
        .is_some_and(|b| b.profile_kind == ProfileKind::Gecko)
}

pub fn normalize_browser_id(name: &str) -> &str {
    let lower = name.to_lowercase();
    for spec in known_browsers() {
        if spec.id == lower || spec.display_name.to_lowercase() == lower {
            return spec.id;
        }
        for alias in spec.aliases {
            if alias.eq_ignore_ascii_case(name) {
                return spec.id;
            }
        }
    }
    name
}

pub fn browser_id_for_bundle_id(bundle_id: &str) -> Option<&'static str> {
    for spec in known_browsers() {
        for id in spec.mac_bundle_ids {
            if id.eq_ignore_ascii_case(bundle_id) {
                return Some(spec.id);
            }
        }
    }
    None
}

pub fn browser_id_for_prog_id(prog_id: &str) -> Option<&'static str> {
    let lower = prog_id.to_ascii_lowercase();
    if lower.contains("firefox") {
        return Some("firefox");
    }
    if lower.contains("brave") {
        if lower.contains("nightly") {
            return Some("brave-nightly");
        }
        if lower.contains("beta") {
            return Some("brave-beta");
        }
        return Some("brave");
    }
    if lower.contains("vivaldi") {
        return Some("vivaldi");
    }
    if lower.contains("operagx") || (lower.contains("opera") && lower.contains("gx")) {
        return Some("opera-gx");
    }
    if lower.contains("opera") {
        return Some("opera");
    }
    if lower.contains("msedge") || lower.contains("edgehtm") || lower.contains("edge") {
        if lower.contains("beta") {
            return Some("edge-beta");
        }
        if lower.contains("canary") || lower.contains("sxs") {
            return Some("edge-canary");
        }
        if lower.contains("dev") {
            return Some("edge-dev");
        }
        return Some("edge");
    }
    if lower.contains("chrome") {
        if lower.contains("sx") || lower.contains("canary") {
            return Some("chrome-canary");
        }
        return Some("chrome");
    }
    None
}

struct KnownBrowser {
    id: &'static str,
    display_name: &'static str,
    aliases: &'static [&'static str],
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_app_names: &'static [&'static str],
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_bundle_ids: &'static [&'static str],
    profile_kind: ProfileKind,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    chromium_data_dir: Option<&'static str>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    gecko_profiles_ini: Option<&'static str>,
}

fn known_browsers() -> Vec<KnownBrowser> {
    vec![
        browser(
            "safari",
            "Safari",
            &[],
            &["Safari.app"],
            &["com.apple.Safari"],
            ProfileKind::None,
            None,
            None,
        ),
        browser(
            "chrome",
            "Google Chrome",
            &["Chrome"],
            &["Google Chrome.app"],
            &["com.google.Chrome"],
            ProfileKind::Chromium,
            Some("Google/Chrome"),
            None,
        ),
        browser(
            "chrome-canary",
            "Google Chrome Canary",
            &["Chrome Canary"],
            &["Google Chrome Canary.app"],
            &["com.google.Chrome.canary"],
            ProfileKind::Chromium,
            Some("Google/Chrome Canary"),
            None,
        ),
        browser(
            "chromium",
            "Chromium",
            &[],
            &["Chromium.app"],
            &["org.chromium.Chromium"],
            ProfileKind::Chromium,
            Some("Chromium"),
            None,
        ),
        browser(
            "firefox",
            "Firefox",
            &[],
            &["Firefox.app"],
            &["org.mozilla.firefox"],
            ProfileKind::Gecko,
            None,
            Some("Firefox/profiles.ini"),
        ),
        browser(
            "firefox-developer-edition",
            "Firefox Developer Edition",
            &["Firefox Developer Edition", "Firefox Dev"],
            &["Firefox Developer Edition.app"],
            &["org.mozilla.firefoxdeveloperedition"],
            ProfileKind::Gecko,
            None,
            Some("Firefox/profiles.ini"),
        ),
        browser(
            "zen",
            "Zen",
            &["Zen Browser"],
            &["Zen.app", "Zen Browser.app"],
            &["app.zen-browser.zen"],
            ProfileKind::Gecko,
            None,
            Some("zen/profiles.ini"),
        ),
        browser(
            "waterfox",
            "Waterfox",
            &[],
            &["Waterfox.app"],
            &["org.waterfox.waterfox"],
            ProfileKind::Gecko,
            None,
            Some("Waterfox/profiles.ini"),
        ),
        browser(
            "tor",
            "Tor Browser",
            &["Tor"],
            &["Tor Browser.app"],
            &["org.torproject.torbrowser"],
            ProfileKind::Gecko,
            None,
            Some("Tor Browser/Browser/TorBrowser/Data/Browser/profiles.ini"),
        ),
        browser(
            "edge",
            "Microsoft Edge",
            &["Edge"],
            &["Microsoft Edge.app"],
            &["com.microsoft.edgemac"],
            ProfileKind::Chromium,
            Some("Microsoft Edge"),
            None,
        ),
        browser(
            "edge-beta",
            "Microsoft Edge Beta",
            &["Edge Beta"],
            &["Microsoft Edge Beta.app"],
            &["com.microsoft.edgemac.Beta"],
            ProfileKind::Chromium,
            Some("Microsoft Edge Beta"),
            None,
        ),
        browser(
            "edge-canary",
            "Microsoft Edge Canary",
            &["Edge Canary"],
            &["Microsoft Edge Canary.app"],
            &["com.microsoft.edgemac.Canary"],
            ProfileKind::Chromium,
            Some("Microsoft Edge Canary"),
            None,
        ),
        browser(
            "brave",
            "Brave Browser",
            &["Brave"],
            &["Brave Browser.app"],
            &["com.brave.Browser"],
            ProfileKind::Chromium,
            Some("BraveSoftware/Brave-Browser"),
            None,
        ),
        browser(
            "brave-beta",
            "Brave Browser Beta",
            &["Brave Beta"],
            &["Brave Browser Beta.app"],
            &["com.brave.Browser.beta"],
            ProfileKind::Chromium,
            Some("BraveSoftware/Brave-Browser-Beta"),
            None,
        ),
        browser(
            "brave-nightly",
            "Brave Browser Nightly",
            &["Brave Nightly"],
            &["Brave Browser Nightly.app"],
            &["com.brave.Browser.nightly"],
            ProfileKind::Chromium,
            Some("BraveSoftware/Brave-Browser-Nightly"),
            None,
        ),
        browser(
            "arc",
            "Arc",
            &[],
            &["Arc.app"],
            &["company.thebrowser.Browser"],
            ProfileKind::Chromium,
            Some("Arc/User Data"),
            None,
        ),
        browser(
            "dia",
            "Dia",
            &[],
            &["Dia.app"],
            &["company.thebrowser.dia"],
            ProfileKind::Chromium,
            Some("Dia/User Data"),
            None,
        ),
        browser(
            "vivaldi",
            "Vivaldi",
            &[],
            &["Vivaldi.app"],
            &["com.vivaldi.Vivaldi"],
            ProfileKind::Chromium,
            Some("Vivaldi"),
            None,
        ),
        browser(
            "opera",
            "Opera",
            &[],
            &["Opera.app"],
            &["com.operasoftware.Opera"],
            ProfileKind::Chromium,
            Some("com.operasoftware.Opera"),
            None,
        ),
        browser(
            "opera-gx",
            "Opera GX",
            &["OperaGX"],
            &["Opera GX.app"],
            &["com.operasoftware.OperaGX"],
            ProfileKind::Chromium,
            Some("com.operasoftware.OperaGX"),
            None,
        ),
        browser(
            "orion",
            "Orion",
            &["Orion Browser"],
            &["Orion.app", "Orion Browser.app"],
            &["com.kagi.kagimacOS"],
            ProfileKind::Chromium,
            Some("Orion/Data"),
            None,
        ),
        browser(
            "sigmaos",
            "SigmaOS",
            &["Sigma OS"],
            &["SigmaOS.app"],
            &["com.sigmaos.sigmaosmacos"],
            ProfileKind::Chromium,
            Some("SigmaOS"),
            None,
        ),
        browser(
            "sidekick",
            "Sidekick",
            &[],
            &["Sidekick.app"],
            &["com.pushplaylabs.sidekick"],
            ProfileKind::Chromium,
            Some("Sidekick"),
            None,
        ),
        browser(
            "helium",
            "Helium",
            &[],
            &["Helium.app"],
            &["com.imput.helium"],
            ProfileKind::Chromium,
            Some("Helium"),
            None,
        ),
        browser(
            "wavebox",
            "Wavebox",
            &[],
            &["Wavebox.app"],
            &["com.bookry.wavebox"],
            ProfileKind::Chromium,
            Some("Wavebox"),
            None,
        ),
        browser(
            "ungoogled-chromium",
            "Ungoogled Chromium",
            &["Ungoogled-Chromium"],
            &["Chromium.app", "Ungoogled Chromium.app"],
            &["org.ungoogled.chromium"],
            ProfileKind::Chromium,
            Some("Chromium"),
            None,
        ),
    ]
}

fn browser(
    id: &'static str,
    display_name: &'static str,
    aliases: &'static [&'static str],
    mac_app_names: &'static [&'static str],
    mac_bundle_ids: &'static [&'static str],
    profile_kind: ProfileKind,
    chromium_data_dir: Option<&'static str>,
    gecko_profiles_ini: Option<&'static str>,
) -> KnownBrowser {
    KnownBrowser {
        id,
        display_name,
        aliases,
        mac_app_names,
        mac_bundle_ids,
        profile_kind,
        chromium_data_dir,
        gecko_profiles_ini,
    }
}

fn discover_browser(spec: &KnownBrowser) -> Result<Option<BrowserInstall>> {
    #[cfg(target_os = "macos")]
    {
        return discover_browser_macos(spec);
    }
    #[cfg(target_os = "windows")]
    {
        return discover_browser_windows(spec);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = spec;
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn discover_browser_macos(spec: &KnownBrowser) -> Result<Option<BrowserInstall>> {
    let app_path = spec
        .mac_app_names
        .iter()
        .map(|name| PathBuf::from("/Applications").join(name))
        .chain(spec.mac_app_names.iter().map(|name| {
            directories::UserDirs::new()
                .map(|u| u.home_dir().to_path_buf())
                .unwrap_or_default()
                .join("Applications")
                .join(name)
        }))
        .find(|p| p.exists());

    let Some(app_path) = app_path else {
        return Ok(None);
    };

    let bundle_id = read_bundle_id(&app_path)
        .or_else(|| spec.mac_bundle_ids.first().map(|s| s.to_string()));
    let profiles = discover_profiles(spec, &app_path)?;

    Ok(Some(BrowserInstall {
        id: spec.id.to_string(),
        display_name: spec.display_name.to_string(),
        app_path: Some(app_path.to_string_lossy().to_string()),
        bundle_id,
        profiles,
    }))
}

#[cfg(target_os = "windows")]
fn discover_browser_windows(spec: &KnownBrowser) -> Result<Option<BrowserInstall>> {
    discover_windows::discover(spec)
}

#[cfg(target_os = "macos")]
fn read_bundle_id(app_path: &Path) -> Option<String> {
    let plist_path = app_path.join("Contents/Info.plist");
    let file = fs::File::open(plist_path).ok()?;
    let value: plist::Value = plist::from_reader(file).ok()?;
    value
        .as_dictionary()?
        .get("CFBundleIdentifier")?
        .as_string()
        .map(str::to_string)
}

#[cfg(target_os = "macos")]
fn discover_profiles(spec: &KnownBrowser, _app_path: &Path) -> Result<Vec<BrowserProfile>> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("no home directory")?;

    match spec.profile_kind {
        ProfileKind::None => Ok(vec![]),
        ProfileKind::Gecko => {
            let Some(relative) = spec.gecko_profiles_ini else {
                return Ok(vec![]);
            };
            discover_gecko_profiles(&home.join("Library/Application Support").join(relative))
        }
        ProfileKind::Chromium => {
            let Some(relative) = spec.chromium_data_dir else {
                return Ok(vec![]);
            };
            discover_chromium_profiles(&home.join("Library/Application Support").join(relative))
        }
    }
}

fn discover_gecko_profiles(ini_path: &Path) -> Result<Vec<BrowserProfile>> {
    if !ini_path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(ini_path)?;
    let mut profiles = Vec::new();
    let mut current_name = None;
    let mut current_path = None;
    for line in content.lines() {
        if let Some(name) = line.strip_prefix("Name=") {
            current_name = Some(name.to_string());
        } else if let Some(path) = line.strip_prefix("Path=") {
            current_path = Some(path.to_string());
        } else if line == "Default=1" || line.starts_with('[') {
            if let (Some(name), Some(path)) = (current_name.take(), current_path.take()) {
                profiles.push(BrowserProfile {
                    name: name.clone(),
                    directory: Some(name),
                    path: Some(path),
                });
            }
        }
    }
    if let (Some(name), Some(path)) = (current_name, current_path) {
        profiles.push(BrowserProfile {
            name: name.clone(),
            directory: Some(name),
            path: Some(path),
        });
    }
    Ok(profiles)
}

fn discover_chromium_profiles(support_dir: &Path) -> Result<Vec<BrowserProfile>> {
    let state_path = support_dir.join("Local State");
    if !state_path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(state_path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;
    let mut profiles = Vec::new();
    if let Some(info_cache) = json
        .pointer("/profile/info_cache")
        .and_then(|v| v.as_object())
    {
        for (dir, info) in info_cache {
            let name = info
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(dir)
                .to_string();
            profiles.push(BrowserProfile {
                name,
                directory: Some(dir.clone()),
                path: None,
            });
        }
    }
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_display_names() {
        assert_eq!(normalize_browser_id("Google Chrome"), "chrome");
        assert_eq!(normalize_browser_id("Brave Browser"), "brave");
        assert_eq!(normalize_browser_id("Firefox Developer Edition"), "firefox-developer-edition");
        assert_eq!(normalize_browser_id("Opera GX"), "opera-gx");
    }

    #[test]
    fn classifies_chromium_browsers() {
        assert!(is_chromium_browser("vivaldi"));
        assert!(is_chromium_browser("opera-gx"));
        assert!(!is_chromium_browser("firefox"));
        assert!(!is_chromium_browser("safari"));
    }

    #[test]
    fn maps_windows_prog_ids() {
        assert_eq!(browser_id_for_prog_id("ChromeHTML"), Some("chrome"));
        assert_eq!(browser_id_for_prog_id("BraveHTML"), Some("brave"));
        assert_eq!(browser_id_for_prog_id("MSEdgeHTM"), Some("edge"));
        assert_eq!(browser_id_for_prog_id("FirefoxURL"), Some("firefox"));
    }
}
