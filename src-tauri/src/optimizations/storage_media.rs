//! Detects whether the boot (system) drive is a spinning HDD rather than
//! an SSD, so Game Mode can skip steps that are actively counterproductive
//! on one - see this module's call site in `mod.rs`'s `apply_game_mode`
//! for the incident (2026-09, Blender misdetected as a game - see
//! `detection.rs`) that made this necessary. Pausing SysMain (Superfetch)
//! specifically is the concern: its entire job is caching disk reads to
//! smooth out exactly the kind of cold-start I/O storm a heavy app
//! triggers on launch, which matters far more on a drive with real seek
//! latency than on an SSD, where SysMain's benefit is already marginal.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Command;
use std::sync::OnceLock;

use super::ExecutionResult;
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

/// Windows' own scheduled optimization ("Defrag and Optimize Drives")
/// only helps a spinning HDD - an SSD is TRIM'd, not defragmented, and
/// Windows already knows this (the built-in task optimizes each volume
/// with whichever method suits its media type). This check is only worth
/// surfacing at all when system_drive_is_hdd() is true; a caller showing
/// this to the user must gate on that first; this module doesn't do it
/// internally so the two facts (media type, task status) stay independent
/// and each remain unit-testable on their own inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledDefragStatus {
    /// False means the task itself is disabled - nothing gets optimized
    /// automatically, on any drive, until re-enabled.
    pub enabled: bool,
    /// ISO-8601, or None if the task has apparently never run.
    pub last_run_time: Option<String>,
    /// 0 means success; anything else (including None, if the task has
    /// never run) is not itself alarming - Windows also skips the run
    /// entirely on some conditions (battery power, in an active game,
    /// etc.), which is normal and not a fault to react to.
    pub last_task_result: Option<i64>,
}

/// Combined shape for the D6 "Explorador de Disco" screen's optimization
/// card - both facts (media type, scheduler status) in one round trip
/// instead of the frontend making two calls and gating visibility itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskOptimizationInsight {
    pub is_hdd: bool,
    pub defrag: Option<ScheduledDefragStatus>,
}

/// Only worth computing the (unelevated but still real) scheduled-defrag
/// PowerShell round trip when the drive is actually an HDD - on an SSD,
/// scheduled optimization uses TRIM instead of defragmentation and is
/// already exactly as it should be regardless of this task's state, so
/// there's nothing useful to tell the user either way.
pub fn disk_optimization_insight() -> DiskOptimizationInsight {
    let is_hdd = system_drive_is_hdd();
    DiskOptimizationInsight {
        is_hdd,
        defrag: if is_hdd { scheduled_defrag_status() } else { None },
    }
}

/// Queries Windows' native "ScheduledDefrag" task (Task Scheduler path
/// `\Microsoft\Windows\Defrag\`) - the same task the Optimize Drives
/// Control Panel applet manages, verified live against a real machine
/// (Get-ScheduledTask/Get-ScheduledTaskInfo both work unelevated).
pub fn scheduled_defrag_status() -> Option<ScheduledDefragStatus> {
    let script = r#"
$task = Get-ScheduledTask -TaskPath "\Microsoft\Windows\Defrag\" -TaskName "ScheduledDefrag" -ErrorAction Stop
$info = Get-ScheduledTaskInfo -TaskPath "\Microsoft\Windows\Defrag\" -TaskName "ScheduledDefrag" -ErrorAction Stop
[PSCustomObject]@{
    enabled = ($task.State -ne "Disabled")
    lastRunTime = if ($info.LastRunTime) { $info.LastRunTime.ToString("o") } else { $null }
    lastTaskResult = $info.LastTaskResult
} | ConvertTo-Json -Compress
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
        return None;
    }
    serde_json::from_str::<ScheduledDefragStatus>(&text).ok()
}

/// Re-enables the "ScheduledDefrag" task - the ENABLE_SCHEDULED_DEFRAG
/// action, offered from an Insights card when disk_optimization_insight()
/// finds it off on an HDD boot drive. Unlike the read-only status check
/// above, changing a scheduled task's enabled state needs elevation (an
/// unelevated attempt returns "Access is denied"), so this only ever runs
/// inside the already-elevated privileged helper - see safety.rs's
/// requires_privileged_helper on this action's profile. No snapshot: the
/// state is a trivial single boolean, re-toggled the same way through the
/// same task if this ever needs undoing.
pub async fn enable_scheduled_defrag(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(enable_scheduled_defrag_sync).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao reativar a otimizacao agendada de disco: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

fn enable_scheduled_defrag_sync() -> ExecutionResult {
    let script = r#"
Enable-ScheduledTask -TaskPath "\Microsoft\Windows\Defrag\" -TaskName "ScheduledDefrag" -ErrorAction Stop | Out-Null
"#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .no_window()
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel chamar o PowerShell.".to_string(),
                details: json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    };

    let success = output.status.success();
    let stderr = decode_console_bytes(&output.stderr).trim().to_string();
    ExecutionResult {
        success,
        message: if success {
            "Otimizacao automatica de disco reativada.".to_string()
        } else {
            "Nao foi possivel reativar a otimizacao automatica de disco.".to_string()
        },
        details: json!({ "implemented": true, "stderr": stderr }),
    }
}

#[cfg(test)]
mod manual_diagnostics {
    use super::{query_system_drive_media_type, scheduled_defrag_status};

    /// Not run in CI - prints the real detection result against whatever
    /// machine runs it, the same way active_use.rs's own manual_diagnostics
    /// test verifies against a real machine instead of trusting the
    /// mechanism from reasoning alone.
    #[test]
    #[ignore]
    fn print_live_media_type() {
        println!("system drive media type: {:?}", query_system_drive_media_type());
    }

    #[test]
    #[ignore]
    fn print_live_scheduled_defrag_status() {
        println!("scheduled defrag status: {:?}", scheduled_defrag_status());
    }
}
