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
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp, &path)?;
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
}
