use crate::browser::registry::BrowserInstall;
use crate::config;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const CACHE_FILE: &str = "browsers.json";

#[derive(Serialize, Deserialize)]
struct CachedRegistry {
    fingerprint: String,
    browsers: HashMap<String, BrowserInstall>,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn registry_fingerprint(entries: &[(String, String, String)]) -> String {
    let mut hasher = Sha256::new();
    for (key, _name, command) in entries {
        hasher.update(key.as_bytes());
        hasher.update(command.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Build a macOS discovery fingerprint from the set of discovered app bundles.
///
/// Each entry is `(app_path, info_plist_mtime)`. The mtime of
/// `Contents/Info.plist` changes whenever an app is installed, updated, or
/// reinstalled, so including it invalidates the cache on the exact events we
/// care about while keeping the snapshot cheap (a single `stat` per bundle).
pub fn macos_fingerprint(entries: &[(String, std::time::SystemTime)]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort();
    let mut hasher = Sha256::new();
    for (path, mtime) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(format!("{:?}", mtime).as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Load a cached registry only if its fingerprint still matches the current
/// start-menu snapshot. A mismatch (browser installed/uninstalled, command
/// path changed) invalidates the cache so discovery runs again.
///
/// Corrupt or unreadable cache files are treated as a miss so discovery can
/// repopulate them; a bad cache must never block default-browser launches.
///
/// **Security:** callers must not launch using `app_path` from this map alone.
/// Use [`rebind_app_paths`] (or equivalent) so executable paths always come
/// from live OS discovery, not the user-writable cache file.
pub fn load(expected_fingerprint: &str) -> Result<Option<HashMap<String, BrowserInstall>>> {
    let path = cache_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let cached: CachedRegistry = match serde_json::from_str(&content) {
        Ok(cached) => cached,
        Err(_) => {
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
    };
    if cached.fingerprint != expected_fingerprint {
        return Ok(None);
    }
    Ok(Some(cached.browsers))
}

/// Merge live-discovered launch paths over a cache hit.
///
/// The browser cache lives in the user config directory and is not integrity-
/// protected. A poisoned `app_path` would otherwise become the executable used
/// for every default-browser open. Profiles and display metadata may still
/// come from cache; **every** returned `app_path` is taken from `live`.
///
/// `live` is the authoritative map of browser id → install from current OS
/// discovery (paths only need be correct; profiles may be empty).
///
/// Returns `None` if `live` is empty (nothing to launch).
pub fn rebind_app_paths(
    cached: HashMap<String, BrowserInstall>,
    live: HashMap<String, BrowserInstall>,
) -> Option<HashMap<String, BrowserInstall>> {
    if live.is_empty() {
        return None;
    }
    let mut out = HashMap::new();
    for (id, live_install) in live {
        let Some(live_path) = live_install.app_path.clone() else {
            // No executable path from live discovery — skip; never fall back to cache path.
            continue;
        };
        if let Some(mut cached_install) = cached.get(&id).cloned() {
            cached_install.id = id.clone();
            cached_install.app_path = Some(live_path);
            // Prefer live display name when present (start-menu / bundle may rename).
            if !live_install.display_name.is_empty() {
                cached_install.display_name = live_install.display_name;
            }
            if live_install.bundle_id.is_some() {
                cached_install.bundle_id = live_install.bundle_id;
            }
            // Keep cached profiles (expensive); live may have skipped profile scan.
            out.insert(id, cached_install);
        } else {
            out.insert(id, live_install);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn save(fingerprint: &str, browsers: &HashMap<String, BrowserInstall>) -> Result<()> {
    let path = cache_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = CachedRegistry {
        fingerprint: fingerprint.to_string(),
        browsers: browsers.clone(),
    };
    let data = serde_json::to_string(&payload)?;
    // Atomic replace so a crash mid-write cannot leave a truncated JSON file.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data)?;
    crate::config::restrict_file(&tmp);
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp, &path)?;
    crate::config::restrict_file(&path);
    Ok(())
}

fn cache_path() -> Result<PathBuf> {
    Ok(config::cache_dir()?.join(CACHE_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn macos_fingerprint_is_order_independent() {
        let t = SystemTime::UNIX_EPOCH;
        let a = vec![
            ("/Applications/Chrome.app".to_string(), t),
            ("/Applications/Firefox.app".to_string(), t),
        ];
        let b = vec![
            ("/Applications/Firefox.app".to_string(), t),
            ("/Applications/Chrome.app".to_string(), t),
        ];
        assert_eq!(macos_fingerprint(&a), macos_fingerprint(&b));
    }

    #[test]
    fn macos_fingerprint_changes_on_mtime() {
        let path = "/Applications/Chrome.app".to_string();
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + Duration::from_secs(60);
        assert_ne!(
            macos_fingerprint(&[(path.clone(), t0)]),
            macos_fingerprint(&[(path, t1)])
        );
    }

    #[test]
    fn macos_fingerprint_changes_on_path_set() {
        let t = SystemTime::UNIX_EPOCH;
        let only_chrome = vec![("/Applications/Chrome.app".to_string(), t)];
        let with_firefox = vec![
            ("/Applications/Chrome.app".to_string(), t),
            ("/Applications/Firefox.app".to_string(), t),
        ];
        assert_ne!(
            macos_fingerprint(&only_chrome),
            macos_fingerprint(&with_firefox)
        );
    }

    #[test]
    fn macos_fingerprint_empty_is_stable() {
        assert_eq!(macos_fingerprint(&[]), macos_fingerprint(&[]));
    }

    #[test]
    fn rebind_app_paths_never_keeps_cached_executable() {
        let mut cached = HashMap::new();
        cached.insert(
            "chrome".to_string(),
            BrowserInstall {
                id: "chrome".to_string(),
                display_name: "Google Chrome".to_string(),
                app_path: Some("/tmp/evil-chrome".to_string()),
                bundle_id: Some("com.google.Chrome".to_string()),
                profiles: vec![],
            },
        );
        let mut live = HashMap::new();
        live.insert(
            "chrome".to_string(),
            BrowserInstall {
                id: "chrome".to_string(),
                display_name: "Google Chrome".to_string(),
                app_path: Some("/Applications/Google Chrome.app".to_string()),
                bundle_id: Some("com.google.Chrome".to_string()),
                profiles: vec![],
            },
        );
        let merged = rebind_app_paths(cached, live).expect("merge");
        assert_eq!(
            merged["chrome"].app_path.as_deref(),
            Some("/Applications/Google Chrome.app")
        );
    }

    #[test]
    fn rebind_app_paths_preserves_cached_profiles() {
        use crate::browser::registry::BrowserProfile;
        let mut cached = HashMap::new();
        cached.insert(
            "chrome".to_string(),
            BrowserInstall {
                id: "chrome".to_string(),
                display_name: "Google Chrome".to_string(),
                app_path: Some("/tmp/evil".to_string()),
                bundle_id: None,
                profiles: vec![BrowserProfile {
                    name: "Work".to_string(),
                    directory: Some("Profile 1".to_string()),
                    path: None,
                }],
            },
        );
        let mut live = HashMap::new();
        live.insert(
            "chrome".to_string(),
            BrowserInstall {
                id: "chrome".to_string(),
                display_name: "Google Chrome".to_string(),
                app_path: Some("/opt/google/chrome".to_string()),
                bundle_id: None,
                profiles: vec![],
            },
        );
        let merged = rebind_app_paths(cached, live).unwrap();
        assert_eq!(merged["chrome"].profiles.len(), 1);
        assert_eq!(merged["chrome"].profiles[0].name, "Work");
        assert_eq!(
            merged["chrome"].app_path.as_deref(),
            Some("/opt/google/chrome")
        );
    }
}
