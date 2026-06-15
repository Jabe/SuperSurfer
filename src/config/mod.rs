use anyhow::{Context as _, Result};
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};

pub mod cache;
pub mod loader;
pub mod scaffold;
pub mod transpile;

pub fn config_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "SuperSurfer")
        .context("could not resolve SuperSurfer config directory")?;
    let dir = dirs.config_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn config_path() -> Result<PathBuf> {
    let dir = config_dir()?;
    for name in ["config.ts", "config.js"] {
        let path = dir.join(name);
        if path.exists() {
            return Ok(path);
        }
    }
    Ok(dir.join("config.ts"))
}

pub fn cache_dir() -> Result<PathBuf> {
    let dir = config_dir()?.join("cache");
    fs::create_dir_all(&dir)?;
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
    let config = dir.join("config.ts");
    let types = types_path()?;

    if config.exists() && !force {
        anyhow::bail!(
            "config already exists at {}. Use --force to overwrite.",
            config.display()
        );
    }

    fs::write(&types, types_stub())?;
    let plan = scaffold::plan()?;
    fs::write(&config, scaffold::render(&plan))?;
    Ok((config, plan))
}

pub fn read_config_source(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}
