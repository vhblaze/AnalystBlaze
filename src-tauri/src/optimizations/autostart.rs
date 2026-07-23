use crate::audit;
use serde_json::json;

/// The app's own entry in the per-user Run key - separate from
/// windows_inventory.rs/windows_actions.rs, which manage *other* apps'
/// startup entries (with risk classification, snapshot-based restore,
/// allowlists). Registering ourselves is a simple, fully self-owned,
/// trivially reversible toggle - HKCU (not HKLM) so it applies per signed-in
/// user and never needs admin/the privileged helper, matching the
/// per-machine install running once per logged-in user.
const RUN_VALUE_NAME: &str = "AnalystBlaze";

pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    let result = set_autostart_enabled_inner(enabled);
    let _ = audit::record_event(
        if result.is_ok() { "info" } else { "warn" },
        "autostart.preference_applied",
        "Preferencia de iniciar com o Windows aplicada.",
        json!({
            "enabled": enabled,
            "error": result.as_ref().err(),
        }),
    );
    result
}

#[cfg(windows)]
fn set_autostart_enabled_inner(enabled: bool) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            KEY_WRITE,
        )
        .map_err(|error| error.to_string())?;

    if enabled {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let command = format!("\"{}\"", exe.display());
        key.set_value(RUN_VALUE_NAME, &command)
            .map_err(|error| error.to_string())
    } else {
        match key.delete_value(RUN_VALUE_NAME) {
            Ok(()) => Ok(()),
            // Already absent - disabling an already-disabled entry isn't an error.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(not(windows))]
fn set_autostart_enabled_inner(_enabled: bool) -> Result<(), String> {
    Ok(())
}
