//! Detects whether the boot (system) drive is a spinning HDD rather than
//! an SSD, so Game Mode can skip steps that are actively counterproductive
//! on one - see this module's call site in `mod.rs`'s `apply_game_mode`
//! for the incident (2026-09, Blender misdetected as a game - see
//! `detection.rs`) that made this necessary. Pausing SysMain (Superfetch)
//! specifically is the concern: its entire job is caching disk reads to
//! smooth out exactly the kind of cold-start I/O storm a heavy app
//! triggers on launch, which matters far more on a drive with real seek
//! latency than on an SSD, where SysMain's benefit is already marginal.

use std::process::Command;
use std::sync::OnceLock;

use crate::process_ext::{decode_console_bytes, CommandExt};

/// True if the boot drive reports as a spinning HDD, or if the check
/// itself fails for any reason - failing toward the gentler assumption
/// (treat as HDD, skip the disk-contentious steps) rather than defaulting
/// to "must be an SSD, go ahead and be aggressive". Cached for the
/// process's lifetime: a drive's media type doesn't change while the app
/// is running, and this shells out to PowerShell, not free to redo on
/// every Game Mode activation.
pub fn system_drive_is_hdd() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        query_system_drive_media_type()
            .map(|media| media.eq_ignore_ascii_case("HDD"))
            .unwrap_or(true)
    })
}

fn query_system_drive_media_type() -> Option<String> {
    // Get-PhysicalDisk/Get-Partition both work unelevated - verified live
    // against a real machine, not assumed. Matches Windows' own Optimize
    // Drives tool's notion of "HDD" vs "SSD" (MediaType), rather than
    // guessing from rotation-speed heuristics of our own.
    let script = r#"
$letter = $env:SystemDrive.TrimEnd(':')
$diskNumber = (Get-Partition -DriveLetter $letter -ErrorAction Stop).DiskNumber
$disk = Get-PhysicalDisk -ErrorAction Stop | Where-Object { $_.DeviceId -eq $diskNumber } | Select-Object -First 1
$disk.MediaType
"#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = decode_console_bytes(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod manual_diagnostics {
    use super::query_system_drive_media_type;

    /// Not run in CI - prints the real detection result against whatever
    /// machine runs it, the same way active_use.rs's own manual_diagnostics
    /// test verifies against a real machine instead of trusting the
    /// mechanism from reasoning alone.
    #[test]
    #[ignore]
    fn print_live_media_type() {
        println!("system drive media type: {:?}", query_system_drive_media_type());
    }
}
