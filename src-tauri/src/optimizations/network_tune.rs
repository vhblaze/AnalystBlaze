use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use super::{
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};
use crate::audit;
use crate::process_ext::{decode_console_bytes, CommandExt};

const SESSION_FILE: &str = "network-tune-session.json";
const CONFIRM_WINDOW_SECONDS: i64 = 30;

/// A batch of TCP/network stack tweaks applied together, mirroring
/// `focus::FocusSession`'s "apply now, auto-revert if nobody confirms"
/// pattern - but this one has a second wrinkle: some of these settings only
/// take effect after a reboot, so the confirm countdown can't just run in
/// this process the way Modo Foco's does. `requires_reboot` sessions get
/// `confirm_deadline` left at 0 until the app is relaunched (see
/// `pending_network_tune_session`), which is the earliest point the
/// registry values could plausibly be live.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkTuneSession {
    pub id: String,
    pub label: String,
    pub created_at: i64,
    pub confirm_deadline: i64,
    pub status: String,
    pub restore_reason: Option<String>,
    pub restored_at: Option<i64>,
    pub confirmed_at: Option<i64>,
    pub snapshot_ids: Vec<String>,
    pub requires_reboot: bool,
    /// Set once the confirm window has actually started counting down -
    /// distinguishes "applied, waiting for the user to relaunch/reboot"
    /// from "actively counting down toward auto-revert".
    pub countdown_started: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TcpGlobalSettings {
    auto_tuning_level_local: Option<String>,
    ecn_capability: Option<String>,
}

static MONITORED_SESSION: OnceLock<Mutex<Option<String>>> = OnceLock::new();

pub async fn apply_network_tune(payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(move || apply_network_tune_sync(payload)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao aplicar otimizacao de rede: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

#[cfg(windows)]
fn apply_network_tune_sync(payload: Option<Value>) -> ExecutionResult {
    if let Some(existing) = read_session() {
        if existing.status == "pending" {
            let stale = existing.countdown_started
                && chrono::Utc::now().timestamp() >= existing.confirm_deadline;
            if stale {
                // Orphaned from a prior run whose monitor never got to
                // finish it (e.g. app closed mid-countdown before this
                // process could revert it). We're already running with
                // whatever privilege this action needed to get applied in
                // the first place, so revert it inline instead of forcing
                // the user to go find and dismiss a stuck session first.
                let _ = revert_network_tune_session(Some("stale_session_replaced".to_string()));
            } else {
                return ExecutionResult {
                    success: false,
                    message: "Ja existe uma mudanca de rede aguardando confirmacao. Resolva-a antes de aplicar outra.".to_string(),
                    details: json!({ "implemented": true, "pendingSession": existing }),
                };
            }
        }
    }

    let payload = payload.unwrap_or_else(|| json!({}));
    let auto_tuning_level = payload
        .get("autoTuningLevel")
        .and_then(Value::as_str)
        .filter(|value| is_safe_tcp_setting_literal(value));
    let ecn_capability = payload
        .get("ecnCapability")
        .and_then(Value::as_str)
        .filter(|value| is_safe_tcp_setting_literal(value));
    let nagle_disabled = payload.get("nagleDisabled").and_then(Value::as_bool);
    let adapter_name = payload.get("adapterName").and_then(Value::as_str);
    let throttling_disabled = payload.get("throttlingDisabled").and_then(Value::as_bool);

    let has_live_settings = auto_tuning_level.is_some() || ecn_capability.is_some();
    let has_nagle = nagle_disabled.is_some() && adapter_name.is_some();
    let has_throttling = throttling_disabled.is_some();
    let requires_reboot = has_nagle || has_throttling;

    if !has_live_settings && !has_nagle && !has_throttling {
        return ExecutionResult {
            success: false,
            message: "Nenhuma otimizacao de rede foi selecionada.".to_string(),
            details: json!({ "implemented": true }),
        };
    }
    if requires_reboot && has_live_settings {
        return ExecutionResult {
            success: false,
            message: "Nao misture ajustes instantaneos com ajustes que exigem reiniciar - aplique em dois passos.".to_string(),
            details: json!({ "implemented": true }),
        };
    }

    let mut entries: Vec<SnapshotEntry> = Vec::new();
    let mut labels: Vec<&str> = Vec::new();

    if has_live_settings {
        let before = match query_tcp_global_settings() {
            Ok(value) => value,
            Err(error) => {
                return ExecutionResult {
                    success: false,
                    message: "Nao foi possivel consultar as configuracoes atuais de TCP."
                        .to_string(),
                    details: json!({ "implemented": true, "error": error }),
                };
            }
        };

        // Each property is its own Set-NetTCPSetting call - some are
        // read-only on certain Windows builds (CongestionProvider was
        // observed as such), and PowerShell rejects the *entire* command
        // when any one flag is invalid. Separate calls mean a rejected
        // property doesn't take the others down with it, and a mid-way
        // failure rolls back only what this call actually changed.
        let mut candidates: Vec<(&str, &str, &str)> = Vec::new();
        if let Some(level) = auto_tuning_level {
            if before.auto_tuning_level_local.as_deref() != Some(level) {
                candidates.push(("autoTuningLevelLocal", "-AutoTuningLevelLocal", level));
            }
        }
        if let Some(ecn) = ecn_capability {
            if before.ecn_capability.as_deref() != Some(ecn) {
                candidates.push(("ecnCapability", "-EcnCapability", ecn));
            }
        }

        for (property, flag, target) in candidates {
            let script = format!("Set-NetTCPSetting -SettingName Internet -ErrorAction Stop {flag} {target}");
            if let Err(error) = run_powershell(&script) {
                let rollback = snapshot::restore_snapshot_entries(&OptimizationSnapshot::new(
                    "APPLY_NETWORK_TUNE",
                    entries.clone(),
                    json!({}),
                ));
                return ExecutionResult {
                    success: false,
                    message: format!("O Windows recusou alterar '{property}': {error}"),
                    details: json!({ "implemented": true, "property": property, "error": error, "rollback": rollback.messages }),
                };
            }
            let previous = match property {
                "autoTuningLevelLocal" => before.auto_tuning_level_local.clone(),
                _ => before.ecn_capability.clone(),
            };
            entries.push(tcp_setting_entry(property, previous, target));
            labels.push(if property == "autoTuningLevelLocal" { "Auto-Tuning" } else { "ECN" });
        }
    }

    if let (Some(disable), Some(adapter)) = (nagle_disabled, adapter_name) {
        if !super::safety::is_safe_network_target(adapter) {
            return ExecutionResult {
                success: false,
                message: "Nome de adaptador invalido.".to_string(),
                details: json!({ "implemented": true, "adapter": adapter }),
            };
        }
        let guid = match resolve_adapter_guid(adapter) {
            Ok(guid) => guid,
            Err(error) => {
                return ExecutionResult {
                    success: false,
                    message: "Nao foi possivel identificar o adaptador selecionado.".to_string(),
                    details: json!({ "implemented": true, "adapter": adapter, "error": error }),
                };
            }
        };
        let subkey = format!(
            r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\{guid}"
        );
        let target: u32 = if disable { 1 } else { 0 };
        for value_name in ["TcpAckFrequency", "TCPNoDelay"] {
            match read_hklm_dword(&subkey, value_name) {
                Ok(previous) => {
                    entries.push(SnapshotEntry::RegistryValue {
                        hive: "HKLM".to_string(),
                        subkey: subkey.clone(),
                        value_name: value_name.to_string(),
                        previous_value_type: previous.map(|_| "REG_DWORD".to_string()),
                        previous_value_bytes: previous.map(|value| value.to_le_bytes().to_vec()),
                        target_value_type: "REG_DWORD".to_string(),
                        target_value_bytes: target.to_le_bytes().to_vec(),
                    });
                }
                Err(error) => {
                    return ExecutionResult {
                        success: false,
                        message: "Nao foi possivel consultar o registro do adaptador.".to_string(),
                        details: json!({ "implemented": true, "error": error }),
                    };
                }
            }
            if let Err(error) = write_hklm_dword(&subkey, value_name, target) {
                let rollback = snapshot::restore_snapshot_entries(&OptimizationSnapshot::new(
                    "APPLY_NETWORK_TUNE",
                    entries.clone(),
                    json!({}),
                ));
                return ExecutionResult {
                    success: false,
                    message: "Falha ao gravar a chave do adaptador; alteracoes revertidas."
                        .to_string(),
                    details: json!({ "implemented": true, "error": error, "rollback": rollback.messages }),
                };
            }
        }
        labels.push("Nagle's Algorithm");
    }

    if throttling_disabled == Some(true) {
        const SUBKEY: &str =
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile";
        for (value_name, target) in [("NetworkThrottlingIndex", 0xffffffffu32), ("SystemResponsiveness", 0u32)] {
            match read_hklm_dword(SUBKEY, value_name) {
                Ok(previous) => {
                    entries.push(SnapshotEntry::RegistryValue {
                        hive: "HKLM".to_string(),
                        subkey: SUBKEY.to_string(),
                        value_name: value_name.to_string(),
                        previous_value_type: previous.map(|_| "REG_DWORD".to_string()),
                        previous_value_bytes: previous.map(|value| value.to_le_bytes().to_vec()),
                        target_value_type: "REG_DWORD".to_string(),
                        target_value_bytes: target.to_le_bytes().to_vec(),
                    });
                }
                Err(error) => {
                    return ExecutionResult {
                        success: false,
                        message: "Nao foi possivel consultar as chaves de throttling de rede."
                            .to_string(),
                        details: json!({ "implemented": true, "error": error }),
                    };
                }
            }
            if let Err(error) = write_hklm_dword(SUBKEY, value_name, target) {
                let rollback = snapshot::restore_snapshot_entries(&OptimizationSnapshot::new(
                    "APPLY_NETWORK_TUNE",
                    entries.clone(),
                    json!({}),
                ));
                return ExecutionResult {
                    success: false,
                    message: "Falha ao gravar as chaves de throttling; alteracoes revertidas."
                        .to_string(),
                    details: json!({ "implemented": true, "error": error, "rollback": rollback.messages }),
                };
            }
        }
        labels.push("Network Throttling Index");
    }

    if entries.is_empty() {
        return ExecutionResult {
            success: true,
            message: "As configuracoes ja estavam no valor solicitado; nada para aplicar."
                .to_string(),
            details: json!({ "implemented": true, "changed": false }),
        };
    }

    let label = labels.join(", ");
    let snapshot = OptimizationSnapshot::new(
        "APPLY_NETWORK_TUNE",
        entries,
        json!({ "label": label, "requiresReboot": requires_reboot }),
    );
    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        let rollback = snapshot::restore_snapshot_entries(&snapshot);
        return ExecutionResult {
            success: false,
            message: "A alteracao foi revertida porque o snapshot nao pode ser salvo."
                .to_string(),
            details: json!({ "implemented": true, "snapshot_error": error, "rollback": rollback.messages }),
        };
    }

    let now = chrono::Utc::now().timestamp();
    let session = NetworkTuneSession {
        id: uuid::Uuid::new_v4().simple().to_string(),
        label: label.clone(),
        created_at: now,
        // Live settings start counting down immediately; reboot-bound
        // settings only start once this process has actually restarted
        // (see pending_network_tune_session), since the values aren't
        // live yet and there's nothing meaningful to confirm.
        confirm_deadline: if requires_reboot { 0 } else { now + CONFIRM_WINDOW_SECONDS },
        status: "pending".to_string(),
        restore_reason: None,
        restored_at: None,
        confirmed_at: None,
        snapshot_ids: vec![snapshot.id.clone()],
        requires_reboot,
        countdown_started: !requires_reboot,
    };

    if let Err(error) = write_session(&session) {
        let rollback = snapshot::restore_snapshots_by_ids(&session.snapshot_ids);
        return ExecutionResult {
            success: false,
            message: "A alteracao foi revertida porque a sessao nao pode ser salva.".to_string(),
            details: json!({ "implemented": true, "error": error, "rollback": rollback.messages }),
        };
    }

    if !requires_reboot {
        spawn_restore_monitor(session.id.clone(), session.confirm_deadline);
    }

    let _ = audit::record_event(
        "info",
        "network_tune.applied",
        "Otimizacao de rede aplicada com confirmacao pendente.",
        json!({ "session": &session }),
    );

    ExecutionResult {
        success: true,
        message: if requires_reboot {
            format!("{label} aplicado. Reinicie o computador para que tenha efeito.")
        } else {
            format!("{label} aplicado. Confirme em ate {CONFIRM_WINDOW_SECONDS}s ou sera revertido.")
        },
        details: json!({
            "implemented": true,
            "session": session,
        }),
    }
}

#[cfg(not(windows))]
fn apply_network_tune_sync(_payload: Option<Value>) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Otimizacao de rede disponivel apenas no Windows.".to_string(),
        details: json!({ "implemented": true }),
    }
}

pub fn confirm_network_tune_session() -> ExecutionResult {
    let Some(mut session) = read_session() else {
        return ExecutionResult {
            success: false,
            message: "Nenhuma sessao de otimizacao de rede pendente.".to_string(),
            details: json!({ "implemented": true }),
        };
    };
    if session.status != "pending" {
        return ExecutionResult {
            success: false,
            message: "Essa sessao ja foi resolvida.".to_string(),
            details: json!({ "implemented": true, "session": session }),
        };
    }

    session.status = "confirmed".to_string();
    session.confirmed_at = Some(chrono::Utc::now().timestamp());
    let _ = write_session(&session);

    let _ = audit::record_event(
        "info",
        "network_tune.confirmed",
        "Usuario confirmou a otimizacao de rede.",
        json!({ "session": &session }),
    );

    ExecutionResult {
        success: true,
        message: "Otimizacao de rede mantida.".to_string(),
        details: json!({ "implemented": true, "session": session }),
    }
}

/// Async, privileged-helper-routable entry point for reverting - this is
/// what gets registered in the action dispatch table (both the manual
/// "Desfazer" button and the timeout monitor call this, never the sync
/// function directly, so both paths get the same elevation handling).
pub async fn revert_network_tune(payload: Option<Value>) -> ExecutionResult {
    let reason = payload
        .as_ref()
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    match tokio::task::spawn_blocking(move || revert_network_tune_session(reason)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao reverter otimizacao de rede: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub fn revert_network_tune_session(reason: Option<String>) -> ExecutionResult {
    let Some(mut session) = read_session() else {
        return ExecutionResult {
            success: false,
            message: "Nenhuma sessao de otimizacao de rede pendente.".to_string(),
            details: json!({ "implemented": true }),
        };
    };
    if session.status != "pending" {
        return ExecutionResult {
            success: false,
            message: "Essa sessao ja foi resolvida.".to_string(),
            details: json!({ "implemented": true, "session": session }),
        };
    }

    let report = snapshot::restore_snapshots_by_ids(&session.snapshot_ids);
    let reason = reason.unwrap_or_else(|| "manual_revert".to_string());
    session.status = "reverted".to_string();
    session.restored_at = Some(chrono::Utc::now().timestamp());
    session.restore_reason = Some(reason.clone());
    let requires_reboot = session.requires_reboot;
    let _ = write_session(&session);

    let _ = audit::record_event(
        "info",
        "network_tune.reverted",
        "Otimizacao de rede revertida por snapshot local.",
        json!({ "reason": &reason, "session": &session, "report": &report }),
    );

    ExecutionResult {
        success: report.failed_entries == 0,
        message: if requires_reboot {
            "Otimizacao de rede revertida no registro. Reinicie novamente para concluir a reversao.".to_string()
        } else {
            "Otimizacao de rede revertida.".to_string()
        },
        details: json!({ "implemented": true, "session": session, "report": report }),
    }
}

/// Called on app startup (and safe to call repeatedly, e.g. from a status
/// poll) - resumes monitoring a session left over from before the process
/// restarted, arming its confirm window the first time it's seen since
/// restart. Returns the session if one is still pending confirmation.
pub async fn pending_network_tune_session() -> Option<NetworkTuneSession> {
    let mut session = read_session()?;
    if session.status != "pending" {
        return None;
    }

    let now = chrono::Utc::now().timestamp();
    if !session.countdown_started {
        session.confirm_deadline = now + CONFIRM_WINDOW_SECONDS;
        session.countdown_started = true;
        let _ = write_session(&session);
    } else if now >= session.confirm_deadline {
        // The window already closed while nobody was around (app closed
        // mid-countdown, or the reboot took longer than 30s to come back
        // to a running app) - resolve it now instead of leaving it stuck.
        // Routed through execute_command (not the local revert_network_tune
        // directly) because this runs in the unprivileged main app process,
        // and the actual rollback needs the same helper escalation the
        // manual "Desfazer" button gets.
        let _ = super::execute_command(
            "REVERT_NETWORK_TUNE",
            Some(json!({ "reason": "deadline_passed_while_closed" })),
        )
        .await;
        return None;
    }

    spawn_restore_monitor(session.id.clone(), session.confirm_deadline);
    Some(session)
}

pub async fn restart_windows(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(restart_windows_now).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao reiniciar: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

fn restart_windows_now() -> ExecutionResult {
    #[cfg(windows)]
    {
        let output = Command::new("shutdown")
            .args(["/r", "/t", "5"])
            .no_window()
            .output();
        match output {
            Ok(output) if output.status.success() => {
                let _ = audit::record_event(
                    "warn",
                    "network_tune.restart_requested",
                    "Reinicio do Windows solicitado pelo usuario para aplicar otimizacao de rede.",
                    json!({}),
                );
                ExecutionResult::ok(
                    "Reiniciando em 5 segundos...",
                    json!({ "implemented": true }),
                )
            }
            Ok(output) => ExecutionResult {
                success: false,
                message: "O Windows recusou reiniciar.".to_string(),
                details: json!({ "implemented": true, "stderr": decode_console_bytes(&output.stderr) }),
            },
            Err(error) => ExecutionResult {
                success: false,
                message: format!("Nao foi possivel chamar shutdown: {error}"),
                details: json!({ "implemented": true }),
            },
        }
    }
    #[cfg(not(windows))]
    {
        ExecutionResult {
            success: false,
            message: "Reiniciar o computador esta disponivel apenas no Windows.".to_string(),
            details: json!({ "implemented": true }),
        }
    }
}

pub async fn renew_dhcp_lease(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(renew_dhcp_lease_sync).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao renovar o IP: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

#[cfg(windows)]
fn renew_dhcp_lease_sync() -> ExecutionResult {
    let output = match Command::new("ipconfig").args(["/renew"]).no_window().output() {
        Ok(output) => output,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel chamar ipconfig.".to_string(),
                details: json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    };
    let stdout = decode_console_bytes(&output.stdout);
    let success = output.status.success();
    if success {
        crate::telemetry::network::invalidate_network_cache();
    }
    let _ = audit::record_event(
        if success { "info" } else { "warn" },
        "optimization.network.dhcp_renewed",
        "Concessao de IP renovada (ipconfig /renew).",
        json!({ "stdout": stdout.trim(), "success": success }),
    );
    ExecutionResult {
        success,
        message: if success {
            "Endereco IP renovado.".to_string()
        } else {
            "O Windows recusou renovar o endereco IP.".to_string()
        },
        details: json!({ "implemented": true, "stdout": stdout.trim() }),
    }
}

#[cfg(not(windows))]
fn renew_dhcp_lease_sync() -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Renovacao de IP indisponivel nesta plataforma.".to_string(),
        details: json!({ "implemented": true }),
    }
}

fn tcp_setting_entry(property: &str, previous: Option<String>, _target: &str) -> SnapshotEntry {
    SnapshotEntry::TcpGlobalSetting {
        property: property.to_string(),
        previous_value: previous,
    }
}

fn is_safe_tcp_setting_literal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.chars().all(|ch| ch.is_ascii_alphanumeric())
}

#[cfg(windows)]
fn query_tcp_global_settings() -> Result<TcpGlobalSettings, String> {
    // Get-NetTCPSetting's properties are enums - ConvertTo-Json serializes
    // them as their raw underlying integer, not the string name, unless
    // explicitly cast. Force [string] on each so this always deserializes
    // to the same enum names Set-NetTCPSetting itself accepts (e.g.
    // "Normal", "Enabled", "CUBIC").
    let output = run_powershell(
        "Get-NetTCPSetting -SettingName Internet -ErrorAction Stop | Select-Object @{N='AutoTuningLevelLocal';E={[string]$_.AutoTuningLevelLocal}},@{N='EcnCapability';E={[string]$_.EcnCapability}} | ConvertTo-Json -Compress",
    )?;
    serde_json::from_str(output.trim()).map_err(|error| format!("resposta inesperada do Windows: {error}"))
}

#[cfg(not(windows))]
fn query_tcp_global_settings() -> Result<TcpGlobalSettings, String> {
    Err("Configuracoes de TCP indisponiveis nesta plataforma.".to_string())
}

#[cfg(windows)]
fn resolve_adapter_guid(adapter_name: &str) -> Result<String, String> {
    let script = format!(
        "(Get-NetAdapter -Name '{}' -ErrorAction Stop).InterfaceGuid",
        escape_powershell_literal(adapter_name)
    );
    let output = run_powershell(&script)?;
    // InterfaceGuid already comes back wrapped in braces (e.g.
    // "{4D36E972-...}"), matching the literal subkey name Windows uses
    // under Tcpip\Parameters\Interfaces - keep it as-is.
    let guid = output.trim().to_string();
    if !guid.starts_with('{') || !guid.ends_with('}') {
        return Err(format!("GUID de adaptador em formato inesperado: {guid}"));
    }
    Ok(guid)
}

#[cfg(windows)]
fn read_hklm_dword(subkey: &str, value_name: &str) -> Result<Option<u32>, String> {
    use std::io;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = match root.open_subkey_with_flags(subkey, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    match key.get_value::<u32, _>(value_name) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(windows)]
fn write_hklm_dword(subkey: &str, value_name: &str, value: u32) -> Result<(), String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (key, _) = root.create_subkey(subkey).map_err(|error| error.to_string())?;
    key.set_value(value_name, &value).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<String, String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .no_window()
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(decode_console_bytes(&output.stdout))
    } else {
        Err(decode_console_bytes(&output.stderr).trim().to_string())
    }
}

#[cfg(windows)]
fn escape_powershell_literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// Spawns a `tokio` task (not an OS thread) so the timeout revert can
/// `.await` the same privileged-helper-aware dispatch a manual click would
/// use - AutoTuning/ECN/CongestionProvider and the registry-based tweaks
/// both need admin to undo, and only `execute_command` knows how to escalate
/// to the helper when this process isn't elevated. Guarded by
/// `MONITORED_SESSION` so a re-entrant call (e.g. the frontend polling
/// `pending_network_tune_session`) doesn't spawn a second timer for the same
/// session.
fn spawn_restore_monitor(session_id: String, deadline: i64) {
    let guard_slot = MONITORED_SESSION.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = guard_slot.lock() {
        if guard.as_deref() == Some(session_id.as_str()) {
            return; // already being monitored in this process
        }
        *guard = Some(session_id.clone());
    }

    // A plain OS thread with its own throwaway runtime, not
    // tauri::async_runtime::spawn - this code also runs inside the
    // privileged helper's bare Windows service process (started via
    // windows_service::service_dispatcher, never through
    // tauri::Builder::run), which has no Tauri/tokio runtime of its own.
    // Spawning onto a runtime that doesn't exist there silently never ran
    // this monitor, leaving "pending" sessions stuck forever and blocking
    // every later apply with "ja existe uma mudanca pendente".
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("Falha ao criar runtime do monitor de rede: {error}");
                if let Ok(mut guard) = guard_slot.lock() {
                    if guard.as_deref() == Some(session_id.as_str()) {
                        *guard = None;
                    }
                }
                return;
            }
        };

        runtime.block_on(async {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let Some(session) = read_session() else {
                    break;
                };
                if session.id != session_id || session.status != "pending" {
                    break;
                }
                if chrono::Utc::now().timestamp() >= deadline {
                    let result = super::execute_command(
                        "REVERT_NETWORK_TUNE",
                        Some(json!({ "reason": "countdown_expired" })),
                    )
                    .await;
                    let _ = audit::record_event(
                        "info",
                        "network_tune.reverted_after_timeout",
                        "Otimizacao de rede revertida automaticamente por falta de confirmacao.",
                        json!({ "session_id": session_id, "result_success": result.success }),
                    );
                    break;
                }
            }
        });

        if let Ok(mut guard) = guard_slot.lock() {
            if guard.as_deref() == Some(session_id.as_str()) {
                *guard = None;
            }
        }
    });
}

fn read_session() -> Option<NetworkTuneSession> {
    let raw = fs::read_to_string(session_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_session(session: &NetworkTuneSession) -> Result<(), String> {
    let path = session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn session_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join(SESSION_FILE)
}
