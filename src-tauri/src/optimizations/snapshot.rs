use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use uuid::Uuid;

use crate::audit;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationSnapshot {
    pub id: String,
    pub action_name: String,
    pub created_at: i64,
    pub restored_at: Option<i64>,
    pub entries: Vec<SnapshotEntry>,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotEntry {
    PowerPlan {
        previous_scheme_guid: String,
        previous_scheme_name: Option<String>,
        target_scheme: String,
    },
    QuarantinedPath {
        original_path: PathBuf,
        quarantine_path: PathBuf,
        bytes: u64,
    },
    RemovedEmptyDir {
        original_path: PathBuf,
    },
    StartupRegistryValue {
        hive: String,
        subkey: String,
        value_name: String,
        value_type: String,
        value_bytes: Vec<u8>,
    },
    ServiceState {
        service_name: String,
        display_name: Option<String>,
        was_running: bool,
        start_type: Option<u32>,
    },
    ProcessPriority {
        pid: u32,
        process_name: String,
        previous_priority_class: u32,
        previous_priority_label: String,
        target_priority_class: u32,
        target_priority_label: String,
    },
    ProcessEfficiency {
        pid: u32,
        process_name: String,
        previous_memory_priority: Option<u32>,
        previous_power_control_mask: Option<u32>,
        previous_power_state_mask: Option<u32>,
        target_memory_priority: Option<u32>,
        target_power_state_mask: Option<u32>,
    },
    ProcessAffinity {
        pid: u32,
        process_name: String,
        previous_process_mask: usize,
        previous_system_mask: usize,
        target_process_mask: usize,
        strategy: String,
    },
    RegistryValue {
        hive: String,
        subkey: String,
        value_name: String,
        previous_value_type: Option<String>,
        previous_value_bytes: Option<Vec<u8>>,
        target_value_type: String,
        target_value_bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub restored_snapshots: usize,
    pub failed_snapshots: usize,
    pub restored_entries: usize,
    pub failed_entries: usize,
    pub skipped_conflicts: usize,
    pub messages: Vec<String>,
}

#[derive(Debug, Default)]
pub struct SnapshotRestoreSummary {
    pub restored_entries: usize,
    pub failed_entries: usize,
    pub skipped_conflicts: usize,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PowerPlanState {
    pub scheme_guid: String,
    pub scheme_name: Option<String>,
}

impl OptimizationSnapshot {
    pub fn new(
        action_name: impl Into<String>,
        entries: Vec<SnapshotEntry>,
        details: Value,
    ) -> Self {
        Self {
            id: new_snapshot_id(),
            action_name: action_name.into(),
            created_at: chrono::Utc::now().timestamp(),
            restored_at: None,
            entries,
            details,
        }
    }
}

pub fn new_snapshot_id() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn save_snapshot(snapshot: &OptimizationSnapshot) -> Result<(), String> {
    let mut snapshots = read_snapshots()?;
    snapshots.push(snapshot.clone());
    write_snapshots(&snapshots)?;
    let _ = audit::record_event(
        "info",
        "optimization.snapshot_created",
        "Snapshot local criado antes/depois de otimizacao reversivel.",
        serde_json::json!({
            "snapshot_id": snapshot.id,
            "action_name": snapshot.action_name,
            "entries": snapshot.entries.len(),
        }),
    );
    Ok(())
}

pub fn list_snapshots(limit: usize) -> Result<Vec<OptimizationSnapshot>, String> {
    let mut snapshots = read_snapshots()?;
    snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created_at));
    snapshots.truncate(limit.clamp(1, 250));
    Ok(snapshots)
}

pub fn restore_pending_snapshots() -> Result<RestoreReport, String> {
    let mut snapshots = read_snapshots()?;
    let mut pending: Vec<_> = snapshots
        .iter()
        .enumerate()
        .filter(|(_, snapshot)| snapshot.restored_at.is_none())
        .map(|(index, snapshot)| (index, snapshot.created_at))
        .collect();

    pending.sort_by_key(|pending| std::cmp::Reverse(pending.1));

    let mut report = RestoreReport {
        restored_snapshots: 0,
        failed_snapshots: 0,
        restored_entries: 0,
        failed_entries: 0,
        skipped_conflicts: 0,
        messages: Vec::new(),
    };

    for (index, _) in pending {
        let summary = restore_snapshot_entries(&snapshots[index]);
        report.restored_entries += summary.restored_entries;
        report.failed_entries += summary.failed_entries;
        report.skipped_conflicts += summary.skipped_conflicts;
        report.messages.extend(summary.messages);

        if summary.failed_entries == 0 && summary.skipped_conflicts == 0 {
            snapshots[index].restored_at = Some(chrono::Utc::now().timestamp());
            report.restored_snapshots += 1;
        } else {
            report.failed_snapshots += 1;
        }
    }

    write_snapshots(&snapshots)?;
    let _ = audit::record_event(
        "info",
        "optimization.snapshots_restored",
        "Snapshots pendentes restaurados pelo agente local.",
        serde_json::to_value(&report).unwrap_or(Value::Null),
    );
    Ok(report)
}

pub fn restore_startup_app_snapshots(target: Option<&str>) -> Result<RestoreReport, String> {
    restore_snapshots_matching(|snapshot| {
        snapshot.restored_at.is_none()
            && snapshot.entries.iter().any(|entry| match entry {
                SnapshotEntry::StartupRegistryValue { value_name, .. } => target
                    .map(|target| value_name.eq_ignore_ascii_case(target))
                    .unwrap_or(true),
                _ => false,
            })
    })
}

pub fn restore_service_snapshots(target: Option<&str>) -> Result<RestoreReport, String> {
    restore_snapshots_matching(|snapshot| {
        snapshot.restored_at.is_none()
            && snapshot.entries.iter().any(|entry| match entry {
                SnapshotEntry::ServiceState { service_name, .. } => target
                    .map(|target| service_name.eq_ignore_ascii_case(target))
                    .unwrap_or(true),
                _ => false,
            })
    })
}

pub fn restore_visual_effect_snapshots() -> Result<RestoreReport, String> {
    restore_snapshots_matching(|snapshot| {
        snapshot.restored_at.is_none()
            && snapshot.action_name == "APPLY_VISUAL_PERFORMANCE_MODE"
            && snapshot
                .entries
                .iter()
                .any(|entry| matches!(entry, SnapshotEntry::RegistryValue { .. }))
    })
}

pub fn restore_snapshots_by_ids(snapshot_ids: &[String]) -> RestoreReport {
    let mut snapshots = match read_snapshots() {
        Ok(snapshots) => snapshots,
        Err(error) => {
            return RestoreReport {
                restored_snapshots: 0,
                failed_snapshots: 1,
                restored_entries: 0,
                failed_entries: 1,
                skipped_conflicts: 0,
                messages: vec![format!("Falha ao ler snapshots: {error}")],
            };
        }
    };
    let mut report = RestoreReport {
        restored_snapshots: 0,
        failed_snapshots: 0,
        restored_entries: 0,
        failed_entries: 0,
        skipped_conflicts: 0,
        messages: Vec::new(),
    };

    for snapshot_id in snapshot_ids.iter().rev() {
        let Some(index) = snapshots
            .iter()
            .position(|snapshot| snapshot.restored_at.is_none() && snapshot.id == *snapshot_id)
        else {
            continue;
        };

        let summary = restore_snapshot_entries(&snapshots[index]);
        report.restored_entries += summary.restored_entries;
        report.failed_entries += summary.failed_entries;
        report.skipped_conflicts += summary.skipped_conflicts;
        report.messages.extend(summary.messages);

        if summary.failed_entries == 0 && summary.skipped_conflicts == 0 {
            snapshots[index].restored_at = Some(chrono::Utc::now().timestamp());
            report.restored_snapshots += 1;
        } else {
            report.failed_snapshots += 1;
        }
    }

    if let Err(error) = write_snapshots(&snapshots) {
        report.failed_snapshots += 1;
        report
            .messages
            .push(format!("Falha ao salvar estado de snapshots: {error}"));
    }

    report
}

pub fn discard_snapshot(snapshot_id: &str) -> Result<(), String> {
    let mut snapshots = read_snapshots()?;
    let before = snapshots.len();
    snapshots.retain(|snapshot| snapshot.id != snapshot_id);
    if snapshots.len() != before {
        write_snapshots(&snapshots)?;
    }
    Ok(())
}

pub fn mark_cleanup_snapshots_purged() -> Result<usize, String> {
    let mut snapshots = read_snapshots()?;
    let now = chrono::Utc::now().timestamp();
    let mut changed = 0;

    for snapshot in &mut snapshots {
        if snapshot.restored_at.is_some() || snapshot.action_name != "EMPTY_TEMP" {
            continue;
        }

        let has_quarantined_entries = snapshot
            .entries
            .iter()
            .any(|entry| matches!(entry, SnapshotEntry::QuarantinedPath { .. }));
        if !has_quarantined_entries {
            continue;
        }

        snapshot.restored_at = Some(now);
        if let Some(details) = snapshot.details.as_object_mut() {
            details.insert("purged".to_string(), Value::Bool(true));
            details.insert("purged_at".to_string(), Value::Number(now.into()));
            details.insert("reversible".to_string(), Value::Bool(false));
        }
        changed += 1;
    }

    if changed > 0 {
        write_snapshots(&snapshots)?;
        let _ = audit::record_event(
            "info",
            "optimization.cleanup_quarantine_purged",
            "Snapshots de limpeza TEMP marcados como purgados apos exclusao permanente da quarentena.",
            serde_json::json!({ "snapshots": changed }),
        );
    }

    Ok(changed)
}

pub fn restore_snapshot_entries(snapshot: &OptimizationSnapshot) -> SnapshotRestoreSummary {
    let mut summary = SnapshotRestoreSummary::default();

    for entry in snapshot.entries.iter().rev() {
        match entry {
            SnapshotEntry::PowerPlan {
                previous_scheme_guid,
                previous_scheme_name,
                ..
            } => match set_active_power_plan(previous_scheme_guid) {
                Ok(()) => {
                    summary.restored_entries += 1;
                    summary.messages.push(format!(
                        "Plano de energia restaurado para {}.",
                        previous_scheme_name
                            .as_deref()
                            .unwrap_or(previous_scheme_guid.as_str())
                    ));
                }
                Err(error) => {
                    summary.failed_entries += 1;
                    summary
                        .messages
                        .push(format!("Falha ao restaurar plano de energia: {error}"));
                }
            },
            SnapshotEntry::QuarantinedPath {
                original_path,
                quarantine_path,
                ..
            } => {
                if !quarantine_path.exists() {
                    summary.failed_entries += 1;
                    summary.messages.push(format!(
                        "Arquivo em quarentena nao encontrado: {}",
                        quarantine_path.display()
                    ));
                    continue;
                }

                if original_path.exists() {
                    summary.skipped_conflicts += 1;
                    summary.messages.push(format!(
                        "Destino ja existe, restauracao ignorada: {}",
                        original_path.display()
                    ));
                    continue;
                }

                if let Some(parent) = original_path.parent() {
                    if let Err(error) = fs::create_dir_all(parent) {
                        summary.failed_entries += 1;
                        summary.messages.push(format!(
                            "Falha ao recriar diretorio {}: {error}",
                            parent.display()
                        ));
                        continue;
                    }
                }

                match move_file_across_volumes(quarantine_path, original_path) {
                    Ok(()) => summary.restored_entries += 1,
                    Err(error) => {
                        summary.failed_entries += 1;
                        summary.messages.push(format!(
                            "Falha ao restaurar {}: {error}",
                            original_path.display()
                        ));
                    }
                }
            }
            SnapshotEntry::RemovedEmptyDir { original_path } => {
                match fs::create_dir_all(original_path) {
                    Ok(()) => summary.restored_entries += 1,
                    Err(error) => {
                        summary.failed_entries += 1;
                        summary.messages.push(format!(
                            "Falha ao restaurar diretorio {}: {error}",
                            original_path.display()
                        ));
                    }
                }
            }
            SnapshotEntry::StartupRegistryValue {
                hive,
                subkey,
                value_name,
                value_type,
                value_bytes,
            } => match restore_registry_value(hive, subkey, value_name, value_type, value_bytes) {
                Ok(()) => {
                    summary.restored_entries += 1;
                    summary
                        .messages
                        .push(format!("App de inicializacao restaurado: {value_name}."));
                }
                Err(error) => {
                    summary.failed_entries += 1;
                    summary.messages.push(format!(
                        "Falha ao restaurar app de inicializacao {value_name}: {error}"
                    ));
                }
            },
            SnapshotEntry::ServiceState {
                service_name,
                was_running,
                ..
            } => {
                if !was_running {
                    summary.restored_entries += 1;
                    summary.messages.push(format!(
                        "Servico {service_name} estava parado antes da acao; nada a restaurar."
                    ));
                    continue;
                }

                match start_service(service_name) {
                    Ok(()) => {
                        summary.restored_entries += 1;
                        summary
                            .messages
                            .push(format!("Servico restaurado: {service_name}."));
                    }
                    Err(error) => {
                        summary.failed_entries += 1;
                        summary
                            .messages
                            .push(format!("Falha ao iniciar servico {service_name}: {error}"));
                    }
                }
            }
            SnapshotEntry::ProcessPriority {
                pid,
                process_name,
                previous_priority_class,
                previous_priority_label,
                ..
            } => {
                if !super::processes::process_exists_by_pid(*pid) {
                    summary.restored_entries += 1;
                    summary.messages.push(format!(
                        "Processo {process_name} ({pid}) ja saiu; prioridade nao precisa ser restaurada."
                    ));
                    continue;
                }

                match super::processes::set_process_priority_class_by_pid(
                    *pid,
                    *previous_priority_class,
                ) {
                    Ok(()) => {
                        summary.restored_entries += 1;
                        summary.messages.push(format!(
                            "Prioridade de {process_name} restaurada para {previous_priority_label}."
                        ));
                    }
                    Err(error) => {
                        summary.failed_entries += 1;
                        summary.messages.push(format!(
                            "Falha ao restaurar prioridade de {process_name}: {error}"
                        ));
                    }
                }
            }
            SnapshotEntry::ProcessEfficiency {
                pid,
                process_name,
                previous_memory_priority,
                previous_power_control_mask,
                previous_power_state_mask,
                ..
            } => {
                if !super::processes::process_exists_by_pid(*pid) {
                    summary.restored_entries += 1;
                    summary.messages.push(format!(
                        "Processo {process_name} ({pid}) ja saiu; eficiencia nao precisa ser restaurada."
                    ));
                    continue;
                }

                match super::processes::restore_process_efficiency_by_pid(
                    *pid,
                    *previous_memory_priority,
                    *previous_power_control_mask,
                    *previous_power_state_mask,
                ) {
                    Ok(()) => {
                        summary.restored_entries += 1;
                        summary
                            .messages
                            .push(format!("Eficiencia de {process_name} restaurada."));
                    }
                    Err(error) => {
                        summary.failed_entries += 1;
                        summary.messages.push(format!(
                            "Falha ao restaurar eficiencia de {process_name}: {error}"
                        ));
                    }
                }
            }
            SnapshotEntry::ProcessAffinity {
                pid,
                process_name,
                previous_process_mask,
                ..
            } => {
                if !super::processes::process_exists_by_pid(*pid) {
                    summary.restored_entries += 1;
                    summary.messages.push(format!(
                        "Processo {process_name} ({pid}) ja saiu; afinidade nao precisa ser restaurada."
                    ));
                    continue;
                }

                match super::processes::restore_process_affinity_by_pid(
                    *pid,
                    *previous_process_mask,
                ) {
                    Ok(()) => {
                        summary.restored_entries += 1;
                        summary
                            .messages
                            .push(format!("Afinidade de {process_name} restaurada."));
                    }
                    Err(error) => {
                        summary.failed_entries += 1;
                        summary.messages.push(format!(
                            "Falha ao restaurar afinidade de {process_name}: {error}"
                        ));
                    }
                }
            }
            SnapshotEntry::RegistryValue {
                hive,
                subkey,
                value_name,
                previous_value_type,
                previous_value_bytes,
                ..
            } => match (previous_value_type, previous_value_bytes) {
                (Some(value_type), Some(value_bytes)) => {
                    match restore_registry_value(hive, subkey, value_name, value_type, value_bytes)
                    {
                        Ok(()) => {
                            summary.restored_entries += 1;
                            notify_user_settings_changed();
                            summary
                                .messages
                                .push(format!("Valor visual do Windows restaurado: {value_name}."));
                        }
                        Err(error) => {
                            summary.failed_entries += 1;
                            summary.messages.push(format!(
                                "Falha ao restaurar valor visual {value_name}: {error}"
                            ));
                        }
                    }
                }
                _ => match delete_registry_value(hive, subkey, value_name) {
                    Ok(()) => {
                        summary.restored_entries += 1;
                        notify_user_settings_changed();
                        summary.messages.push(format!(
                            "Valor visual criado pela otimizacao removido: {value_name}."
                        ));
                    }
                    Err(error) => {
                        summary.failed_entries += 1;
                        summary.messages.push(format!(
                            "Falha ao remover valor visual {value_name}: {error}"
                        ));
                    }
                },
            },
        }
    }

    summary
}

pub fn active_power_plan() -> Result<PowerPlanState, String> {
    let output = Command::new("powercfg")
        .arg("/getactivescheme")
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    parse_power_plan_output(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "Nao foi possivel identificar o plano de energia ativo.".to_string())
}

pub fn set_active_power_plan(scheme_guid_or_alias: &str) -> Result<(), String> {
    let output = Command::new("powercfg")
        .args(["/setactive", scheme_guid_or_alias])
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn cleanup_quarantine_root() -> PathBuf {
    app_data_dir().join("quarantine")
}

pub fn cleanup_quarantine_dir(snapshot_id: &str) -> PathBuf {
    cleanup_quarantine_root().join(snapshot_id)
}

pub fn move_file_across_volumes(source: &PathBuf, target: &PathBuf) -> Result<(), String> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            fs::copy(source, target).map_err(|copy_error| {
                format!("rename failed: {rename_error}; copy failed: {copy_error}")
            })?;
            fs::remove_file(source).map_err(|remove_error| {
                let _ = fs::remove_file(target);
                format!("rename failed: {rename_error}; copied file cleanup failed: {remove_error}")
            })
        }
    }
}

pub fn app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
        .join("AnalystBlaze")
        .join("agent")
}

fn read_snapshots() -> Result<Vec<OptimizationSnapshot>, String> {
    let path = snapshot_file_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn write_snapshots(snapshots: &[OptimizationSnapshot]) -> Result<(), String> {
    let path = snapshot_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let raw = serde_json::to_string_pretty(snapshots).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn restore_snapshots_matching(
    predicate: impl Fn(&OptimizationSnapshot) -> bool,
) -> Result<RestoreReport, String> {
    let mut snapshots = read_snapshots()?;
    let mut pending: Vec<_> = snapshots
        .iter()
        .enumerate()
        .filter(|(_, snapshot)| predicate(snapshot))
        .map(|(index, snapshot)| (index, snapshot.created_at))
        .collect();

    pending.sort_by_key(|pending| std::cmp::Reverse(pending.1));

    let mut report = RestoreReport {
        restored_snapshots: 0,
        failed_snapshots: 0,
        restored_entries: 0,
        failed_entries: 0,
        skipped_conflicts: 0,
        messages: Vec::new(),
    };

    for (index, _) in pending {
        let summary = restore_snapshot_entries(&snapshots[index]);
        report.restored_entries += summary.restored_entries;
        report.failed_entries += summary.failed_entries;
        report.skipped_conflicts += summary.skipped_conflicts;
        report.messages.extend(summary.messages);

        if summary.failed_entries == 0 && summary.skipped_conflicts == 0 {
            snapshots[index].restored_at = Some(chrono::Utc::now().timestamp());
            report.restored_snapshots += 1;
        } else {
            report.failed_snapshots += 1;
        }
    }

    write_snapshots(&snapshots)?;
    Ok(report)
}

fn snapshot_file_path() -> PathBuf {
    app_data_dir().join("optimization-snapshots.json")
}

#[cfg(windows)]
fn restore_registry_value(
    hive: &str,
    subkey: &str,
    value_name: &str,
    value_type: &str,
    value_bytes: &[u8],
) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::{RegKey, RegValue};

    let root = match hive.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => RegKey::predef(HKEY_CURRENT_USER),
        "HKLM" | "HKEY_LOCAL_MACHINE" => RegKey::predef(HKEY_LOCAL_MACHINE),
        _ => return Err("Hive de registro nao suportada para restauracao.".to_string()),
    };

    let (key, _) = root
        .create_subkey(subkey)
        .map_err(|error| error.to_string())?;
    let reg_value = RegValue {
        vtype: registry_type_from_name(value_type)?,
        bytes: value_bytes.to_vec(),
    };
    key.set_raw_value(value_name, &reg_value)
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn delete_registry_value(hive: &str, subkey: &str, value_name: &str) -> Result<(), String> {
    use std::io;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WRITE};
    use winreg::RegKey;

    let root = match hive.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => RegKey::predef(HKEY_CURRENT_USER),
        "HKLM" | "HKEY_LOCAL_MACHINE" => RegKey::predef(HKEY_LOCAL_MACHINE),
        _ => return Err("Hive de registro nao suportada para restauracao.".to_string()),
    };

    let key = match root.open_subkey_with_flags(subkey, KEY_WRITE) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    match key.delete_value(value_name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(windows))]
fn restore_registry_value(
    _hive: &str,
    _subkey: &str,
    _value_name: &str,
    _value_type: &str,
    _value_bytes: &[u8],
) -> Result<(), String> {
    Err("Registro do Windows indisponivel nesta plataforma.".to_string())
}

#[cfg(not(windows))]
fn delete_registry_value(_hive: &str, _subkey: &str, _value_name: &str) -> Result<(), String> {
    Err("Registro do Windows indisponivel nesta plataforma.".to_string())
}

#[cfg(windows)]
fn registry_type_from_name(value_type: &str) -> Result<winreg::enums::RegType, String> {
    use winreg::enums::RegType::*;

    match value_type {
        "REG_NONE" => Ok(REG_NONE),
        "REG_SZ" => Ok(REG_SZ),
        "REG_EXPAND_SZ" => Ok(REG_EXPAND_SZ),
        "REG_BINARY" => Ok(REG_BINARY),
        "REG_DWORD" => Ok(REG_DWORD),
        "REG_DWORD_BIG_ENDIAN" => Ok(REG_DWORD_BIG_ENDIAN),
        "REG_LINK" => Ok(REG_LINK),
        "REG_MULTI_SZ" => Ok(REG_MULTI_SZ),
        "REG_RESOURCE_LIST" => Ok(REG_RESOURCE_LIST),
        "REG_FULL_RESOURCE_DESCRIPTOR" => Ok(REG_FULL_RESOURCE_DESCRIPTOR),
        "REG_RESOURCE_REQUIREMENTS_LIST" => Ok(REG_RESOURCE_REQUIREMENTS_LIST),
        "REG_QWORD" => Ok(REG_QWORD),
        other => Err(format!("Tipo de valor de registro nao suportado: {other}")),
    }
}

fn start_service(service_name: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let output = Command::new("sc.exe")
            .args(["start", service_name])
            .output()
            .map_err(|error| error.to_string())?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if output.status.success() || stdout.to_ascii_lowercase().contains("already") {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("{}{}", stdout.trim(), stderr.trim()))
        }
    }

    #[cfg(not(windows))]
    {
        let _ = service_name;
        Err("Servicos do Windows indisponiveis nesta plataforma.".to_string())
    }
}

fn notify_user_settings_changed() {
    #[cfg(windows)]
    {
        let _ = Command::new("rundll32.exe")
            .args(["user32.dll,UpdatePerUserSystemParameters"])
            .status();
        let _ = Command::new("ie4uinit.exe").arg("-show").status();
    }
}

fn parse_power_plan_output(output: &str) -> Option<PowerPlanState> {
    let scheme_guid = output
        .split(|character: char| !(character.is_ascii_hexdigit() || character == '-'))
        .find(|part| looks_like_guid(part))?
        .to_string();

    let scheme_name = output
        .split_once('(')
        .and_then(|(_, right)| {
            right
                .split_once(')')
                .map(|(name, _)| name.trim().to_string())
        })
        .filter(|name| !name.is_empty());

    Some(PowerPlanState {
        scheme_guid,
        scheme_name,
    })
}

fn looks_like_guid(value: &str) -> bool {
    value.len() == 36
        && value
            .chars()
            .enumerate()
            .all(|(index, character)| match index {
                8 | 13 | 18 | 23 => character == '-',
                _ => character.is_ascii_hexdigit(),
            })
}

#[cfg(test)]
mod tests {
    use super::parse_power_plan_output;

    #[test]
    fn parses_english_powercfg_output() {
        let parsed = parse_power_plan_output(
            "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)",
        )
        .expect("power plan");

        assert_eq!(parsed.scheme_guid, "381b4222-f694-41f0-9685-ff5bb260df2e");
        assert_eq!(parsed.scheme_name.as_deref(), Some("Balanced"));
    }

    #[test]
    fn parses_localized_powercfg_output_by_guid() {
        let parsed = parse_power_plan_output(
            "GUID do Esquema de Energia: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (Alto desempenho)",
        )
        .expect("power plan");

        assert_eq!(parsed.scheme_guid, "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c");
        assert_eq!(parsed.scheme_name.as_deref(), Some("Alto desempenho"));
    }
}
