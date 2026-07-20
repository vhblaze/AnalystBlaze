use serde_json::{json, Value};
use std::process::Command;

use super::{
    safety,
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    windows_inventory, ExecutionResult,
};
use crate::process_ext::{decode_console_bytes, CommandExt};

pub async fn disable_startup_app(payload: Option<Value>) -> ExecutionResult {
    let target = extract_payload_string(payload.as_ref(), &["target", "name", "app", "value_name"]);
    let location = extract_payload_string(payload.as_ref(), &["location", "registry_location"]);
    let fallback_payload = payload.clone();

    let Some(target) = target else {
        return ExecutionResult {
            success: false,
            message: "Informe o nome do app de inicializacao.".to_string(),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        };
    };

    match tokio::task::spawn_blocking(move || {
        disable_startup_app_sync(&target, location.as_deref())
    })
    .await
    {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao desativar app de inicializacao: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
    }
}

pub async fn restore_startup_app(payload: Option<Value>) -> ExecutionResult {
    let target = extract_payload_string(payload.as_ref(), &["target", "name", "app", "value_name"]);
    let fallback_payload = payload.clone();

    match tokio::task::spawn_blocking(move || {
        snapshot::restore_startup_app_snapshots(target.as_deref())
    })
    .await
    {
        Ok(Ok(report)) => {
            let success = report.failed_snapshots == 0 && report.failed_entries == 0;
            ExecutionResult {
                success,
                message: if report.restored_snapshots == 0 {
                    "Nenhum snapshot de app de inicializacao pendente para restaurar.".to_string()
                } else if success {
                    "App(s) de inicializacao restaurado(s) por snapshot local.".to_string()
                } else {
                    "Restauracao de app de inicializacao concluida com falhas.".to_string()
                },
                details: json!({
                    "implemented": true,
                    "payload": fallback_payload,
                    "restore": report,
                }),
            }
        }
        Ok(Err(error)) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar app de inicializacao: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar app de inicializacao: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
    }
}

pub async fn stop_service(payload: Option<Value>) -> ExecutionResult {
    let target = extract_payload_string(
        payload.as_ref(),
        &["target", "service", "service_name", "name"],
    );
    let fallback_payload = payload.clone();

    let Some(service_name) = target else {
        return ExecutionResult {
            success: false,
            message: "Informe o nome do servico do Windows.".to_string(),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        };
    };

    match tokio::task::spawn_blocking(move || stop_service_sync(&service_name)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao parar servico: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
    }
}

pub async fn restore_service(payload: Option<Value>) -> ExecutionResult {
    let target = extract_payload_string(
        payload.as_ref(),
        &["target", "service", "service_name", "name"],
    );
    let fallback_payload = payload.clone();

    match tokio::task::spawn_blocking(move || {
        snapshot::restore_service_snapshots(target.as_deref())
    })
    .await
    {
        Ok(Ok(report)) => {
            let success = report.failed_snapshots == 0 && report.failed_entries == 0;
            ExecutionResult {
                success,
                message: if report.restored_snapshots == 0 {
                    "Nenhum snapshot de servico pendente para restaurar.".to_string()
                } else if success {
                    "Servico(s) restaurado(s) por snapshot local.".to_string()
                } else {
                    "Restauracao de servico concluida com falhas.".to_string()
                },
                details: json!({
                    "implemented": true,
                    "payload": fallback_payload,
                    "restore": report,
                }),
            }
        }
        Ok(Err(error)) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar servico: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar servico: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
    }
}

#[cfg(windows)]
fn disable_startup_app_sync(target: &str, location: Option<&str>) -> ExecutionResult {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let Some((hive, subkey)) = resolve_startup_location(target, location) else {
        return ExecutionResult {
            success: false,
            message: "App de inicializacao nao encontrado no inventario local.".to_string(),
            details: json!({
                "implemented": true,
                "target": target,
                "location": location,
            }),
        };
    };

    if !is_allowed_startup_subkey(&subkey) {
        return ExecutionResult {
            success: false,
            message: "Local de inicializacao nao permitido pela camada local.".to_string(),
            details: json!({
                "implemented": true,
                "target": target,
                "hive": hive,
                "subkey": subkey,
            }),
        };
    }

    let root = match hive.as_str() {
        "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
        "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
        _ => {
            return ExecutionResult {
                success: false,
                message: "Hive de registro nao suportada.".to_string(),
                details: json!({ "implemented": true, "hive": hive }),
            }
        }
    };

    let key = match root.open_subkey_with_flags(&subkey, KEY_READ | KEY_WRITE) {
        Ok(key) => key,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel abrir a chave de inicializacao para escrita."
                    .to_string(),
                details: json!({
                    "implemented": true,
                    "target": target,
                    "hive": hive,
                    "subkey": subkey,
                    "requires_admin": hive == "HKLM",
                    "error": error.to_string(),
                }),
            }
        }
    };

    let raw_value = match key.get_raw_value(target) {
        Ok(value) => value,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Valor de inicializacao nao encontrado.".to_string(),
                details: json!({
                    "implemented": true,
                    "target": target,
                    "hive": hive,
                    "subkey": subkey,
                    "error": error.to_string(),
                }),
            }
        }
    };

    let snapshot = OptimizationSnapshot::new(
        "DISABLE_STARTUP_APP",
        vec![SnapshotEntry::StartupRegistryValue {
            hive: hive.clone(),
            subkey: subkey.clone(),
            value_name: target.to_string(),
            value_type: format!("{:?}", raw_value.vtype),
            value_bytes: raw_value.bytes.clone(),
        }],
        json!({
            "target": target,
            "hive": hive,
            "subkey": subkey,
            "value_type": format!("{:?}", raw_value.vtype),
            "command_preview": raw_value.to_string(),
        }),
    );

    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        return ExecutionResult {
            success: false,
            message: "A alteracao foi bloqueada porque o snapshot nao pode ser salvo.".to_string(),
            details: json!({
                "implemented": true,
                "target": target,
                "snapshot_error": error,
            }),
        };
    }

    if let Err(error) = key.delete_value(target) {
        let _ = snapshot::discard_snapshot(&snapshot.id);
        return ExecutionResult {
            success: false,
            message: "Nao foi possivel remover o app da inicializacao.".to_string(),
            details: json!({
                "implemented": true,
                "target": target,
                "snapshot_discarded": true,
                "error": error.to_string(),
            }),
        };
    }

    ExecutionResult::ok(
        "App removido da inicializacao com snapshot reversivel.",
        json!({
            "implemented": true,
            "target": target,
            "hive": hive,
            "subkey": subkey,
            "snapshot": {
                "id": snapshot.id,
                "entries": snapshot.entries.len(),
                "reversible": true,
            },
        }),
    )
}

#[cfg(not(windows))]
fn disable_startup_app_sync(target: &str, location: Option<&str>) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Apps de inicializacao do Windows indisponiveis nesta plataforma.".to_string(),
        details: json!({
            "implemented": true,
            "target": target,
            "location": location,
        }),
    }
}

#[cfg(windows)]
fn stop_service_sync(service_name: &str) -> ExecutionResult {
    if safety::is_critical_service(service_name) {
        return ExecutionResult {
            success: false,
            message: "Servico critico protegido pela denylist local.".to_string(),
            details: json!({
                "implemented": true,
                "service": service_name,
                "blocked_by": "critical_service_denylist",
            }),
        };
    }

    let state = match query_service_state(service_name) {
        Ok(state) => state,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Servico nao encontrado ou inacessivel.".to_string(),
                details: json!({
                    "implemented": true,
                    "service": service_name,
                    "error": error,
                }),
            }
        }
    };

    if !state.running {
        return ExecutionResult::ok(
            "Servico ja estava parado; nenhuma alteracao aplicada.",
            json!({
                "implemented": true,
                "service": service_name,
                "changed": false,
                "snapshot": null,
            }),
        );
    }

    let snapshot = OptimizationSnapshot::new(
        "STOP_SERVICE",
        vec![SnapshotEntry::ServiceState {
            service_name: service_name.to_string(),
            display_name: state.display_name.clone(),
            was_running: state.running,
            start_type: state.start_type,
        }],
        json!({
            "service": service_name,
            "display_name": state.display_name,
            "was_running": state.running,
            "start_type": state.start_type,
        }),
    );

    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        return ExecutionResult {
            success: false,
            message: "A alteracao foi bloqueada porque o snapshot nao pode ser salvo.".to_string(),
            details: json!({
                "implemented": true,
                "service": service_name,
                "snapshot_error": error,
            }),
        };
    }

    let output = match Command::new("sc.exe").args(["stop", service_name]).no_window().output() {
        Ok(output) => output,
        Err(error) => {
            let _ = snapshot::discard_snapshot(&snapshot.id);
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel chamar o Service Control Manager.".to_string(),
                details: json!({
                    "implemented": true,
                    "service": service_name,
                    "snapshot_discarded": true,
                    "error": error.to_string(),
                }),
            };
        }
    };

    let stdout = decode_console_bytes(&output.stdout);
    let stderr = decode_console_bytes(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let accepted = output.status.success()
        || combined.contains("STOP_PENDING")
        || combined.contains("STOPPED")
        || combined.to_ascii_lowercase().contains("already stopped");

    if !accepted {
        let _ = snapshot::discard_snapshot(&snapshot.id);
        return ExecutionResult {
            success: false,
            message: "O Windows recusou parar o servico.".to_string(),
            details: json!({
                "implemented": true,
                "service": service_name,
                "snapshot_discarded": true,
                "requires_admin": access_denied(&combined),
                "stdout": stdout.trim(),
                "stderr": stderr.trim(),
            }),
        };
    }

    ExecutionResult::ok(
        "Servico parado com snapshot reversivel.",
        json!({
            "implemented": true,
            "service": service_name,
            "changed": true,
            "snapshot": {
                "id": snapshot.id,
                "entries": snapshot.entries.len(),
                "reversible": true,
            },
            "stdout": stdout.trim(),
        }),
    )
}

#[cfg(not(windows))]
fn stop_service_sync(service_name: &str) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Servicos do Windows indisponiveis nesta plataforma.".to_string(),
        details: json!({ "implemented": true, "service": service_name }),
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct ServiceState {
    running: bool,
    display_name: Option<String>,
    start_type: Option<u32>,
}

#[cfg(windows)]
fn query_service_state(service_name: &str) -> Result<ServiceState, String> {
    let output = Command::new("sc.exe")
        .args(["query", service_name])
        .no_window()
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(decode_console_bytes(&output.stderr).trim().to_string());
    }

    let stdout = decode_console_bytes(&output.stdout);
    let running = stdout.contains("RUNNING")
        || stdout.contains("START_PENDING")
        || stdout.contains("PAUSED")
        || stdout.contains("PAUSE_PENDING")
        || stdout.contains("CONTINUE_PENDING");

    let inventory_match = windows_inventory::collect_windows_inventory()
        .services
        .into_iter()
        .find(|service| service.name.eq_ignore_ascii_case(service_name));

    Ok(ServiceState {
        running,
        display_name: inventory_match
            .as_ref()
            .and_then(|service| service.display_name.clone()),
        start_type: inventory_match.and_then(|service| service.start_type),
    })
}

#[cfg(windows)]
fn resolve_startup_location(target: &str, location: Option<&str>) -> Option<(String, String)> {
    if let Some(location) = location.and_then(parse_startup_location) {
        return Some(location);
    }

    windows_inventory::collect_windows_inventory()
        .startup_apps
        .into_iter()
        .find(|app| app.name.eq_ignore_ascii_case(target))
        .and_then(|app| parse_startup_location(&app.location))
}

#[cfg(windows)]
fn parse_startup_location(location: &str) -> Option<(String, String)> {
    let normalized = location.trim().replace('/', "\\");
    let (hive, subkey) = normalized.split_once('\\')?;
    let hive = match hive.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => "HKCU",
        "HKLM" | "HKEY_LOCAL_MACHINE" => "HKLM",
        _ => return None,
    };
    Some((hive.to_string(), subkey.trim_matches('\\').to_string()))
}

#[cfg(windows)]
fn is_allowed_startup_subkey(subkey: &str) -> bool {
    matches!(
        subkey.to_ascii_lowercase().as_str(),
        "software\\microsoft\\windows\\currentversion\\run"
            | "software\\microsoft\\windows\\currentversion\\runonce"
    )
}

#[cfg(windows)]
fn access_denied(output: &str) -> bool {
    let normalized = output.to_ascii_lowercase();
    normalized.contains("access is denied")
        || normalized.contains("acesso negado")
        || normalized.contains("error 5")
        || normalized.contains("erro 5")
}

fn extract_payload_string(payload: Option<&Value>, keys: &[&str]) -> Option<String> {
    let payload = payload?;
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
