use anyhow::{Context as _, Result};
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};

pub mod loader;
pub mod prepare;
pub mod scaffold;

pub fn config_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "SuperSurfer")
        .context("could not resolve SuperSurfer config directory")?;
    let dir = dirs.config_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    restrict_dir(&dir);
    Ok(dir)
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.js"))
}

pub fn cache_dir() -> Result<PathBuf> {
    let dir = config_dir()?.join("cache");
    fs::create_dir_all(&dir)?;
    restrict_dir(&dir);
    Ok(dir)
}

pub fn types_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("supersurfer.d.ts"))
}

pub fn types_stub() -> &'static str {
    include_str!("supersurfer.d.ts")
}

pub fn write_scaffold(force: bool) -> Result<(PathBuf, scaffold::ScaffoldPlan)> {
    let dir = config_dir()?;
    let config = dir.join("config.js");
    let types = types_path()?;

    if config.exists() && !force {
        anyhow::bail!(
            "config already exists at {}. Use --force to overwrite.",
            config.display()
        );
    }

    fs::write(&types, types_stub())?;
    restrict_file(&types);
    let plan = scaffold::plan()?;
    fs::write(&config, scaffold::render(&plan))?;
    restrict_file(&config);
    Ok((config, plan))
}

/// Decision logs and config contain full URLs (magic links, OAuth codes).
/// Keep them owner-only even when the home directory is traversable.
pub(crate) fn restrict_dir(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

pub(crate) fn restrict_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

pub fn read_config_source(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_dir_and_file_are_owner_only() {
        let dir = std::env::temp_dir().join(format!("supersurfer-perms-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        restrict_dir(&dir);
        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);

        let file = dir.join("decisions.log");
        fs::write(&file, "https://example.com/?token=secret").unwrap();
        restrict_file(&file);
        let file_mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }
}
