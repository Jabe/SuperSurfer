use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use std::fs;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use std::path::Path;
#[cfg(any(target_os = "macos", target_os = "linux"))]
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
        discover_inner(false)
    }

    /// Full discovery with profiles; bypasses the on-disk cache.
    pub fn discover_fresh() -> Result<Self> {
        discover_inner(true)
    }

    pub fn list(&self) -> Vec<&BrowserInstall> {
        let mut items: Vec<_> = self.browsers.values().collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items
    }

    pub fn id_for_bundle_id(&self, bundle_id: &str) -> Option<String> {
        self.browsers
            .values()
            .find_map(|install| {
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

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn id_for_desktop_id(&self, desktop_id: &str) -> Option<String> {
        browser_id_for_desktop_id(desktop_id).map(str::to_string)
    }

    pub fn resolve(&self, id: &str, profile: Option<&str>) -> Result<ResolvedBrowser> {
        let install = self
            .browsers
            .get(id)
            .with_context(|| format!("unknown browser '{id}'"))?;

        let profiles = if profile.is_some() && install.profiles.is_empty() {
            #[cfg(target_os = "windows")]
            {
                if let Some(spec) = known_browsers().into_iter().find(|s| s.id == id) {
                    discover_windows::discover_profiles_for(&spec)?
                } else {
                    install.profiles.clone()
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                install.profiles.clone()
            }
        } else {
            install.profiles.clone()
        };

        let profile_match = profile.and_then(|wanted| {
            profiles
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

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn browser_id_for_desktop_id(desktop_id: &str) -> Option<&'static str> {
    let needle = desktop_id.trim_end_matches(".desktop");
    for spec in known_browsers() {
        for id in spec.linux_desktop_ids {
            if id.trim_end_matches(".desktop").eq_ignore_ascii_case(needle) {
                return Some(spec.id);
            }
        }
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
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    linux_desktop_ids: &'static [&'static str],
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    linux_config_dir: Option<&'static str>,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    linux_gecko_dir: Option<&'static str>,
}

impl KnownBrowser {
    fn linux(
        mut self,
        desktop_ids: &'static [&'static str],
        config_dir: Option<&'static str>,
        gecko_dir: Option<&'static str>,
    ) -> Self {
        self.linux_desktop_ids = desktop_ids;
        self.linux_config_dir = config_dir;
        self.linux_gecko_dir = gecko_dir;
        self
    }
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
        )
        .linux(&["google-chrome.desktop"], Some("google-chrome"), None),
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
        )
        .linux(
            &[
                "chromium.desktop",
                "chromium-browser.desktop",
                "chromium_chromium.desktop",
            ],
            Some("chromium"),
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
        )
        .linux(
            &[
                "firefox.desktop",
                "firefox-esr.desktop",
                "firefox_firefox.desktop",
                "org.mozilla.firefox.desktop",
            ],
            None,
            Some(".mozilla/firefox"),
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
        )
        .linux(
            &["firefox-developer-edition.desktop"],
            None,
            Some(".mozilla/firefox"),
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
        )
        .linux(
            &[
                "zen.desktop",
                "zen-browser.desktop",
                "app.zen_browser.zen.desktop",
            ],
            None,
            Some(".zen"),
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
        )
        .linux(&["waterfox.desktop"], None, Some(".waterfox")),
        browser(
            "tor",
            "Tor Browser",
            &["Tor"],
            &["Tor Browser.app"],
            &["org.torproject.torbrowser"],
            ProfileKind::Gecko,
            None,
            Some("Tor Browser/Browser/TorBrowser/Data/Browser/profiles.ini"),
        )
        .linux(
            &[
                "torbrowser.desktop",
                "org.torproject.torbrowser-launcher.desktop",
            ],
            None,
            None,
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
        )
        .linux(
            &["microsoft-edge.desktop", "microsoft-edge-dev.desktop"],
            Some("microsoft-edge"),
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
        )
        .linux(
            &["microsoft-edge-beta.desktop"],
            Some("microsoft-edge-beta"),
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
        )
        .linux(
            &["brave-browser.desktop", "brave_brave.desktop"],
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
        )
        .linux(
            &["brave-browser-beta.desktop"],
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
        )
        .linux(
            &["brave-browser-nightly.desktop"],
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
        )
        .linux(
            &["vivaldi-stable.desktop", "vivaldi.desktop"],
            Some("vivaldi"),
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
        )
        .linux(
            &["opera.desktop", "opera_opera.desktop"],
            Some("opera"),
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
        )
        .linux(
            &["ungoogled-chromium.desktop", "chromium.desktop"],
            Some("chromium"),
            None,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
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
        linux_desktop_ids: &[],
        linux_config_dir: None,
        linux_gecko_dir: None,
    }
}

#[cfg(target_os = "macos")]
fn discover_inner(fresh: bool) -> Result<BrowserRegistry> {
    use crate::browser::cache;

    // Cheap pass: resolve each known browser's .app bundle path (a couple of
    // stats per browser) and the mtime of its Info.plist. This snapshot is the
    // cache key and invalidates on install/update/reinstall.
    let mut snapshot: Vec<(String, std::time::SystemTime)> = Vec::new();
    let mut resolved: Vec<(KnownBrowser, PathBuf)> = Vec::new();
    for spec in known_browsers() {
        if let Some(path) = resolve_app_path_macos(&spec) {
            let plist = path.join("Contents/Info.plist");
            let mtime = fs::metadata(&plist)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            snapshot.push((path.to_string_lossy().to_string(), mtime));
            resolved.push((spec, path));
        }
    }
    let fingerprint = cache::macos_fingerprint(&snapshot);

    if !fresh {
        if let Some(browsers) = cache::load(&fingerprint)? {
            return Ok(BrowserRegistry { browsers });
        }
    }

    // Cache miss: do the expensive work (plist parse + profile discovery) for
    // each resolved bundle, then persist.
    let mut browsers = HashMap::new();
    for (spec, app_path) in &resolved {
        if let Some(install) = discover_browser_macos(spec, app_path)? {
            browsers.insert(spec.id.to_string(), install);
        }
    }

    cache::save(&fingerprint, &browsers)?;

    Ok(BrowserRegistry { browsers })
}

/// Resolve a known browser's `.app` bundle path by checking the system and
/// user Applications directories. Returns the first existing match.
#[cfg(target_os = "macos")]
fn resolve_app_path_macos(spec: &KnownBrowser) -> Option<PathBuf> {
    spec.mac_app_names
        .iter()
        .map(|name| PathBuf::from("/Applications").join(name))
        .chain(spec.mac_app_names.iter().map(|name| {
            directories::UserDirs::new()
                .map(|u| u.home_dir().to_path_buf())
                .unwrap_or_default()
                .join("Applications")
                .join(name)
        }))
        .find(|p| p.exists())
}

#[cfg(target_os = "windows")]
fn discover_inner(fresh: bool) -> Result<BrowserRegistry> {
    discover_windows_cached(fresh)
}

#[cfg(target_os = "linux")]
fn discover_inner(_fresh: bool) -> Result<BrowserRegistry> {
    let dirs = linux_application_dirs();
    let mut browsers = HashMap::new();
    for spec in known_browsers() {
        if let Some(install) = discover_browser_linux(&spec, &dirs)? {
            browsers.insert(spec.id.to_string(), install);
        }
    }
    Ok(BrowserRegistry { browsers })
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn discover_inner(_fresh: bool) -> Result<BrowserRegistry> {
    Ok(BrowserRegistry {
        browsers: HashMap::new(),
    })
}

#[cfg(target_os = "linux")]
fn linux_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.data_local_dir().join("applications"));
    }
    // Always search standard install locations. CLI sessions often omit snap/flatpak
    // paths from XDG_DATA_DIRS even when those browsers are the system default.
    for path in [
        "/usr/local/share/applications",
        "/usr/share/applications",
        "/var/lib/snapd/desktop/applications",
        "/var/lib/flatpak/exports/share/applications",
    ] {
        dirs.push(PathBuf::from(path));
    }
    let xdg_data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for entry in xdg_data_dirs.split(':') {
        if entry.is_empty() {
            continue;
        }
        dirs.push(Path::new(entry).join("applications"));
    }
    dirs.dedup();
    dirs
}

#[cfg(target_os = "linux")]
fn discover_browser_linux(spec: &KnownBrowser, dirs: &[PathBuf]) -> Result<Option<BrowserInstall>> {
    let desktop_file = spec.linux_desktop_ids.iter().find_map(|id| {
        dirs.iter()
            .map(|dir| dir.join(id))
            .find(|path| path.exists())
    });

    let Some(desktop_file) = desktop_file else {
        return Ok(None);
    };

    let Some(exec) = parse_desktop_exec(&desktop_file) else {
        return Ok(None);
    };

    let profiles = discover_profiles_linux(spec)?;

    Ok(Some(BrowserInstall {
        id: spec.id.to_string(),
        display_name: spec.display_name.to_string(),
        app_path: Some(exec),
        bundle_id: None,
        profiles,
    }))
}

#[cfg(target_os = "linux")]
fn parse_desktop_exec(desktop_file: &Path) -> Option<String> {
    let content = fs::read_to_string(desktop_file).ok()?;
    let mut in_entry = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_entry = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("Exec=") {
            return parse_desktop_exec_value(value);
        }
    }
    None
}

/// Parse an `Exec=` value from a `.desktop` file, skipping `env VAR=…` prefixes
/// used by Snap/Flatpak entries.
#[cfg(any(target_os = "linux", test))]
fn parse_desktop_exec_value(value: &str) -> Option<String> {
    let mut tokens = value.split_whitespace().peekable();

    if tokens.peek() == Some(&"env") {
        tokens.next();
        while tokens.peek().is_some_and(|token| {
            token.contains('=') && !token.starts_with('/') && !token.starts_with('%')
        }) {
            tokens.next();
        }
    }

    for token in tokens {
        if !token.starts_with('%') {
            return Some(token.to_string());
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn discover_profiles_linux(spec: &KnownBrowser) -> Result<Vec<BrowserProfile>> {
    let Some(base) = directories::BaseDirs::new() else {
        return Ok(vec![]);
    };
    match spec.profile_kind {
        ProfileKind::None => Ok(vec![]),
        ProfileKind::Gecko => {
            let mut candidates = Vec::new();
            if let Some(relative) = spec.linux_gecko_dir {
                candidates.push(base.home_dir().join(relative).join("profiles.ini"));
            }
            if spec.id == "firefox" {
                candidates.push(
                    base.home_dir()
                        .join("snap/firefox/common/.mozilla/firefox/profiles.ini"),
                );
            }
            for path in candidates {
                let profiles = discover_gecko_profiles(&path)?;
                if !profiles.is_empty() {
                    return Ok(profiles);
                }
            }
            Ok(vec![])
        }
        ProfileKind::Chromium => {
            let mut candidates = Vec::new();
            if let Some(relative) = spec.linux_config_dir {
                candidates.push(base.config_dir().join(relative));
            }
            if spec.id == "chromium" {
                candidates.push(base.home_dir().join("snap/chromium/common/chromium"));
            }
            for path in candidates {
                let profiles = discover_chromium_profiles(&path)?;
                if !profiles.is_empty() {
                    return Ok(profiles);
                }
            }
            Ok(vec![])
        }
    }
}

#[cfg(target_os = "windows")]
fn discover_windows_cached(fresh: bool) -> Result<BrowserRegistry> {
    use crate::browser::cache;

    let start_menu = discover_windows::enumerate_start_menu_browsers()?;
    let fingerprint = cache::registry_fingerprint(&start_menu);

    if !fresh {
        if let Some(browsers) = cache::load(&fingerprint)? {
            return Ok(BrowserRegistry { browsers });
        }
    }

    let load_profiles = fresh;
    let mut browsers = HashMap::new();
    for spec in known_browsers() {
        if let Some(install) = discover_windows::discover_one(&spec, &start_menu, load_profiles)? {
            browsers.insert(spec.id.to_string(), install);
        }
    }

    cache::save(&fingerprint, &browsers)?;

    Ok(BrowserRegistry { browsers })
}

#[cfg(target_os = "macos")]
fn discover_browser_macos(spec: &KnownBrowser, app_path: &Path) -> Result<Option<BrowserInstall>> {
    let bundle_id =
        read_bundle_id(app_path).or_else(|| spec.mac_bundle_ids.first().map(|s| s.to_string()));
    let profiles = discover_profiles(spec, app_path)?;

    Ok(Some(BrowserInstall {
        id: spec.id.to_string(),
        display_name: spec.display_name.to_string(),
        app_path: Some(app_path.to_string_lossy().to_string()),
        bundle_id,
        profiles,
    }))
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

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
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

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
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
        assert_eq!(
            normalize_browser_id("Firefox Developer Edition"),
            "firefox-developer-edition"
        );
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

    #[test]
    fn maps_linux_desktop_ids() {
        assert_eq!(
            browser_id_for_desktop_id("firefox_firefox.desktop"),
            Some("firefox")
        );
        assert_eq!(
            browser_id_for_desktop_id("chromium_chromium.desktop"),
            Some("chromium")
        );
    }

    #[test]
    fn parses_snap_desktop_exec_lines() {
        assert_eq!(
            parse_desktop_exec_value(
                "env BAMF_DESKTOP_FILE_HINT=/var/lib/snapd/desktop/applications/firefox_firefox.desktop /snap/bin/firefox %u"
            ),
            Some("/snap/bin/firefox".to_string())
        );
        assert_eq!(
            parse_desktop_exec_value("google-chrome-stable %U"),
            Some("google-chrome-stable".to_string())
        );
    }
}
