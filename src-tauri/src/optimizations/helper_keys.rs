//! Per-user request-signing keys for the privileged helper (S1).
//!
//! Before 0.1.78 one key, `Helper\request-signing.key`, signed every request.
//! The NSIS install made it readable by all of `BUILTIN\Users`, so on a
//! machine with two Windows accounts either one could sign helper requests
//! as the other; the in-app install instead made it readable by a single SID,
//! which locked every other account out ("helper indisponivel").
//!
//! Now each Windows user gets `Helper\keys\<SID>\request-signing.key`, created
//! by the service (running as SYSTEM) the first time that user's app talks to
//! it, with a protected ACL: SYSTEM and Administrators full control, that SID
//! read-only, nobody else. The service learns the caller's SID from the
//! pipe client's process token, never from the request, and verifies the
//! HMAC with that user's key.
//!
//! Migration: if the legacy shared key still exists when a user is first
//! provisioned, it is moved (not copied) into that user's folder, so an
//! updated install keeps working without the "helper indisponivel" dialog
//! and the shared key stops existing.

use std::fs;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose, Engine as _};

pub const KEY_FILE_NAME: &str = "request-signing.key";
pub const MIN_KEY_LEN: usize = 32;

/// `S-1-<authority>-<sub>-...`: digits and dashes only. Checked before a SID
/// is ever placed in a path or an SDDL string.
pub fn is_valid_sid(sid: &str) -> bool {
    let Some(rest) = sid.strip_prefix("S-1-") else {
        return false;
    };
    sid.len() <= 184
        && !rest.is_empty()
        && rest.split('-').all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

pub fn legacy_key_path(root: &Path) -> PathBuf {
    root.join(KEY_FILE_NAME)
}

pub fn keys_dir(root: &Path) -> PathBuf {
    root.join("keys")
}

pub fn user_key_dir(root: &Path, sid: &str) -> Result<PathBuf, String> {
    if !is_valid_sid(sid) {
        return Err("SID invalido para a chave do helper.".to_string());
    }
    Ok(keys_dir(root).join(sid))
}

pub fn user_key_path(root: &Path, sid: &str) -> Result<PathBuf, String> {
    Ok(user_key_dir(root, sid)?.join(KEY_FILE_NAME))
}

/// Folder holding every user's key folder: SYSTEM and Administrators only.
pub const KEYS_DIR_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// A user's key folder and file: SYSTEM/Administrators full, that SID read.
pub fn user_key_sddl(sid: &str) -> Result<String, String> {
    if !is_valid_sid(sid) {
        return Err("SID invalido para a ACL da chave do helper.".to_string());
    }
    Ok(format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;{sid})"))
}

/// Non-secret helper files (version.txt): readable by every local user.
pub const PUBLIC_FILE_SDDL: &str = "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)";

pub fn decode_key(raw: &str) -> Result<Vec<u8>, String> {
    let trimmed = raw.trim();
    let decoded = general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| hex::decode(trimmed))
        .map_err(|_| "Chave local de assinatura do helper possui formato invalido.".to_string())?;
    if decoded.len() < MIN_KEY_LEN {
        return Err("Chave local de assinatura do helper e curta demais.".to_string());
    }
    Ok(decoded)
}

pub fn load_key(path: &Path) -> Result<Vec<u8>, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("Chave local de assinatura do helper indisponivel: {error}"))?;
    decode_key(&raw)
}

pub fn generate_key() -> String {
    let mut bytes = Vec::with_capacity(MIN_KEY_LEN);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    general_purpose::STANDARD.encode(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionOutcome {
    Existing,
    Created,
    MigratedLegacy,
}

/// Service side: returns this user's key, creating it (or moving the legacy
/// shared key into it) when missing.
///
/// Order matters: the `keys` folder is locked to SYSTEM/Administrators first,
/// so the user folder and the key file are born with only those two (by
/// inheritance). Only after the key is written does the user get read access.
/// Even on an old install whose root still grants `Users` read, the key is
/// never readable by another account, not even for an instant.
pub fn provision_user_key(root: &Path, sid: &str) -> Result<(Vec<u8>, ProvisionOutcome), String> {
    let key_path = user_key_path(root, sid)?;
    if let Ok(key) = load_key(&key_path) {
        return Ok((key, ProvisionOutcome::Existing));
    }
    let user_sddl = user_key_sddl(sid)?;

    let keys = keys_dir(root);
    fs::create_dir_all(&keys).map_err(|error| error.to_string())?;
    apply_sddl(&keys, KEYS_DIR_SDDL)?;

    let dir = user_key_dir(root, sid)?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    apply_sddl(&dir, KEYS_DIR_SDDL)?;

    let legacy = legacy_key_path(root);
    let (contents, outcome) = match fs::read_to_string(&legacy)
        .ok()
        .filter(|raw| decode_key(raw).is_ok())
    {
        Some(raw) => (raw.trim().to_string(), ProvisionOutcome::MigratedLegacy),
        None => (generate_key(), ProvisionOutcome::Created),
    };

    fs::write(&key_path, &contents).map_err(|error| error.to_string())?;
    apply_sddl(&key_path, &user_sddl)?;
    apply_sddl(&dir, &user_sddl)?;
    if outcome == ProvisionOutcome::MigratedLegacy {
        fs::remove_file(&legacy).map_err(|error| {
            format!("Chave migrada, mas a chave compartilhada antiga nao pode ser removida: {error}")
        })?;
    }

    Ok((decode_key(&contents)?, outcome))
}

#[cfg(windows)]
pub fn apply_sddl(path: &Path, sddl: &str) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW, SDDL_REVISION_1,
        SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{
        GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR,
    };

    let sddl_w = HSTRING::from(sddl);
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_w.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|error| format!("ACL invalida para {}: {error}", path.display()))?;

    let result = (|| {
        let mut present = windows::core::BOOL::default();
        let mut defaulted = windows::core::BOOL::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
            .map_err(|error| format!("Falha ao ler DACL: {error}"))?;
        let path_w = HSTRING::from(path.as_os_str());
        let status = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(path_w.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(dacl as *const ACL),
                None,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(format!(
                "Falha ao aplicar ACL em {}: codigo {}",
                path.display(),
                status.0
            ));
        }
        Ok(())
    })();

    let _ = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    result
}

#[cfg(not(windows))]
pub fn apply_sddl(_path: &Path, _sddl: &str) -> Result<(), String> {
    Ok(())
}

/// DACL of a file or folder as SDDL (tests).
#[cfg(all(windows, test))]
pub fn read_dacl_sddl(path: &Path) -> Result<String, String> {
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1,
        SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};

    let path_w = HSTRING::from(path.as_os_str());
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!("GetNamedSecurityInfoW falhou: {}", status.0));
    }
    let mut text = PWSTR::null();
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            None,
        )
    };
    let result = converted
        .map_err(|error| error.to_string())
        .and_then(|_| unsafe { text.to_string() }.map_err(|error| error.to_string()));
    unsafe {
        if !text.is_null() {
            let _ = LocalFree(Some(HLOCAL(text.0 as _)));
        }
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    result
}

/// SID of the user that owns process `pid`, read from its access token. This
/// is what the service trusts, never anything inside the request.
#[cfg(windows)]
pub fn sid_of_process(pid: u32) -> Result<String, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(|error| format!("Nao foi possivel abrir o processo cliente: {error}"))?;
    let result = sid_of_process_handle(process);
    let _ = unsafe { CloseHandle(process) };
    result
}

#[cfg(windows)]
pub fn current_process_sid() -> Result<String, String> {
    use windows::Win32::System::Threading::GetCurrentProcess;
    sid_of_process_handle(unsafe { GetCurrentProcess() })
}

#[cfg(windows)]
fn sid_of_process_handle(process: windows::Win32::Foundation::HANDLE) -> Result<String, String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows::Win32::System::Threading::OpenProcessToken;

    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }
        .map_err(|error| format!("Nao foi possivel ler o token do processo: {error}"))?;

    let result = (|| {
        let mut needed = 0_u32;
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
        if needed == 0 {
            return Err("Token do processo sem informacao de usuario.".to_string());
        }
        // u64 backing keeps TOKEN_USER suitably aligned.
        let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
        }
        .map_err(|error| format!("GetTokenInformation falhou: {error}"))?;
        let token_user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
        let mut text = PWSTR::null();
        unsafe { ConvertSidToStringSidW(token_user.User.Sid, &mut text) }
            .map_err(|error| format!("ConvertSidToStringSidW falhou: {error}"))?;
        let sid = unsafe { text.to_string() }.map_err(|error| error.to_string());
        let _ = unsafe { LocalFree(Some(HLOCAL(text.0 as _))) };
        let sid = sid?;
        if !is_valid_sid(&sid) {
            return Err("SID do processo com formato inesperado.".to_string());
        }
        Ok(sid)
    })();

    let _ = unsafe { CloseHandle(token) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writing under a SYSTEM/Administrators-only ACL needs an elevated
    /// token (GitHub's Windows runners have one). Unelevated local runs skip
    /// the ACL tests explicitly instead of failing on access denied.
    #[cfg(windows)]
    fn elevated() -> bool {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = HANDLE::default();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0_u32;
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            )
        }
        .is_ok();
        let _ = unsafe { CloseHandle(token) };
        ok && elevation.TokenIsElevated != 0
    }

    #[cfg(windows)]
    macro_rules! require_elevation {
        () => {
            if !elevated() {
                eprintln!("skipped: ACL test needs an elevated token");
                return;
            }
        };
    }

    /// Windows writes some SIDs back as aliases: the built-in Administrator
    /// account (RID 500, which GitHub's runners use) comes back as "LA".
    fn sddl_names_sid(dacl: &str, sid: &str) -> bool {
        dacl.contains(&format!(";;;{sid})")) || (sid.ends_with("-500") && dacl.contains(";;;LA)"))
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "analystblaze-helper-keys-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sid_validation_rejects_anything_that_is_not_a_sid() {
        assert!(is_valid_sid("S-1-5-18"));
        assert!(is_valid_sid("S-1-5-21-1004336348-1177238915-682003330-1001"));
        for bad in ["", "S-1-", "S-1-5-", "S-1-5-21-..\\..", "S-1-5-21-1)(A;;FA;;;WD", "s-1-5-18", "S-1-5-18 "] {
            assert!(!is_valid_sid(bad), "{bad:?}");
        }
        assert!(user_key_path(Path::new("C:\\x"), "..\\..\\evil").is_err());
        assert!(user_key_sddl("S-1-5-21-1)(A;;FA;;;WD").is_err());
    }

    #[test]
    fn user_sddl_grants_only_system_admins_and_that_user() {
        let sddl = user_key_sddl("S-1-5-21-1-2-3-1001").unwrap();
        assert_eq!(sddl, "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;S-1-5-21-1-2-3-1001)");
        assert!(!sddl.contains(";BU)") && !sddl.contains(";WD)") && !sddl.contains(";IU)"));
    }

    #[cfg(windows)]
    #[test]
    fn provisions_a_fresh_key_with_a_private_acl() {
        require_elevation!();
        let root = temp_root("fresh");
        let sid = current_process_sid().expect("test process has a SID");

        let (key, outcome) = provision_user_key(&root, &sid).unwrap();
        assert_eq!(outcome, ProvisionOutcome::Created);
        assert!(key.len() >= MIN_KEY_LEN);

        let path = user_key_path(&root, &sid).unwrap();
        assert_eq!(load_key(&path).unwrap(), key);
        let dacl = read_dacl_sddl(&path).unwrap();
        // Protected, and exactly: SYSTEM full, Administrators full, user read.
        assert!(dacl.starts_with("D:P"), "{dacl}");
        assert!(sddl_names_sid(&dacl, &sid), "{dacl}");
        for broad in [";;;BU)", ";;;WD)", ";;;IU)", ";;;AU)"] {
            assert!(!dacl.contains(broad), "key readable by a broad group: {dacl}");
        }

        // A second call reuses it.
        let (again, outcome) = provision_user_key(&root, &sid).unwrap();
        assert_eq!(outcome, ProvisionOutcome::Existing);
        assert_eq!(again, key);

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn migrates_the_legacy_shared_key_and_deletes_it() {
        require_elevation!();
        let root = temp_root("migrate");
        let sid = current_process_sid().unwrap();
        let legacy = generate_key();
        fs::write(legacy_key_path(&root), &legacy).unwrap();

        let (key, outcome) = provision_user_key(&root, &sid).unwrap();
        assert_eq!(outcome, ProvisionOutcome::MigratedLegacy);
        assert_eq!(key, decode_key(&legacy).unwrap(), "same key, so an updated install keeps working");
        assert!(!legacy_key_path(&root).exists(), "shared key must not survive the migration");

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn two_users_get_two_different_keys() {
        require_elevation!();
        // Second SID is synthetic: the ACL only names it, no account needed.
        let root = temp_root("two-users");
        let a = current_process_sid().unwrap();
        let b = "S-1-5-21-111111111-222222222-333333333-1999";

        let (key_a, _) = provision_user_key(&root, &a).unwrap();
        let (key_b, _) = provision_user_key(&root, b).unwrap();
        assert_ne!(key_a, key_b);
        let dacl_b = read_dacl_sddl(&user_key_path(&root, b).unwrap()).unwrap();
        assert!(sddl_names_sid(&dacl_b, b), "{dacl_b}");
        assert!(!sddl_names_sid(&dacl_b, &a), "user A must not be able to read B's key: {dacl_b}");

        let _ = fs::remove_dir_all(&root);
    }
}
