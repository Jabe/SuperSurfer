use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub fn cache_key(source: &str, path: &Path, helpers: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    if let Ok(meta) = fs::metadata(path) {
        if let Ok(mtime) = meta.modified() {
            hasher.update(format!("{mtime:?}").as_bytes());
        }
    }
    hasher.update(source.as_bytes());
    hasher.update(helpers.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn read_cached(cache_dir: &Path, key: &str) -> Result<Option<String>> {
    let path = cache_dir.join(format!("{key}.js"));
    if path.exists() {
        Ok(Some(fs::read_to_string(path)?))
    } else {
        Ok(None)
    }
}

pub fn write_cached(cache_dir: &Path, key: &str, js: &str) -> Result<()> {
    let path = cache_dir.join(format!("{key}.js"));
    fs::write(path, js)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn cache_key_changes_when_helpers_change() {
        let path = Path::new("/tmp/config.js");
        let source = "globalThis.__SUPERSURFER_CONFIG__ = {};";
        let a = cache_key(source, path, "helpers-v1");
        let b = cache_key(source, path, "helpers-v2");
        assert_ne!(a, b);
    }
}
