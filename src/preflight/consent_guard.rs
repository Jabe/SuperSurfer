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
        use sha2::{Digest, Sha256};
        use std::io::Write;
        use std::process::{Command, Stdio};

        let parent = path
            .parent()
            .context("guarded consent path has no parent directory")?;
        let digest = Sha256::digest(contents.as_bytes());
        let digest = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let script =
            consent_install_script(&digest, &parent.to_string_lossy(), &path.to_string_lossy());

        // The payload goes through sudo's stdin. A temp file in /tmp sits
        // there for the whole password prompt, and another process running as
        // the same user can replace it before root installs it.
        let mut child = Command::new("sudo")
            .args(["sh", "-c", &script])
            .stdin(Stdio::piped())
            .spawn()
            .context("failed to run sudo (is sudo available?)")?;
        {
            let mut stdin = child
                .stdin
                .take()
                .context("sudo did not provide stdin for the allowlist")?;
            stdin
                .write_all(contents.as_bytes())
                .context("failed to pass allowlist contents to sudo")?;
        }
        let status = child.wait().context("failed to wait for sudo")?;

        if !status.success() {
            anyhow::bail!(
                "sudo failed — preflight allowlist must be root-owned.\n\
                 Re-run this command and enter your password when prompted."
            );
        }
        Ok(())
    }

    /// Root shell that installs `contents` only if they still hash to `digest`.
    /// The digest is fixed in argv; swapped stdin fails the check and never
    /// reaches `install`.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(super) fn consent_install_script(digest: &str, parent: &str, path: &str) -> String {
        let group = if cfg!(target_os = "macos") {
            "wheel"
        } else {
            "root"
        };
        format!(
            r#"set -eu
payload=$(mktemp)
trap 'rm -f "$payload"' EXIT
cat > "$payload"
actual=$(if command -v sha256sum >/dev/null 2>&1; then sha256sum "$payload" | awk '{{print $1}}'; elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$payload" | awk '{{print $1}}'; else openssl dgst -sha256 "$payload" | awk '{{print $NF}}'; fi)
[ "$actual" = {digest_q} ]
if [ -L {parent_q} ]; then
  echo "refusing to install the allowlist through a symlink" >&2
  exit 1
fi
mkdir -p {parent_q}
chown root:{group} {parent_q}
chmod 755 {parent_q}
if [ -L {path_q} ]; then rm -f {path_q}; fi
install -m 644 -o root -g {group} "$payload" {path_q}
"#,
            digest_q = shell_quote(digest),
            parent_q = shell_quote(parent),
            path_q = shell_quote(path),
            group = group,
        )
    }

    // Well-known SIDs, used instead of account names because names are localized
    // ("Administrators" does not resolve on e.g. a German Windows). icacls accepts
    // SIDs with a `*` prefix; SDDL uses the two-letter aliases.
    #[cfg(target_os = "windows")]
    const SID_ADMINISTRATORS: &str = "S-1-5-32-544";
    #[cfg(target_os = "windows")]
    const SID_SYSTEM: &str = "S-1-5-18";
    #[cfg(target_os = "windows")]
    const SID_USERS: &str = "S-1-5-32-545";

    #[cfg(target_os = "windows")]
    fn save_guarded_consent_impl(path: &Path, contents: &str) -> Result<()> {
        use std::fs;

        let parent = path
            .parent()
            .context("guarded consent path has no parent directory")?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create {} — run an elevated terminal",
                parent.display()
            )
        })?;

        // A new ProgramData directory is writable by Users until icacls runs.
        // Lock the directory first, then write. Writing first lets another
        // process swap the file and have this command seal their copy.
        harden_windows_acl(parent, "(OI)(CI)RX")?;
        if let Ok(meta) = fs::symlink_metadata(path) {
            if meta.file_type().is_symlink() {
                fs::remove_file(path).with_context(|| {
                    format!("failed to remove allowlist symlink {}", path.display())
                })?;
            } else {
                harden_windows_acl(path, "R")?;
            }
        }
        fs::write(path, contents).with_context(|| {
            format!(
                "failed to write {} — run `supersurfer resolve allow` from an elevated terminal",
                path.display()
            )
        })?;
        harden_windows_acl(path, "R")?;
        let written = fs::read_to_string(path)
            .with_context(|| format!("failed to read back {}", path.display()))?;
        if written != contents {
            anyhow::bail!("allowlist contents changed while saving {}", path.display());
        }
        Ok(())
    }

    /// Set and verify: owner Administrators, no inheritance, Administrators and
    /// SYSTEM full, Users limited to `users_grant`, and no other write grants.
    #[cfg(target_os = "windows")]
    fn harden_windows_acl(target: &Path, users_grant: &str) -> Result<()> {
        use std::process::Command;

        let target_arg = target.to_string_lossy().into_owned();
        let steps = [
            vec![
                target_arg.clone(),
                "/setowner".into(),
                format!("*{SID_ADMINISTRATORS}"),
            ],
            vec![
                target_arg,
                "/inheritance:r".into(),
                "/grant:r".into(),
                format!("*{SID_ADMINISTRATORS}:F"),
                format!("*{SID_SYSTEM}:F"),
                format!("*{SID_USERS}:{users_grant}"),
            ],
        ];
        for args in steps {
            let status = Command::new("icacls")
                .args(args)
                .status()
                .context("failed to run icacls to harden allowlist ACLs")?;
            if !status.success() {
                anyhow::bail!(
                    "icacls failed to harden {} — allowlist would remain user-writable.\n\
                     Re-run `supersurfer resolve allow` from an elevated terminal.",
                    target.display()
                );
            }
        }
        let hardened = read_owner_dacl_sddl(target)
            .map(|sddl| sddl_is_hardened(&sddl))
            .unwrap_or(false);
        if !hardened {
            anyhow::bail!(
                "{} still has an unsafe owner or ACL after icacls hardening",
                target.display()
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
        // Follow no symlinks. `metadata` would treat a link to a root-owned
        // file as guarded, and a writable parent can swap that link after the
        // check.
        let Ok(meta) = fs::symlink_metadata(&guarded) else {
            return Ok(ConsentProtection::Missing);
        };
        let Some(parent) = guarded.parent() else {
            return Ok(ConsentProtection::Missing);
        };
        let Ok(parent_meta) = fs::symlink_metadata(parent) else {
            return Ok(ConsentProtection::Missing);
        };
        let file_ok = meta.file_type().is_file() && meta.uid() == 0 && meta.mode() & 0o022 == 0;
        let parent_ok =
            parent_meta.is_dir() && parent_meta.uid() == 0 && parent_meta.mode() & 0o022 == 0;
        if file_ok && parent_ok {
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
        // Existence alone is not enough: a user-writable ProgramData file or
        // parent directory must not count as a protected SSRF allowlist. Require
        // hardened owner/DACL pairs, read as SDDL so the check is locale-independent
        // (icacls prints localized names, e.g. "VORDEFINIERT\Administratoren").
        let hardened = guarded
            .parent()
            .and_then(|parent| {
                let file_sddl = read_owner_dacl_sddl(&guarded)?;
                let parent_sddl = read_owner_dacl_sddl(parent)?;
                Some(consent_sddls_are_hardened(&file_sddl, &parent_sddl))
            })
            .unwrap_or(false);
        if hardened {
            Ok(ConsentProtection::Guarded)
        } else {
            Ok(ConsentProtection::Missing)
        }
    }

    /// Read a file's owner + DACL as an SDDL string via the security APIs.
    #[cfg(windows)]
    fn read_owner_dacl_sddl(path: &Path) -> Option<String> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
            SDDL_REVISION_1, SE_FILE_OBJECT,
        };
        use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let mut descriptor: *mut core::ffi::c_void = core::ptr::null_mut();
        let status = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                info,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 || descriptor.is_null() {
            return None;
        }
        let mut sddl_ptr: windows_sys::core::PWSTR = core::ptr::null_mut();
        let mut sddl_len: u32 = 0;
        let ok = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                info,
                &mut sddl_ptr,
                &mut sddl_len,
            )
        };
        let sddl = if ok != 0 && !sddl_ptr.is_null() {
            let chars = unsafe { std::slice::from_raw_parts(sddl_ptr, sddl_len as usize) };
            let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
            Some(String::from_utf16_lossy(&chars[..end]))
        } else {
            None
        };
        unsafe {
            if !sddl_ptr.is_null() {
                LocalFree(sddl_ptr.cast());
            }
            LocalFree(descriptor);
        }
        sddl
    }

    /// Allow-list check of an owner+DACL SDDL string, matching what
    /// `save_guarded_consent_impl` applies: owner Administrators (or SYSTEM),
    /// protected DACL, and write access only for Administrators/SYSTEM.
    ///
    /// Fails closed: any unrecognized owner, ACE type, flag, or rights token
    /// counts as NOT hardened. The owner matters because a file's owner holds
    /// implicit WRITE_DAC and could re-grant themselves access at any time.
    #[cfg(any(windows, test))]
    pub(super) fn consent_sddls_are_hardened(file_sddl: &str, parent_sddl: &str) -> bool {
        sddl_is_hardened(file_sddl) && sddl_is_hardened(parent_sddl)
    }

    #[cfg(any(windows, test))]
    pub(super) fn sddl_is_hardened(sddl: &str) -> bool {
        let sddl = sddl.to_ascii_uppercase();
        let owner = match sddl_component(&sddl, "O:") {
            Some(owner) => owner,
            None => return false,
        };
        if !sddl_sid_is_admin_or_system(&owner) {
            return false;
        }
        let dacl = match sddl_component(&sddl, "D:") {
            Some(dacl) => dacl,
            None => return false,
        };
        let (flags, aces) = split_dacl(&dacl);
        // 'P' = protected: no ACEs inherited from the parent directory.
        if !flags.contains('P') {
            return false;
        }

        let mut saw_admin_or_system_grant = false;
        for ace in aces {
            let fields: Vec<&str> = ace.split(';').collect();
            if fields.len() < 6 {
                return false;
            }
            let (ace_type, ace_flags, rights, sid) = (fields[0], fields[1], fields[2], fields[5]);
            match ace_type {
                "A" => {}
                // Deny ACEs only ever restrict access; audit/callback/unknown
                // types mean we no longer understand the policy — fail closed.
                "D" => continue,
                _ => return false,
            }
            if ace_flags.contains("ID") {
                return false;
            }
            let mask = match sddl_rights_mask(rights) {
                Some(mask) => mask,
                None => return false,
            };
            if mask & SDDL_WRITE_MASK != 0 {
                if !sddl_sid_is_admin_or_system(sid) {
                    return false;
                }
                saw_admin_or_system_grant = true;
            }
        }
        saw_admin_or_system_grant
    }

    /// Access-mask bits that allow changing the file's content or its policy:
    /// FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_EA | FILE_WRITE_ATTRIBUTES
    /// | DELETE | WRITE_DAC | WRITE_OWNER | GENERIC_ALL | GENERIC_WRITE.
    /// DELETE counts because delete-and-recreate replaces the file's contents.
    #[cfg(any(windows, test))]
    const SDDL_WRITE_MASK: u32 = 0x2
        | 0x4
        | 0x10
        | 0x100
        | 0x0001_0000
        | 0x0004_0000
        | 0x0008_0000
        | 0x1000_0000
        | 0x4000_0000;

    #[cfg(any(windows, test))]
    fn sddl_sid_is_admin_or_system(sid: &str) -> bool {
        // BA/SY are the SDDL aliases for the SIDs below; which form appears
        // depends on the converter, so accept both.
        matches!(sid, "BA" | "SY" | "S-1-5-32-544" | "S-1-5-18")
    }

    /// Extract one SDDL component (e.g. everything after "O:" up to the next
    /// top-level "O:"/"G:"/"D:"/"S:" tag).
    #[cfg(any(windows, test))]
    fn sddl_component(sddl: &str, tag: &str) -> Option<String> {
        let start = sddl.find(tag)? + tag.len();
        let rest = &sddl[start..];
        let end = ["O:", "G:", "D:", "S:"]
            .iter()
            .filter_map(|marker| rest.find(marker))
            .min()
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }

    /// Split a DACL component into its control flags (before the first ACE)
    /// and the parenthesized ACE bodies.
    #[cfg(any(windows, test))]
    fn split_dacl(dacl: &str) -> (String, Vec<String>) {
        let flags_end = dacl.find('(').unwrap_or(dacl.len());
        let flags = dacl[..flags_end].to_string();
        let mut aces = Vec::new();
        let mut rest = &dacl[flags_end..];
        while let Some(open) = rest.find('(') {
            match rest[open..].find(')') {
                Some(close) => {
                    aces.push(rest[open + 1..open + close].to_string());
                    rest = &rest[open + close + 1..];
                }
                None => break,
            }
        }
        (flags, aces)
    }

    /// Convert an SDDL rights field ("FA", "0x1200a9", "GRGX", …) to an access
    /// mask. Returns None for any unrecognized token so callers fail closed.
    #[cfg(any(windows, test))]
    fn sddl_rights_mask(rights: &str) -> Option<u32> {
        if let Some(hex) = rights.strip_prefix("0X") {
            return u32::from_str_radix(hex, 16).ok();
        }
        if !rights.len().is_multiple_of(2) {
            return None;
        }
        let mut mask = 0u32;
        for chunk in rights.as_bytes().chunks(2) {
            mask |= match std::str::from_utf8(chunk).ok()? {
                "GA" => 0x1000_0000,
                "GX" => 0x2000_0000,
                "GW" => 0x4000_0000,
                "GR" => 0x8000_0000,
                "RC" => 0x0002_0000,
                "SD" => 0x0001_0000,
                "WD" => 0x0004_0000,
                "WO" => 0x0008_0000,
                "FA" => 0x001F_01FF,
                "FR" => 0x0012_0089,
                "FW" => 0x0012_0116,
                "FX" => 0x0012_00A0,
                "KA" => 0x000F_003F,
                "KR" | "KX" => 0x0002_0019,
                "KW" => 0x0002_0006,
                "CC" => 0x0001,
                "DC" => 0x0002,
                "LC" => 0x0004,
                "SW" => 0x0008,
                "RP" => 0x0010,
                "WP" => 0x0020,
                "DT" => 0x0040,
                "LO" => 0x0080,
                "CR" => 0x0100,
                _ => return None,
            };
        }
        Some(mask)
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
    use super::platform::{consent_sddls_are_hardened, sddl_is_hardened};

    #[cfg(unix)]
    #[test]
    fn consent_install_script_checks_hash_before_install() {
        let script = super::platform::consent_install_script(
            "abc123",
            "/etc/supersurfer",
            "/etc/supersurfer/resolve-allowed-hosts.json",
        );
        assert!(script.starts_with("set -eu\n"));
        assert!(script.contains("[ \"$actual\" = 'abc123' ]"));
        let check_at = script.find("[ \"$actual\"").unwrap();
        let install_at = script.find("install -m 644").unwrap();
        assert!(check_at < install_at);
        assert!(script.contains("if [ -L '/etc/supersurfer' ]"));
        assert!(!script.contains("evil.example"));
    }

    // What save_guarded_consent_impl produces: Administrators owner, protected
    // DACL, Admins/SYSTEM full, Users read.
    const HARDENED: &str = "O:BAG:SYD:PAI(A;;FA;;;BA)(A;;FA;;;SY)(A;;FR;;;BU)";
    const HARDENED_PARENT: &str = "O:BAG:SYD:PAI(A;;FA;;;BA)(A;;FA;;;SY)(A;OICI;0x1200a9;;;BU)";

    #[test]
    fn hardened_descriptor_is_accepted() {
        assert!(sddl_is_hardened(HARDENED));
        // Raw-SID spellings and a SYSTEM owner are equivalent.
        assert!(sddl_is_hardened(
            "O:S-1-5-32-544D:P(A;;FA;;;S-1-5-32-544)(A;;FA;;;S-1-5-18)(A;;FR;;;S-1-5-32-545)"
        ));
        assert!(sddl_is_hardened("O:SYD:P(A;;FA;;;SY)"));
        // Lowercase output from a converter must parse the same way.
        assert!(sddl_is_hardened(&HARDENED.to_ascii_lowercase()));
    }

    #[test]
    fn consent_requires_hardened_file_and_parent() {
        assert!(consent_sddls_are_hardened(HARDENED, HARDENED_PARENT));
        assert!(!consent_sddls_are_hardened(
            HARDENED,
            "O:BAD:P(A;;FA;;;BA)(A;;FA;;;SY)(A;;FA;;;S-1-5-21-1-2-3-1001)"
        ));
        assert!(!consent_sddls_are_hardened(
            "O:BAD:P(A;;FA;;;BA)(A;;FA;;;SY)(A;;FW;;;BU)",
            HARDENED_PARENT
        ));
    }

    #[test]
    fn non_admin_owner_is_rejected() {
        // The owner holds implicit WRITE_DAC even without a matching ACE.
        assert!(!sddl_is_hardened(
            "O:S-1-5-21-1-2-3-1001D:PAI(A;;FA;;;BA)(A;;FA;;;SY)(A;;FR;;;BU)"
        ));
        assert!(!sddl_is_hardened("D:PAI(A;;FA;;;BA)(A;;FA;;;SY)"));
    }

    #[test]
    fn named_user_write_ace_is_rejected() {
        // The reported hole: Alice:(F) alongside the expected principals.
        assert!(!sddl_is_hardened(
            "O:BAD:PAI(A;;FA;;;S-1-5-21-1-2-3-1001)(A;;FA;;;SY)(A;;FR;;;BU)"
        ));
    }

    #[test]
    fn broad_write_grants_are_rejected() {
        for sid in ["WD", "BU", "AU"] {
            let sddl = format!("O:BAD:P(A;;FA;;;BA)(A;;FA;;;SY)(A;;FW;;;{sid})");
            assert!(!sddl_is_hardened(&sddl), "{sid} write must reject");
        }
        // DELETE alone is enough to replace the file (delete + recreate).
        assert!(!sddl_is_hardened("O:BAD:P(A;;FA;;;BA)(A;;SD;;;BU)"));
        assert!(!sddl_is_hardened("O:BAD:P(A;;FA;;;BA)(A;;WDWO;;;BU)"));
    }

    #[test]
    fn unprotected_dacl_is_rejected() {
        // Without 'P' the parent directory's ACEs apply on top of these.
        assert!(!sddl_is_hardened("O:BAD:AI(A;;FA;;;BA)(A;;FA;;;SY)"));
        assert!(!sddl_is_hardened("O:BA"));
    }

    #[test]
    fn inherited_or_exotic_aces_fail_closed() {
        assert!(!sddl_is_hardened("O:BAD:P(A;ID;FA;;;BA)(A;;FA;;;SY)"));
        // Conditional/callback ACE types are not understood — fail closed.
        assert!(!sddl_is_hardened("O:BAD:P(XA;;FA;;;BA;(TRUE))(A;;FA;;;SY)"));
    }

    #[test]
    fn deny_aces_do_not_grant() {
        assert!(sddl_is_hardened(
            "O:BAD:P(A;;FA;;;BA)(A;;FA;;;SY)(D;;FA;;;WD)"
        ));
    }

    #[test]
    fn hex_rights_are_evaluated() {
        // 0x1200a9 = FILE_GENERIC_READ|EXECUTE (read-only) — fine for Users.
        assert!(sddl_is_hardened(
            "O:BAD:P(A;;FA;;;BA)(A;;FA;;;SY)(A;;0x1200a9;;;BU)"
        ));
        // 0x1301bf = FILE_GENERIC_READ|WRITE|EXECUTE|DELETE — not fine.
        assert!(!sddl_is_hardened(
            "O:BAD:P(A;;FA;;;BA)(A;;FA;;;SY)(A;;0x1301bf;;;BU)"
        ));
    }

    #[test]
    fn unknown_rights_tokens_fail_closed() {
        assert!(!sddl_is_hardened("O:BAD:P(A;;FA;;;BA)(A;;FZ;;;BU)"));
        assert!(!sddl_is_hardened("O:BAD:P(A;;FA;;;BA)(A;;FAX;;;BU)"));
    }

    #[test]
    fn read_only_users_grant_is_not_write() {
        // FR and generic-read/execute must not trip the write check.
        assert!(sddl_is_hardened(
            "O:BAD:P(A;;FA;;;BA)(A;;FA;;;SY)(A;;GRGX;;;BU)"
        ));
    }

    #[test]
    fn missing_admin_grant_is_rejected() {
        // A descriptor nobody (Admins/SYSTEM) can write is misconfigured, not
        // hardened — the saver always grants them full control.
        assert!(!sddl_is_hardened("O:BAD:P(A;;FR;;;BU)"));
    }
}
