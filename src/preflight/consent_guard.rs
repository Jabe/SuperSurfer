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
        let guarded = guarded_consent_path_impl();
        if !guarded.exists() {
            return Ok(ConsentProtection::Missing);
        }
        // Existence alone is not enough: a user-writable ProgramData file must
        // not count as a protected SSRF allowlist. Require hardened ACLs.
        if windows_allowlist_acl_is_hardened(&guarded) {
            Ok(ConsentProtection::Guarded)
        } else {
            Ok(ConsentProtection::Missing)
        }
    }

    /// True when `icacls` shows no write/modify/full grant to broad principals
    /// (Users, Everyone, Authenticated Users) and at least one Admin/SYSTEM ACE.
    ///
    /// This matches what `save_guarded_consent_impl` applies:
    /// inheritance removed; Administrators/SYSTEM full; Users read-only.
    #[cfg(windows)]
    fn windows_allowlist_acl_is_hardened(path: &Path) -> bool {
        use std::process::Command;

        let output = match Command::new("icacls").arg(path.as_os_str()).output() {
            Ok(o) if o.status.success() => o,
            _ => return false,
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let lower = text.to_ascii_lowercase();

        let mut saw_admin_or_system = false;
        for line in lower.lines() {
            // icacls lines look like: `path BUILTIN\Administrators:(F)` or
            // indented `         BUILTIN\Users:(R)`.
            let is_admin = line.contains("administrators");
            let is_system = line.contains("nt authority\\system") || line.contains("\\system:");
            if is_admin || is_system {
                saw_admin_or_system = true;
            }

            let is_broad = line.contains("everyone")
                || line.contains("authenticated users")
                || line.contains("builtin\\users")
                || line.contains("\\users:");
            if is_broad && windows_ace_grants_write(line) {
                return false;
            }
        }
        saw_admin_or_system
    }

    /// Whether an icacls ACE line grants write/modify/full (not mere read/execute).
    #[cfg(any(windows, test))]
    pub(super) fn windows_ace_grants_write(ace_line: &str) -> bool {
        // Permissions appear as (F), (M), (W), (RX), (R), often combined like (OI)(CI)(F).
        // Avoid treating (R) as write: match known write-ish tokens inside parentheses.
        for part in ace_line.split('(').skip(1) {
            let token = part.split(')').next().unwrap_or("").trim();
            // Skip inheritance-only markers.
            if matches!(
                token,
                "oi" | "ci" | "io" | "np" | "i" | "r" | "x" | "rx" | "rd" | "rc" | "s" | "n"
            ) {
                continue;
            }
            if matches!(
                token,
                "f" | "m" | "w" | "d" | "wd" | "ad" | "dc" | "de" | "wea" | "wa" | "wdac" | "wo"
            ) {
                return true;
            }
            // Combined forms e.g. "r,w" or "M,RX"
            for sub in token.split(|c: char| c == ',' || c == ' ') {
                let s = sub.trim();
                if matches!(
                    s,
                    "f" | "m"
                        | "w"
                        | "d"
                        | "wd"
                        | "ad"
                        | "dc"
                        | "de"
                        | "wea"
                        | "wa"
                        | "wdac"
                        | "wo"
                ) {
                    return true;
                }
            }
        }
        false
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

#[cfg(test)]
mod tests {
    use super::platform::windows_ace_grants_write;

    #[test]
    fn users_read_only_is_not_write() {
        assert!(!windows_ace_grants_write(r"    builtin\users:(r)"));
        assert!(!windows_ace_grants_write(r"    builtin\users:(rx)"));
        assert!(!windows_ace_grants_write(r"    builtin\users:(oi)(ci)(rx)"));
    }

    #[test]
    fn users_full_or_modify_is_write() {
        assert!(windows_ace_grants_write(r"    builtin\users:(f)"));
        assert!(windows_ace_grants_write(r"    builtin\users:(m)"));
        assert!(windows_ace_grants_write(r"    everyone:(w)"));
        assert!(windows_ace_grants_write(
            r"    nt authority\authenticated users:(m)"
        ));
    }
}
