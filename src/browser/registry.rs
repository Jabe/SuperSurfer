use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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

struct KnownBrowser {
    id: &'static str,
    display_name: &'static str,
    mac_app_names: &'static [&'static str],
    mac_bundle_ids: &'static [&'static str],
}

fn known_browsers() -> Vec<KnownBrowser> {
    vec![
        KnownBrowser {
            id: "safari",
            display_name: "Safari",
            mac_app_names: &["Safari.app"],
            mac_bundle_ids: &["com.apple.Safari"],
        },
        KnownBrowser {
            id: "chrome",
            display_name: "Google Chrome",
            mac_app_names: &["Google Chrome.app"],
            mac_bundle_ids: &["com.google.Chrome"],
        },
        KnownBrowser {
            id: "firefox",
            display_name: "Firefox",
            mac_app_names: &["Firefox.app"],
            mac_bundle_ids: &["org.mozilla.firefox"],
        },
        KnownBrowser {
            id: "edge",
            display_name: "Microsoft Edge",
            mac_app_names: &["Microsoft Edge.app"],
            mac_bundle_ids: &["com.microsoft.edgemac"],
        },
        KnownBrowser {
            id: "brave",
            display_name: "Brave Browser",
            mac_app_names: &["Brave Browser.app"],
            mac_bundle_ids: &["com.brave.Browser"],
        },
        KnownBrowser {
            id: "arc",
            display_name: "Arc",
            mac_app_names: &["Arc.app"],
            mac_bundle_ids: &["company.thebrowser.Browser"],
        },
    ]
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

    let bundle_id = read_bundle_id(&app_path).or_else(|| spec.mac_bundle_ids.first().map(|s| s.to_string()));
    let profiles = discover_profiles(&spec.id, &app_path)?;

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
    let _ = spec;
    Ok(None)
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
fn discover_profiles(browser_id: &str, app_path: &Path) -> Result<Vec<BrowserProfile>> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("no home directory")?;
    match browser_id {
        "firefox" => discover_firefox_profiles(&home),
        "chrome" | "edge" | "brave" | "arc" => {
            discover_chromium_profiles(browser_id, &home, app_path)
        }
        _ => Ok(vec![]),
    }
}

#[cfg(target_os = "macos")]
fn discover_firefox_profiles(home: &Path) -> Result<Vec<BrowserProfile>> {
    let ini_path = home
        .join("Library/Application Support/Firefox/profiles.ini");
    if !ini_path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&ini_path)?;
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

#[cfg(target_os = "macos")]
fn discover_chromium_profiles(
    browser_id: &str,
    home: &Path,
    _app_path: &Path,
) -> Result<Vec<BrowserProfile>> {
    let support_dir = match browser_id {
        "chrome" => home.join("Library/Application Support/Google/Chrome"),
        "edge" => home.join("Library/Application Support/Microsoft Edge"),
        "brave" => home.join("Library/Application Support/BraveSoftware/Brave-Browser"),
        "arc" => home.join("Library/Application Support/Arc/User Data"),
        _ => return Ok(vec![]),
    };
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
