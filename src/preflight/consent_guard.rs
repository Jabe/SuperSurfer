use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentProtection {
    /// Root-owned system file; requires elevation to modify.
    Guarded,
    /// No root-owned allowlist present.
    Missing,
}

pub fn guarded_consent_path() -> PathBuf {
    platform::guarded_consent_path()
}

pub fn save_guarded(path: &Path, contents: &str) -> Result<()> {
    platform::save_guarded_consent(path, contents)
}

pub fn protection_status() -> Result<ConsentProtection> {
    platform::consent_protection_status()
}

mod platform {
    use super::*;

    pub fn guarded_consent_path() -> PathBuf {
        guarded_consent_path_impl()
    }

    pub fn save_guarded_consent(path: &Path, contents: &str) -> Result<()> {
        save_guarded_consent_impl(path, contents)
    }

    pub fn consent_protection_status() -> Result<ConsentProtection> {
        consent_protection_status_impl()
    }

    #[cfg(target_os = "macos")]
    fn guarded_consent_path_impl() -> PathBuf {
        PathBuf::from("/Library/Application Support/SuperSurfer/resolve-allowed-hosts.json")
    }

    #[cfg(target_os = "linux")]
    fn guarded_consent_path_impl() -> PathBuf {
        PathBuf::from("/etc/supersurfer/resolve-allowed-hosts.json")
    }

    #[cfg(target_os = "windows")]
    fn guarded_consent_path_impl() -> PathBuf {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("SuperSurfer")
            .join("resolve-allowed-hosts.json")
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn guarded_consent_path_impl() -> PathBuf {
        PathBuf::from("/etc/supersurfer/resolve-allowed-hosts.json")
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn save_guarded_consent_impl(path: &Path, contents: &str) -> Result<()> {
        use std::fs;
        use std::process::Command;

        let parent = path
            .parent()
            .context("guarded consent path has no parent directory")?;
        let tmp = std::env::temp_dir().join(format!(
            "supersurfer-consent-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::write(&tmp, contents).with_context(|| format!("failed to write {}", tmp.display()))?;

        let parent_arg = parent.to_string_lossy().into_owned();
        let path_arg = path.to_string_lossy().into_owned();
        let tmp_arg = tmp.to_string_lossy().into_owned();

        let status = Command::new("sudo")
            .args([
                "sh",
                "-c",
                &format!(
                    "mkdir -p {parent_q} && install -m 644 -o root -g {} {tmp_q} {path_q}",
                    if cfg!(target_os = "macos") {
                        "wheel"
                    } else {
                        "root"
                    },
                    parent_q = shell_quote(&parent_arg),
                    tmp_q = shell_quote(&tmp_arg),
                    path_q = shell_quote(&path_arg),
                ),
            ])
            .status()
            .context("failed to run sudo (is sudo available?)")?;

        let _ = fs::remove_file(&tmp);

        if !status.success() {
            anyhow::bail!(
                "sudo failed — preflight allowlist must be root-owned.\n\
                 Re-run this command and enter your password when prompted."
            );
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn save_guarded_consent_impl(path: &Path, contents: &str) -> Result<()> {
        use std::fs;
        use std::process::Command;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create {} — run an elevated terminal",
                    parent.display()
                )
            })?;
        }

        fs::write(path, contents).with_context(|| {
            format!(
                "failed to write {} — run `supersurfer resolve allow` from an elevated terminal",
                path.display()
            )
        })?;

        let path_arg = path.to_string_lossy().into_owned();
        let status = Command::new("icacls")
            .args([
                &path_arg,
                "/inheritance:r",
                "/grant:r",
                "Administrators:F",
                "SYSTEM:F",
                "Users:R",
            ])
            .status()
            .context("failed to run icacls to harden allowlist ACLs")?;

        if !status.success() {
            anyhow::bail!(
                "icacls failed to harden {} — allowlist would remain user-writable.\n\
                 Re-run `supersurfer resolve allow` from an elevated terminal.",
                path.display()
            );
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn save_guarded_consent_impl(path: &Path, contents: &str) -> Result<()> {
        use std::fs;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
        Ok(())
    }

    #[cfg(unix)]
    fn consent_protection_status_impl() -> Result<ConsentProtection> {
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        let guarded = guarded_consent_path_impl();
        if !guarded.exists() {
            return Ok(ConsentProtection::Missing);
        }
        let meta = fs::metadata(&guarded)?;
        // Root-owned alone is not enough: reject group/other-writable files so a
        // mis-chmod'd allowlist is not treated as a protected SSRF consent source.
        if meta.uid() == 0 && meta.mode() & 0o022 == 0 {
            Ok(ConsentProtection::Guarded)
        } else {
            Ok(ConsentProtection::Missing)
        }
    }

    #[cfg(windows)]
    fn consent_protection_status_impl() -> Result<ConsentProtection> {
        use std::fs;

        let guarded = guarded_consent_path_impl();
        if guarded.exists() {
            Ok(ConsentProtection::Guarded)
        } else {
            Ok(ConsentProtection::Missing)
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn consent_protection_status_impl() -> Result<ConsentProtection> {
        Ok(ConsentProtection::Missing)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}
