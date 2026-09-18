//! The fixes behind the "Defender is keeping the disk at 100%" Insights
//! card (telemetry::defender_disk decides which one applies). None of
//! them touches real-time protection, and Defender's own process/service
//! stay on safety.rs's protected lists - the whole point is to keep the
//! antivirus working while stopping it from starving the disk:
//!
//! - THROTTLE_DEFENDER_SCANS: `Set-MpPreference -ScanAvgCPULoadFactor 20
//!   -ScanOnlyIfIdleEnabled $true -DisableCpuThrottleOnIdleScans $false`.
//!   Windows' defaults (50 / true / true on the dev machine) let a
//!   scheduled scan use half the CPU and ignore the idle throttle, which
//!   on an HDD is enough to saturate it. Snapshot-backed: the previous
//!   value of each of the three properties is restored by
//!   RESTORE_DEFENDER_SCAN_SETTINGS.
//! - RENEW_DEFENDER_DEFINITIONS: `MpCmdRun -RemoveDefinitions -All` then a
//!   detached `MpCmdRun -SignatureUpdate`. The classic remedy for a scan
//!   loop caused by a corrupt definition set. The update can take a minute
//!   and the helper's pipe connection is bounded at 20s, so it is started
//!   and left to Windows Security to show, not awaited.
//! - SCHEDULE_VOLUME_CHECK: `fsutil dirty set X:` - marks the volume so
//!   autochk runs at next boot. Instant, needs no dismount, works for the
//!   system volume; the user reboots when convenient. Deliberately not an
//!   online `Repair-Volume -Scan` (minutes on a 2 TB disk, again past the
//!   pipe bound) nor `chkdsk /f` (interactive dismount prompt).
//!
//! All three need elevation (Set-MpPreference, MpCmdRun's definition
//! removal and fsutil all fail unelevated), so they only run inside the
//! privileged helper - see their profiles in safety.rs.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

use super::{
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};
use crate::process_ext::{decode_console_bytes, CommandExt};

/// Target `ScanAvgCPULoadFactor`. Microsoft's own guidance for
/// performance-sensitive machines; 5 is the floor Windows accepts and would
/// make a full scan take days on a large disk.
pub const THROTTLED_SCAN_CPU_LOAD_FACTOR: u32 = 20;

/// The Set-MpPreference properties THROTTLE_DEFENDER_SCANS changes and the
/// snapshot restore is allowed to write back - anything else is refused.
const SCAN_PREFERENCE_PROPERTIES: [&str; 3] = [
    "ScanAvgCPULoadFactor",
    "ScanOnlyIfIdleEnabled",
    "DisableCpuThrottleOnIdleScans",
];

pub async fn throttle_scans(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(throttle_scans_sync).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao limitar as verificacoes do Defender: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

fn throttle_scans_sync() -> ExecutionResult {
    let Some(current) = powershell_json(
        "Get-MpPreference -ErrorAction Stop | Select-Object ScanAvgCPULoadFactor, ScanOnlyIfIdleEnabled, DisableCpuThrottleOnIdleScans | ConvertTo-Json -Compress",
    ) else {
        return ExecutionResult {
            success: false,
            message: "Nao foi possivel ler as preferencias atuais do Defender.".to_string(),
            details: json!({ "implemented": true }),
        };
    };

    let previous_factor = current.get("ScanAvgCPULoadFactor").and_then(Value::as_u64);
    let previous_idle_only = current.get("ScanOnlyIfIdleEnabled").and_then(Value::as_bool);
    let previous_no_throttle = current
        .get("DisableCpuThrottleOnIdleScans")
        .and_then(Value::as_bool);

    let already = previous_factor.is_some_and(|factor| factor <= THROTTLED_SCAN_CPU_LOAD_FACTOR as u64)
        && previous_idle_only == Some(true)
        && previous_no_throttle == Some(false);
    if already {
        return ExecutionResult::ok(
            "As verificacoes do Defender ja estavam limitadas.",
            json!({ "implemented": true, "changed": false, "previous": current }),
        );
    }

    let snapshot = OptimizationSnapshot::new(
        "THROTTLE_DEFENDER_SCANS",
        vec![
            SnapshotEntry::DefenderScanPreference {
                property: "ScanAvgCPULoadFactor".to_string(),
                previous_value: previous_factor.map(|value| value.to_string()),
            },
            SnapshotEntry::DefenderScanPreference {
                property: "ScanOnlyIfIdleEnabled".to_string(),
                previous_value: previous_idle_only.map(|value| value.to_string()),
            },
            SnapshotEntry::DefenderScanPreference {
                property: "DisableCpuThrottleOnIdleScans".to_string(),
                previous_value: previous_no_throttle.map(|value| value.to_string()),
            },
        ],
        json!({ "previous": current }),
    );
    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        return ExecutionResult {
            success: false,
            message: "A alteracao foi bloqueada porque o snapshot nao pode ser salvo.".to_string(),
            details: json!({ "implemented": true, "snapshot_error": error }),
        };
    }

    let script = format!(
        "Set-MpPreference -ErrorAction Stop -ScanAvgCPULoadFactor {THROTTLED_SCAN_CPU_LOAD_FACTOR} -ScanOnlyIfIdleEnabled $true -DisableCpuThrottleOnIdleScans $false"
    );
    match run_powershell(&script) {
        Ok(()) => ExecutionResult::ok(
            format!(
                "Verificacoes do Defender limitadas a {THROTTLED_SCAN_CPU_LOAD_FACTOR}% de CPU e so com o PC ocioso. A protecao em tempo real continua igual."
            ),
            json!({
                "implemented": true,
                "changed": true,
                "previous": current,
                "snapshot": { "id": snapshot.id, "reversible": true },
            }),
        ),
        Err(error) => {
            let _ = snapshot::discard_snapshot(&snapshot.id);
            ExecutionResult {
                success: false,
                message: "O Windows nao aceitou a alteracao nas preferencias do Defender.".to_string(),
                details: json!({
                    "implemented": true,
                    "snapshot_discarded": true,
                    "error": error,
                }),
            }
        }
    }
}

pub async fn restore_scan_settings(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(snapshot::restore_defender_scan_snapshots).await {
        Ok(Ok(report)) => {
            let success = report.failed_entries == 0;
            ExecutionResult {
                success,
                message: if report.restored_snapshots == 0 && report.failed_snapshots == 0 {
                    "Nenhuma alteracao do Defender pendente para restaurar.".to_string()
                } else if success {
                    "Preferencias de verificacao do Defender restauradas.".to_string()
                } else {
                    "Parte das preferencias do Defender nao pode ser restaurada.".to_string()
                },
                details: serde_json::to_value(report).unwrap_or(Value::Null),
            }
        }
        Ok(Err(error)) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar preferencias do Defender: {error}"),
            details: json!({ "implemented": true }),
        },
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar preferencias do Defender: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

/// Writes one snapshot entry back. Called by snapshot.rs's restore loop
/// (inside the helper, where Set-MpPreference is allowed).
pub(crate) fn restore_scan_preference(property: &str, previous_value: Option<&str>) -> Result<(), String> {
    if !SCAN_PREFERENCE_PROPERTIES.contains(&property) {
        return Err(format!("Propriedade do Defender desconhecida: {property}"));
    }
    let Some(value) = previous_value else {
        // Never had a readable value before - nothing to put back.
        return Ok(());
    };
    let literal = match property {
        "ScanAvgCPULoadFactor" => {
            let factor: u32 = value
                .parse()
                .map_err(|_| "Valor de ScanAvgCPULoadFactor invalido.".to_string())?;
            if !(5..=100).contains(&factor) {
                return Err("Valor de ScanAvgCPULoadFactor fora da faixa.".to_string());
            }
            factor.to_string()
        }
        _ => match value.to_ascii_lowercase().as_str() {
            "true" => "$true".to_string(),
            "false" => "$false".to_string(),
            _ => return Err(format!("Valor booleano invalido para {property}.")),
        },
    };
    run_powershell(&format!("Set-MpPreference -ErrorAction Stop -{property} {literal}"))
}

pub async fn renew_definitions(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(renew_definitions_sync).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao renovar as definicoes do Defender: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

/// `C:\Program Files\Windows Defender\MpCmdRun.exe` is a stable launcher
/// that forwards to the current platform build; the versioned copies under
/// ProgramData are the fallback if a future Windows drops the launcher.
fn mpcmdrun_path() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from)?;
    let launcher = program_files.join("Windows Defender").join("MpCmdRun.exe");
    if launcher.exists() {
        return Some(launcher);
    }
    let platform = std::env::var_os("ProgramData")
        .map(PathBuf::from)?
        .join("Microsoft")
        .join("Windows Defender")
        .join("Platform");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(platform)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("MpCmdRun.exe"))
        .filter(|path| path.exists())
        .collect();
    versions.sort();
    versions.pop()
}

fn renew_definitions_sync() -> ExecutionResult {
    let Some(mpcmdrun) = mpcmdrun_path() else {
        return ExecutionResult {
            success: false,
            message: "MpCmdRun.exe nao foi encontrado neste Windows.".to_string(),
            details: json!({ "implemented": true }),
        };
    };

    let removal = Command::new(&mpcmdrun)
        .args(["-RemoveDefinitions", "-All"])
        .no_window()
        .output();
    let removal = match removal {
        Ok(output) => output,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel executar o MpCmdRun.".to_string(),
                details: json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    };
    if !removal.status.success() {
        return ExecutionResult {
            success: false,
            message: "O Defender recusou remover as definicoes atuais.".to_string(),
            details: json!({
                "implemented": true,
                "exit_code": removal.status.code(),
                "stdout": decode_console_bytes(&removal.stdout).trim(),
                "stderr": decode_console_bytes(&removal.stderr).trim(),
            }),
        };
    }

    // Fire and forget - see module docs. Windows Security's "Protection
    // updates" page shows the download; a failure there shows up as
    // Defender event 2001, which the next evidence pass counts.
    let update = Command::new(&mpcmdrun)
        .arg("-SignatureUpdate")
        .no_window()
        .spawn();
    match update {
        Ok(_) => ExecutionResult::ok(
            "Definicoes antigas removidas e download das novas iniciado. O Defender fica sem definicoes por alguns minutos ate terminar - acompanhe em Seguranca do Windows > Atualizacoes de protecao.",
            json!({ "implemented": true, "definitions_removed": true, "update_started": true }),
        ),
        Err(error) => ExecutionResult {
            success: false,
            message: "As definicoes foram removidas, mas o download das novas nao pode ser iniciado - abra Seguranca do Windows e procure atualizacoes manualmente.".to_string(),
            details: json!({ "implemented": true, "definitions_removed": true, "update_started": false, "error": error.to_string() }),
        },
    }
}

pub async fn schedule_volume_check(payload: Option<Value>) -> ExecutionResult {
    let Some(drive_letter) = drive_letter_from_payload(payload.as_ref()) else {
        return ExecutionResult {
            success: false,
            message: "Letra de unidade ausente ou invalida para agendar a verificacao.".to_string(),
            details: json!({ "implemented": true }),
        };
    };
    match tokio::task::spawn_blocking(move || schedule_volume_check_sync(drive_letter)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao agendar a verificacao do disco: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

/// A single ASCII letter, however the caller spelled it (`c`, `C:`, `C:\`).
pub(crate) fn drive_letter_from_payload(payload: Option<&Value>) -> Option<char> {
    let raw = payload?
        .get("driveLetter")
        .or_else(|| payload?.get("drive_letter"))
        .or_else(|| payload?.get("target"))?
        .as_str()?
        .trim();
    let mut chars = raw.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let rest: String = chars.collect();
    (letter.is_ascii_alphabetic() && matches!(rest.as_str(), "" | ":" | ":\\" | ":/")).then_some(letter)
}

fn schedule_volume_check_sync(drive_letter: char) -> ExecutionResult {
    let volume = format!("{drive_letter}:");
    let output = Command::new("fsutil")
        .args(["dirty", "set", &volume])
        .no_window()
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel executar o fsutil.".to_string(),
                details: json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    };
    let stdout = decode_console_bytes(&output.stdout).trim().to_string();
    let stderr = decode_console_bytes(&output.stderr).trim().to_string();
    if !output.status.success() {
        return ExecutionResult {
            success: false,
            message: format!("O Windows nao aceitou marcar a unidade {volume} para verificacao."),
            details: json!({ "implemented": true, "exit_code": output.status.code(), "stdout": stdout, "stderr": stderr }),
        };
    }
    ExecutionResult::ok(
        format!("Verificacao do sistema de arquivos da unidade {volume} agendada para a proxima reinicializacao. Reinicie quando puder - ela roda antes do Windows abrir e pode levar alguns minutos."),
        json!({ "implemented": true, "volume": volume, "requires_reboot": true, "stdout": stdout }),
    )
}

fn run_powershell(script: &str) -> Result<(), String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .no_window()
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = decode_console_bytes(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("PowerShell saiu com codigo {:?}.", output.status.code())
        } else {
            stderr
        })
    }
}

fn powershell_json(script: &str) -> Option<Value> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = decode_console_bytes(&output.stdout);
    serde_json::from_str(text.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_letter_accepts_the_common_spellings_and_nothing_else() {
        for spelling in ["c", "C", "C:", "c:\\", "C:/"] {
            assert_eq!(
                drive_letter_from_payload(Some(&json!({ "driveLetter": spelling }))),
                Some('C'),
                "{spelling}"
            );
        }
        for bad in ["", "CD", "C:\\Users", "1:", "..", "\\\\?\\C:"] {
            assert_eq!(drive_letter_from_payload(Some(&json!({ "driveLetter": bad }))), None, "{bad}");
        }
        assert_eq!(drive_letter_from_payload(None), None);
        assert_eq!(drive_letter_from_payload(Some(&json!({ "drive_letter": "d" }))), Some('D'));
    }

    #[test]
    fn restore_refuses_unknown_properties_and_bad_values_before_touching_powershell() {
        assert!(restore_scan_preference("DisableRealtimeMonitoring", Some("true")).is_err());
        assert!(restore_scan_preference("ScanAvgCPULoadFactor", Some("200")).is_err());
        assert!(restore_scan_preference("ScanAvgCPULoadFactor", Some("abc")).is_err());
        assert!(restore_scan_preference("ScanOnlyIfIdleEnabled", Some("yes")).is_err());
        // A property that had no readable previous value is a no-op, not an error.
        assert!(restore_scan_preference("ScanOnlyIfIdleEnabled", None).is_ok());
    }
}
