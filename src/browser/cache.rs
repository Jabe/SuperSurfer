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

pub fn registry_fingerprint(entries: &[(String, String, String)]) -> String {
    let mut hasher = Sha256::new();
    for (key, _name, command) in entries {
        hasher.update(key.as_bytes());
        hasher.update(command.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub fn load() -> Result<Option<HashMap<String, BrowserInstall>>> {
    let path = cache_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    let cached: CachedRegistry = serde_json::from_str(&content)?;
    Ok(Some(cached.browsers))
}

pub fn save(fingerprint: &str, browsers: &HashMap<String, BrowserInstall>) -> Result<()> {
    let path = cache_path()?;
    let payload = CachedRegistry {
        fingerprint: fingerprint.to_string(),
        browsers: browsers.clone(),
    };
    fs::write(path, serde_json::to_string(&payload)?)?;
    Ok(())
}

fn cache_path() -> Result<PathBuf> {
    Ok(config::cache_dir()?.join(CACHE_FILE))
}
