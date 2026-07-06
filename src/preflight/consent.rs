use super::consent_guard::{self, ConsentProtection};
use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConsentFile {
    hosts: BTreeSet<String>,
}

pub fn consent_path() -> PathBuf {
    consent_guard::guarded_consent_path()
}

pub fn protection_status() -> Result<ConsentProtection> {
    consent_guard::protection_status()
}

pub fn list_allowed_hosts() -> Result<Vec<String>> {
    let file = load()?;
    Ok(file.hosts.into_iter().collect())
}

pub fn is_allowed(host: &str) -> Result<bool> {
    let normalized = normalize_host(host)?;
    let file = load()?;
    Ok(file.hosts.contains(&normalized))
}

pub fn allow_host(host: &str) -> Result<()> {
    let normalized = normalize_host(host)?;
    let mut file = load()?;
    if file.hosts.insert(normalized.clone()) {
        save(&file)?;
        println!("Allowed preflight resolve for {normalized} (root-owned allowlist)");
    } else {
        println!("{normalized} is already allowed for preflight resolve");
    }
    Ok(())
}

pub fn revoke_host(host: &str) -> Result<()> {
    let normalized = normalize_host(host)?;
    let mut file = load()?;
    if file.hosts.remove(&normalized) {
        save(&file)?;
        println!("Revoked preflight resolve for {normalized}");
    } else {
        println!("{normalized} was not in the preflight resolve allowlist");
    }
    Ok(())
}

fn load() -> Result<ConsentFile> {
    let path = consent_path();
    if consent_guard::protection_status()? != ConsentProtection::Guarded {
        return Ok(ConsentFile::default());
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn save(file: &ConsentFile) -> Result<()> {
    let path = consent_path();
    let raw = serde_json::to_string_pretty(file)?;
    consent_guard::save_guarded(&path, &raw)
}

pub fn normalize_host(host: &str) -> Result<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        bail!("hostname must not be empty");
    }
    if host.contains('/') || host.contains(':') || host.contains('*') {
        bail!("hostname must not contain /, :, or *");
    }
    if host.starts_with('.') {
        bail!("hostname must not start with '.'");
    }
    Ok(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_host_rejects_wildcards() {
        assert!(normalize_host("*.example.com").is_err());
    }

    #[test]
    fn normalize_host_lowercases() {
        assert_eq!(
            normalize_host("Mail.Example.com").unwrap(),
            "mail.example.com"
        );
    }
}
