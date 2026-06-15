use anyhow::Result;
use std::path::{Path, PathBuf};
use url::Url;

pub fn is_routable_input(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
        || trimmed.starts_with("~/")
        || Path::new(trimmed).is_absolute()
        || is_windows_drive_path(trimmed)
        || is_unc_path(trimmed)
}

fn is_windows_drive_path(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 3
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && bytes[0].is_ascii_alphabetic()
}

fn is_unc_path(raw: &str) -> bool {
    raw.starts_with(r"\\") || raw.starts_with("//")
}

fn is_absolute_path(path: &Path, raw: &str) -> bool {
    path.is_absolute() || is_windows_drive_path(raw) || is_unc_path(raw)
}

pub fn normalize_input_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
    {
        return Ok(trimmed.to_string());
    }

    if let Some(file_url) = file_path_to_url(trimmed)? {
        return Ok(file_url);
    }

    Ok(trimmed.to_string())
}

fn file_path_to_url(raw: &str) -> Result<Option<String>> {
    let expanded = expand_tilde(raw);
    let path = Path::new(&expanded);
    if !is_absolute_path(path, raw) {
        return Ok(None);
    }

    if !path.exists() && !is_html_path(path) {
        return Ok(None);
    }

    let usable_path = path
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&expanded));
    if !usable_path.exists() {
        anyhow::bail!("file not found: {}", usable_path.display());
    }
    if !usable_path.is_file() {
        anyhow::bail!("not a file: {}", usable_path.display());
    }

    let url = Url::from_file_path(&usable_path)
        .map_err(|()| anyhow::anyhow!("invalid file path: {}", usable_path.display()))?;
    Ok(Some(url.to_string()))
}

fn expand_tilde(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("~/") else {
        return raw.to_string();
    };
    directories::UserDirs::new()
        .map(|dirs| dirs.home_dir().join(rest).to_string_lossy().into_owned())
        .unwrap_or_else(|| raw.to_string())
}

fn is_html_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("html") | Some("htm") | Some("xhtml")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_routable_inputs() {
        assert!(is_routable_input("https://example.com"));
        assert!(is_routable_input("file:///tmp/a.html"));
        assert!(is_routable_input("/tmp/a.html"));
        assert!(is_routable_input("~/report.html"));
        assert!(!is_routable_input("register"));
    }

    #[test]
    fn normalizes_absolute_file_paths() {
        let dir = std::env::temp_dir();
        let file = dir.join("supersurfer-test.html");
        std::fs::write(&file, "<html></html>").unwrap();
        let normalized = normalize_input_url(file.to_str().unwrap()).unwrap();
        assert!(normalized.starts_with("file://"));
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn detects_windows_paths() {
        assert!(is_routable_input(r"C:\Users\jan\report.html"));
        assert!(is_routable_input(r"\\server\share\report.html"));
        assert!(is_windows_drive_path(r"C:/Users/jan/report.html"));
    }
}
