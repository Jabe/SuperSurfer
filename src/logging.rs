use anyhow::{Context as _, Result};
use directories::ProjectDirs;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub fn log_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "SuperSurfer")
        .context("could not resolve SuperSurfer config directory")?;
    let dir = dirs.data_local_dir().join("logs");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn log_file() -> Result<PathBuf> {
    Ok(log_dir()?.join("decisions.log"))
}

pub fn append_decision(line: &str) -> Result<()> {
    let path = log_file()?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown-time".to_string());
    writeln!(file, "{timestamp} {line}")?;
    Ok(())
}

pub fn tail_logs(lines: usize) -> Result<()> {
    let path = log_file()?;
    if !path.exists() {
        println!("No log file yet at {}", path.display());
        return Ok(());
    }
    let content = fs::read_to_string(&path)?;
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(lines);
    for line in &all[start..] {
        println!("{line}");
    }
    Ok(())
}
