use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use sysinfo::{ProcessesToUpdate, System};

use super::{
    detection::GameDetection,
    protected_apps, safety,
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};

const IDLE_PRIORITY_CLASS_RAW: u32 = 0x0000_0040;
const NORMAL_PRIORITY_CLASS_RAW: u32 = 0x0000_0020;
const HIGH_PRIORITY_CLASS_RAW: u32 = 0x0000_0080;
const REALTIME_PRIORITY_CLASS_RAW: u32 = 0x0000_0100;
const BELOW_NORMAL_PRIORITY_CLASS_RAW: u32 = 0x0000_4000;
const ABOVE_NORMAL_PRIORITY_CLASS_RAW: u32 = 0x0000_8000;
const MEMORY_PRIORITY_LOW_RAW: u32 = 2;
const MEMORY_PRIORITY_NORMAL_RAW: u32 = 5;
const PROCESS_POWER_THROTTLING_EXECUTION_SPEED_RAW: u32 = 0x1;
const PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION_RAW: u32 = 0x4;
const BACKGROUND_POWER_THROTTLING_MASK: u32 = PROCESS_POWER_THROTTLING_EXECUTION_SPEED_RAW
    | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION_RAW;

const DEFAULT_BACKGROUND_TARGETS: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "discord.exe",
    "spotify.exe",
    "teams.exe",
    "slack.exe",
    "onedrive.exe",
    "dropbox.exe",
    "googledrivesync.exe",
    "steam.exe",
    "steamwebhelper.exe",
    "epicwebhelper.exe",
    "msedgewebview2.exe",
    "adobearm.exe",
    "creative cloud.exe",
];

#[derive(Debug, Clone)]
struct PriorityCandidate {
    pid: u32,
    process_name: String,
    target_class: u32,
    target_label: String,
    role: &'static str,
    efficiency_mode: EfficiencyMode,
}

#[derive(Debug, Clone, Serialize)]
struct PriorityChange {
    pid: u32,
    process_name: String,
    previous_priority_class: u32,
    previous_priority_label: String,
    target_priority_class: u32,
    target_priority_label: String,
    role: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EfficiencyMode {
    None,
    Foreground,
    Background,
}

#[derive(Debug, Clone, Serialize)]
struct ProcessEfficiencyState {
    memory_priority: Option<u32>,
    power_control_mask: Option<u32>,
    power_state_mask: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct ProcessEfficiencyChange {
    pid: u32,
    process_name: String,
    previous: ProcessEfficiencyState,
    target_memory_priority: Option<u32>,
    target_power_state_mask: Option<u32>,
    role: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct ProcessAffinityChange {
    pid: u32,
    process_name: String,
    previous_process_mask: usize,
    previous_system_mask: usize,
    target_process_mask: usize,
    strategy: &'static str,
}

#[derive(Debug, Default)]
struct PriorityApplySummary {
    changed: Vec<PriorityChange>,
    efficiency_changed: Vec<ProcessEfficiencyChange>,
    skipped_already_set: usize,
    skipped_protected: usize,
    failed: Vec<Value>,
    snapshot_id: Option<String>,
}

pub async fn set_process_priority(payload: Option<Value>) -> ExecutionResult {
    let Some(payload) = payload else {
        return ExecutionResult {
            success: false,
            message: "Informe o processo alvo e a prioridade desejada.".to_string(),
            details: json!({
                "implemented": true,
                "required": ["process_name|pid", "priority"],
            }),
        };
    };

    let Some(target_class) = requested_priority_class(&payload)
        .filter(|class| *class != REALTIME_PRIORITY_CLASS_RAW && *class != IDLE_PRIORITY_CLASS_RAW)
    else {
        return ExecutionResult {
            success: false,
            message: "Prioridade solicitada nao suportada para ajuste local.".to_string(),
            details: json!({
                "implemented": true,
                "allowed_priorities": ["below_normal", "normal", "above_normal", "high"],
            }),
        };
    };

    let target_label = priority_class_label(target_class).to_string();
    match tokio::task::spawn_blocking(move || {
        let candidates = candidates_from_payload(&payload, target_class, target_label);
        apply_priority_candidates("SET_PROCESS_PRIORITY", candidates)
    })
    .await
    {
        Ok(summary) => priority_result(
            "SET_PROCESS_PRIORITY",
            summary,
            "Prioridade do processo ajustada com snapshot reversivel.",
            "Nenhum processo alvo encontrado para ajustar prioridade.",
        ),
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao ajustar prioridade do processo: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn optimize_game_process_priorities(
    payload: Option<Value>,
    detected_game: &GameDetection,
) -> ExecutionResult {
    let Some(game_pid) = detected_game
        .pid
        .as_deref()
        .and_then(|pid| pid.parse::<u32>().ok())
    else {
        return ExecutionResult::ok(
            "Prioridades ignoradas porque nenhum processo de jogo foi detectado.",
            json!({
                "implemented": true,
                "skipped": true,
                "reason": "no_game_process",
            }),
        );
    };

    let game_name = detected_game
        .process_name
        .clone()
        .unwrap_or_else(|| format!("pid:{game_pid}"));
    let lower_background = payload_bool(payload.as_ref(), "lower_background_processes", true);
    let game_priority = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("game_priority")
                .or_else(|| payload.get("priority"))
                .and_then(Value::as_str)
        })
        .and_then(priority_class_from_name)
        .filter(|class| {
            matches!(
                *class,
                NORMAL_PRIORITY_CLASS_RAW | ABOVE_NORMAL_PRIORITY_CLASS_RAW
            )
        })
        .unwrap_or(ABOVE_NORMAL_PRIORITY_CLASS_RAW);
    let background_priority = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("background_priority")
                .or_else(|| payload.get("backgroundPriority"))
                .and_then(Value::as_str)
        })
        .and_then(priority_class_from_name)
        .filter(|class| *class != REALTIME_PRIORITY_CLASS_RAW)
        .unwrap_or(BELOW_NORMAL_PRIORITY_CLASS_RAW);
    let max_background = payload
        .as_ref()
        .and_then(|payload| payload.get("max_background_processes"))
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .clamp(0, 30) as usize;
    let custom_background_targets = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("background_targets")
                .or_else(|| payload.get("backgroundTargets"))
        })
        .and_then(Value::as_array)
        .map(|targets| {
            targets
                .iter()
                .filter_map(Value::as_str)
                .map(normalize_process_name)
                .filter(|target| !target.is_empty())
                .collect::<HashSet<_>>()
        });
    match tokio::task::spawn_blocking(move || {
        let mut candidates = vec![PriorityCandidate {
            pid: game_pid,
            process_name: game_name,
            target_class: game_priority,
            target_label: priority_class_label(game_priority).to_string(),
            role: "game",
            efficiency_mode: EfficiencyMode::Foreground,
        }];

        if lower_background && max_background > 0 {
            candidates.extend(background_priority_candidates(
                game_pid,
                background_priority,
                max_background,
                custom_background_targets.as_ref(),
            ));
        }

        apply_priority_candidates("APPLY_GAME_MODE_PROCESS_PRIORITIES", candidates)
    })
    .await
    {
        Ok(summary) => priority_result(
            "APPLY_GAME_MODE_PROCESS_PRIORITIES",
            summary,
            "Prioridades do jogo e dos apps de fundo ajustadas.",
            "Prioridades ja estavam adequadas ou nenhum app de fundo elegivel foi encontrado.",
        ),
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao otimizar prioridades do Modo Gamer: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn apply_game_affinity(
    payload: Option<Value>,
    detected_game: &GameDetection,
) -> ExecutionResult {
    let Some(pid) = detected_game
        .pid
        .as_deref()
        .and_then(|pid| pid.parse::<u32>().ok())
    else {
        return ExecutionResult::ok(
            "Afinidade ignorada porque nenhum processo de jogo foi detectado.",
            json!({
                "implemented": true,
                "skipped": true,
                "reason": "no_game_process",
            }),
        );
    };

    let process_name = detected_game
        .process_name
        .clone()
        .unwrap_or_else(|| format!("pid:{pid}"));
    let requested_mask = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("affinityMask")
                .or_else(|| payload.get("affinity_mask"))
        })
        .and_then(parse_affinity_mask);

    match tokio::task::spawn_blocking(move || {
        apply_full_game_affinity_with_snapshot(pid, &process_name, requested_mask)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao ajustar afinidade do processo em jogo: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn apply_foreground_burst_priority(
    payload: Option<Value>,
    detected_game: &GameDetection,
) -> ExecutionResult {
    let Some(pid) = detected_game
        .pid
        .as_deref()
        .and_then(|pid| pid.parse::<u32>().ok())
    else {
        return ExecutionResult::ok(
            "Foreground Burst ignorado porque nenhum processo alvo foi detectado.",
            json!({
                "implemented": true,
                "skipped": true,
                "reason": "no_foreground_target",
                "detection": detected_game,
            }),
        );
    };

    let process_name = detected_game
        .process_name
        .clone()
        .unwrap_or_else(|| format!("pid:{pid}"));
    let priority = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("foreground_priority")
                .or_else(|| payload.get("foregroundPriority"))
                .or_else(|| payload.get("priority"))
                .and_then(Value::as_str)
        })
        .and_then(priority_class_from_name)
        .filter(|class| {
            matches!(
                *class,
                NORMAL_PRIORITY_CLASS_RAW | ABOVE_NORMAL_PRIORITY_CLASS_RAW
            )
        })
        .unwrap_or(ABOVE_NORMAL_PRIORITY_CLASS_RAW);

    match tokio::task::spawn_blocking(move || {
        apply_priority_candidates(
            "APPLY_FOREGROUND_BURST_MODE",
            vec![PriorityCandidate {
                pid,
                process_name,
                target_class: priority,
                target_label: priority_class_label(priority).to_string(),
                role: "foreground_target",
                efficiency_mode: EfficiencyMode::Foreground,
            }],
        )
    })
    .await
    {
        Ok(summary) => priority_result(
            "APPLY_FOREGROUND_BURST_MODE",
            summary,
            "Foreground Burst aplicado ao processo em primeiro plano.",
            "Foreground Burst nao encontrou ajuste necessario para o processo alvo.",
        ),
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao aplicar Foreground Burst: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn apply_background_quiet_mode(payload: Option<Value>) -> ExecutionResult {
    optimize_background_process_priorities(payload).await
}

pub async fn optimize_background_process_priorities(payload: Option<Value>) -> ExecutionResult {
    let background_priority = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("background_priority")
                .or_else(|| payload.get("backgroundPriority"))
                .and_then(Value::as_str)
        })
        .and_then(priority_class_from_name)
        .filter(|class| *class != REALTIME_PRIORITY_CLASS_RAW)
        .unwrap_or(BELOW_NORMAL_PRIORITY_CLASS_RAW);
    let max_background = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("max_background_processes")
                .or_else(|| payload.get("maxBackgroundProcesses"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(0, 30) as usize;
    let custom_background_targets = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("background_targets")
                .or_else(|| payload.get("backgroundTargets"))
        })
        .and_then(Value::as_array)
        .map(|targets| {
            targets
                .iter()
                .filter_map(Value::as_str)
                .map(normalize_process_name)
                .filter(|target| !target.is_empty())
                .collect::<HashSet<_>>()
        });

    match tokio::task::spawn_blocking(move || {
        let candidates = background_priority_candidates(
            0,
            background_priority,
            max_background,
            custom_background_targets.as_ref(),
        );
        apply_priority_candidates("APPLY_PC_CLEAN_FAST_BACKGROUND_PRIORITIES", candidates)
    })
    .await
    {
        Ok(summary) => priority_result(
            "APPLY_PC_CLEAN_FAST_BACKGROUND_PRIORITIES",
            summary,
            "Apps de fundo elegiveis foram rebaixados para reduzir disputa por CPU.",
            "Nenhum app de fundo elegivel precisou de ajuste de prioridade.",
        ),
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao otimizar apps de fundo: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub fn process_exists_by_pid(pid: u32) -> bool {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
        .processes()
        .keys()
        .any(|candidate| candidate.as_u32() == pid)
}

pub fn set_process_priority_class_by_pid(pid: u32, priority_class: u32) -> Result<(), String> {
    set_priority_class_by_pid(pid, priority_class)
}

pub fn restore_process_affinity_by_pid(pid: u32, process_mask: usize) -> Result<(), String> {
    set_process_affinity_by_pid(pid, process_mask)
}

pub fn process_priority_report(pid: u32) -> Value {
    match priority_class_by_pid(pid) {
        Ok(priority_class) => json!({
            "pid": pid,
            "priority_class": priority_class,
            "priority_label": priority_class_label(priority_class),
        }),
        Err(error) => json!({
            "pid": pid,
            "error": error,
        }),
    }
}

fn candidates_from_payload(
    payload: &Value,
    target_class: u32,
    target_label: String,
) -> Vec<PriorityCandidate> {
    let target_pid = payload
        .get("pid")
        .or_else(|| payload.get("process_id"))
        .or_else(|| payload.get("processId"))
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok());
    let target_name = extract_target_name(payload).map(|name| normalize_process_name(&name));
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);

    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let pid_u32 = pid.as_u32();
            let process_name = process.name().to_string_lossy().trim().to_string();
            let normalized = normalize_process_name(&process_name);
            let matches_pid = target_pid.is_some_and(|target_pid| target_pid == pid_u32);
            let matches_name = target_name.as_deref().is_some_and(|target_name| {
                target_name == normalized || normalized.contains(target_name)
            });
            if !matches_pid && !matches_name {
                return None;
            }

            Some(PriorityCandidate {
                pid: pid_u32,
                process_name,
                target_class,
                target_label: target_label.clone(),
                role: "manual_target",
                efficiency_mode: EfficiencyMode::None,
            })
        })
        .take(20)
        .collect()
}

fn background_priority_candidates(
    game_pid: u32,
    target_class: u32,
    max_background: usize,
    custom_targets: Option<&HashSet<String>>,
) -> Vec<PriorityCandidate> {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let current_pid = std::process::id();
    let default_targets = DEFAULT_BACKGROUND_TARGETS
        .iter()
        .map(|target| target.to_string())
        .collect::<HashSet<_>>();
    let targets = custom_targets.unwrap_or(&default_targets);

    let mut candidates = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let pid_u32 = pid.as_u32();
            if pid_u32 == game_pid || pid_u32 == current_pid {
                return None;
            }

            let process_name = process.name().to_string_lossy().trim().to_string();
            let normalized = normalize_process_name(&process_name);
            if !targets.contains(&normalized) {
                return None;
            }

            if safety::is_critical_process(&process_name)
                || protected_apps::is_protected_app(&process_name)
            {
                return None;
            }

            Some((
                process.cpu_usage(),
                PriorityCandidate {
                    pid: pid_u32,
                    process_name,
                    target_class,
                    target_label: priority_class_label(target_class).to_string(),
                    role: "background_app",
                    efficiency_mode: EfficiencyMode::Background,
                },
            ))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    candidates
        .into_iter()
        .map(|(_, candidate)| candidate)
        .take(max_background)
        .collect()
}

fn apply_priority_candidates(
    action_name: &str,
    candidates: Vec<PriorityCandidate>,
) -> PriorityApplySummary {
    let mut summary = PriorityApplySummary::default();
    let mut seen = HashSet::new();

    for candidate in candidates {
        if !seen.insert(candidate.pid) {
            continue;
        }

        if safety::is_critical_process(&candidate.process_name)
            || protected_apps::is_protected_app(&candidate.process_name)
        {
            summary.skipped_protected += 1;
            continue;
        }

        let previous_class = match priority_class_by_pid(candidate.pid) {
            Ok(priority_class) => priority_class,
            Err(error) => {
                summary.failed.push(json!({
                    "pid": candidate.pid,
                    "process_name": candidate.process_name,
                    "role": candidate.role,
                    "error": error,
                }));
                continue;
            }
        };

        let previous_efficiency = if candidate.efficiency_mode == EfficiencyMode::None {
            None
        } else {
            match process_efficiency_state_by_pid(candidate.pid) {
                Ok(state) => Some(state),
                Err(error) => {
                    summary.failed.push(json!({
                        "pid": candidate.pid,
                        "process_name": candidate.process_name,
                        "role": candidate.role,
                        "surface": "process_efficiency_query",
                        "error": error,
                    }));
                    None
                }
            }
        };

        let mut priority_changed = false;
        let mut efficiency_changed = false;

        if previous_class == candidate.target_class {
            summary.skipped_already_set += 1;
        } else {
            if let Err(error) = set_priority_class_by_pid(candidate.pid, candidate.target_class) {
                summary.failed.push(json!({
                    "pid": candidate.pid,
                    "process_name": candidate.process_name,
                    "role": candidate.role,
                    "surface": "process_priority",
                    "error": error,
                }));
                continue;
            }
            match priority_class_by_pid(candidate.pid) {
                Ok(applied_class) if applied_class == candidate.target_class => {
                    priority_changed = true;
                }
                Ok(applied_class) => {
                    summary.failed.push(json!({
                        "pid": candidate.pid,
                        "process_name": candidate.process_name,
                        "role": candidate.role,
                        "requested_priority_class": candidate.target_class,
                        "applied_priority_class": applied_class,
                        "error": "priority_verification_failed",
                    }));
                    continue;
                }
                Err(error) => {
                    summary.failed.push(json!({
                        "pid": candidate.pid,
                        "process_name": candidate.process_name,
                        "role": candidate.role,
                        "error": format!("priority_verification_failed: {error}"),
                    }));
                    continue;
                }
            }
        }

        if let Some(previous) = previous_efficiency {
            let target_memory_priority = match candidate.efficiency_mode {
                EfficiencyMode::Background => Some(MEMORY_PRIORITY_LOW_RAW),
                EfficiencyMode::Foreground => Some(MEMORY_PRIORITY_NORMAL_RAW),
                EfficiencyMode::None => None,
            };
            let target_power_state_mask = match candidate.efficiency_mode {
                EfficiencyMode::Background => Some(BACKGROUND_POWER_THROTTLING_MASK),
                EfficiencyMode::Foreground => Some(0),
                EfficiencyMode::None => None,
            };
            let needs_efficiency_update = previous.memory_priority != target_memory_priority
                || previous.power_state_mask != target_power_state_mask;
            if needs_efficiency_update {
                match set_process_efficiency_by_pid(
                    candidate.pid,
                    target_memory_priority,
                    Some(BACKGROUND_POWER_THROTTLING_MASK),
                    target_power_state_mask,
                ) {
                    Ok(()) => {
                        efficiency_changed = true;
                        summary.efficiency_changed.push(ProcessEfficiencyChange {
                            pid: candidate.pid,
                            process_name: candidate.process_name.clone(),
                            previous,
                            target_memory_priority,
                            target_power_state_mask,
                            role: candidate.role,
                        });
                    }
                    Err(error) => summary.failed.push(json!({
                        "pid": candidate.pid,
                        "process_name": candidate.process_name,
                        "role": candidate.role,
                        "surface": "process_efficiency_apply",
                        "error": error,
                    })),
                }
            }
        }

        if priority_changed {
            summary.changed.push(PriorityChange {
                pid: candidate.pid,
                process_name: candidate.process_name.clone(),
                previous_priority_class: previous_class,
                previous_priority_label: priority_class_label(previous_class).to_string(),
                target_priority_class: candidate.target_class,
                target_priority_label: candidate.target_label,
                role: candidate.role,
            });
        }

        if !priority_changed
            && !efficiency_changed
            && candidate.efficiency_mode != EfficiencyMode::None
        {
            summary.skipped_already_set += 1;
        }
    }

    if summary.changed.is_empty() && summary.efficiency_changed.is_empty() {
        return summary;
    }

    let mut entries = summary
        .changed
        .iter()
        .map(|change| SnapshotEntry::ProcessPriority {
            pid: change.pid,
            process_name: change.process_name.clone(),
            previous_priority_class: change.previous_priority_class,
            previous_priority_label: change.previous_priority_label.clone(),
            target_priority_class: change.target_priority_class,
            target_priority_label: change.target_priority_label.clone(),
        })
        .collect::<Vec<_>>();
    entries.extend(summary.efficiency_changed.iter().map(|change| {
        SnapshotEntry::ProcessEfficiency {
            pid: change.pid,
            process_name: change.process_name.clone(),
            previous_memory_priority: change.previous.memory_priority,
            previous_power_control_mask: change.previous.power_control_mask,
            previous_power_state_mask: change.previous.power_state_mask,
            target_memory_priority: change.target_memory_priority,
            target_power_state_mask: change.target_power_state_mask,
        }
    }));

    let snapshot = OptimizationSnapshot::new(
        action_name,
        entries,
        json!({
            "changed_processes": summary.changed,
            "efficiency_changes": summary.efficiency_changed,
        }),
    );

    match snapshot::save_snapshot(&snapshot) {
        Ok(()) => {
            summary.snapshot_id = Some(snapshot.id);
        }
        Err(error) => {
            for change in summary.changed.iter().rev() {
                let _ = set_priority_class_by_pid(change.pid, change.previous_priority_class);
            }
            for change in summary.efficiency_changed.iter().rev() {
                let _ = restore_process_efficiency_by_pid(
                    change.pid,
                    change.previous.memory_priority,
                    change.previous.power_control_mask,
                    change.previous.power_state_mask,
                );
            }
            summary.failed.push(json!({
                "snapshot_error": error,
                "rollback_attempted": true,
            }));
            summary.changed.clear();
            summary.efficiency_changed.clear();
        }
    }

    summary
}

fn priority_result(
    action_name: &str,
    summary: PriorityApplySummary,
    success_message: &str,
    empty_message: &str,
) -> ExecutionResult {
    let changed_count = summary.changed.len();
    let efficiency_changed_count = summary.efficiency_changed.len();
    let total_changed_count = changed_count + efficiency_changed_count;
    let already_set_count = summary.skipped_already_set;
    let success = total_changed_count > 0 || (summary.failed.is_empty() && already_set_count > 0);
    let message = if total_changed_count > 0 {
        success_message.to_string()
    } else if summary.failed.is_empty() {
        empty_message.to_string()
    } else {
        "Nao foi possivel ajustar prioridade dos processos elegiveis.".to_string()
    };

    ExecutionResult {
        success,
        message,
        details: json!({
            "implemented": true,
            "action_name": action_name,
            "changed_processes": summary.changed,
            "efficiency_changes": summary.efficiency_changed,
            "changed_count": total_changed_count,
            "priority_changed_count": changed_count,
            "efficiency_changed_count": efficiency_changed_count,
            "skipped_already_set": already_set_count,
            "skipped_protected": summary.skipped_protected,
            "failed": summary.failed,
            "snapshot": summary.snapshot_id.map(|id| json!({
                "id": id,
                "entries": total_changed_count,
                "reversible": true,
            })),
        }),
    }
}

fn requested_priority_class(payload: &Value) -> Option<u32> {
    [
        "priority",
        "priority_class",
        "priorityClass",
        "class",
        "target_priority",
        "targetPriority",
    ]
    .iter()
    .find_map(|key| payload.get(*key).and_then(Value::as_str))
    .and_then(priority_class_from_name)
}

fn priority_class_from_name(value: &str) -> Option<u32> {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "idle" | "low" | "baixa" => Some(IDLE_PRIORITY_CLASS_RAW),
        "below_normal" | "below normal" | "abaixo_do_normal" | "abaixo normal" => {
            Some(BELOW_NORMAL_PRIORITY_CLASS_RAW)
        }
        "normal" => Some(NORMAL_PRIORITY_CLASS_RAW),
        "above_normal" | "above normal" | "acima_do_normal" | "acima normal" => {
            Some(ABOVE_NORMAL_PRIORITY_CLASS_RAW)
        }
        "high" | "alta" | "alto" => Some(HIGH_PRIORITY_CLASS_RAW),
        "realtime" | "real_time" | "tempo_real" => Some(REALTIME_PRIORITY_CLASS_RAW),
        _ => None,
    }
}

fn priority_class_label(priority_class: u32) -> &'static str {
    match priority_class {
        IDLE_PRIORITY_CLASS_RAW => "idle",
        BELOW_NORMAL_PRIORITY_CLASS_RAW => "below_normal",
        NORMAL_PRIORITY_CLASS_RAW => "normal",
        ABOVE_NORMAL_PRIORITY_CLASS_RAW => "above_normal",
        HIGH_PRIORITY_CLASS_RAW => "high",
        REALTIME_PRIORITY_CLASS_RAW => "realtime",
        _ => "unknown",
    }
}

fn extract_target_name(payload: &Value) -> Option<String> {
    [
        "target",
        "process",
        "process_name",
        "processName",
        "exe",
        "name",
    ]
    .iter()
    .find_map(|key| payload.get(*key).and_then(Value::as_str))
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn payload_bool(payload: Option<&Value>, key: &str, default: bool) -> bool {
    payload
        .and_then(|payload| payload.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn normalize_process_name(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

fn parse_affinity_mask(value: &Value) -> Option<usize> {
    if let Some(number) = value.as_u64() {
        return usize::try_from(number).ok().filter(|mask| *mask != 0);
    }
    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    let parsed = if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16).ok()
    } else {
        raw.parse::<usize>().ok()
    };
    parsed.filter(|mask| *mask != 0)
}

fn apply_full_game_affinity_with_snapshot(
    pid: u32,
    process_name: &str,
    requested_mask: Option<usize>,
) -> ExecutionResult {
    let (previous_process_mask, previous_system_mask) = match process_affinity_by_pid(pid) {
        Ok(mask) => mask,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel consultar afinidade do processo de jogo.".to_string(),
                details: json!({
                    "implemented": true,
                    "pid": pid,
                    "processName": process_name,
                    "error": error,
                }),
            }
        }
    };

    let target_process_mask = requested_mask
        .map(|mask| mask & previous_system_mask)
        .filter(|mask| *mask != 0)
        .unwrap_or(previous_system_mask);
    if previous_process_mask == target_process_mask {
        return ExecutionResult::ok(
            "Afinidade do processo de jogo ja usa o conjunto esperado de CPUs.",
            json!({
                "implemented": true,
                "changed": false,
                "pid": pid,
                "processName": process_name,
                "previousProcessMask": previous_process_mask,
                "systemMask": previous_system_mask,
                "targetProcessMask": target_process_mask,
                "strategy": "ensure_full_system_affinity",
            }),
        );
    }

    let snapshot = OptimizationSnapshot::new(
        "APPLY_GAME_AFFINITY",
        vec![SnapshotEntry::ProcessAffinity {
            pid,
            process_name: process_name.to_string(),
            previous_process_mask,
            previous_system_mask,
            target_process_mask,
            strategy: "ensure_full_system_affinity".to_string(),
        }],
        json!({
            "pid": pid,
            "processName": process_name,
            "previousProcessMask": previous_process_mask,
            "systemMask": previous_system_mask,
            "targetProcessMask": target_process_mask,
            "strategy": "ensure_full_system_affinity",
        }),
    );

    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        return ExecutionResult {
            success: false,
            message: "A alteracao de afinidade foi bloqueada porque o snapshot falhou.".to_string(),
            details: json!({
                "implemented": true,
                "pid": pid,
                "processName": process_name,
                "snapshot_error": error,
            }),
        };
    }

    if let Err(error) = set_process_affinity_by_pid(pid, target_process_mask) {
        let _ = snapshot::discard_snapshot(&snapshot.id);
        return ExecutionResult {
            success: false,
            message: "Nao foi possivel ajustar afinidade do processo de jogo.".to_string(),
            details: json!({
                "implemented": true,
                "pid": pid,
                "processName": process_name,
                "snapshot_discarded": true,
                "error": error,
            }),
        };
    }

    let change = ProcessAffinityChange {
        pid,
        process_name: process_name.to_string(),
        previous_process_mask,
        previous_system_mask,
        target_process_mask,
        strategy: "ensure_full_system_affinity",
    };

    ExecutionResult::ok(
        "Afinidade do processo de jogo ajustada com snapshot reversivel.",
        json!({
            "implemented": true,
            "changed": true,
            "affinityChange": change,
            "snapshot": {
                "id": snapshot.id,
                "entries": snapshot.entries.len(),
                "reversible": true,
            },
        }),
    )
}

#[cfg(windows)]
fn priority_class_by_pid(pid: u32) -> Result<u32, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetPriorityClass, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(|error| error.to_string())?;
    let priority_class = unsafe { GetPriorityClass(handle) };
    let _ = unsafe { CloseHandle(handle) };

    if priority_class == 0 {
        Err(format!(
            "GetPriorityClass falhou para pid {pid}: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(priority_class)
    }
}

#[cfg(not(windows))]
fn priority_class_by_pid(_pid: u32) -> Result<u32, String> {
    Err("Prioridade de processos esta disponivel apenas no Windows.".to_string())
}

#[cfg(windows)]
fn set_priority_class_by_pid(pid: u32, priority_class: u32) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, SetPriorityClass, PROCESS_CREATION_FLAGS, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(|error| error.to_string())?;
    let result = unsafe { SetPriorityClass(handle, PROCESS_CREATION_FLAGS(priority_class)) };
    let _ = unsafe { CloseHandle(handle) };
    result.map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn set_priority_class_by_pid(_pid: u32, _priority_class: u32) -> Result<(), String> {
    Err("Prioridade de processos esta disponivel apenas no Windows.".to_string())
}

#[cfg(windows)]
fn process_affinity_by_pid(pid: u32) -> Result<(usize, usize), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetProcessAffinityMask, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(|error| error.to_string())?;
    let mut process_mask = 0usize;
    let mut system_mask = 0usize;
    let result = unsafe { GetProcessAffinityMask(handle, &mut process_mask, &mut system_mask) };
    let _ = unsafe { CloseHandle(handle) };
    result
        .map(|_| (process_mask, system_mask))
        .map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn process_affinity_by_pid(_pid: u32) -> Result<(usize, usize), String> {
    Err("Afinidade de processos esta disponivel apenas no Windows.".to_string())
}

#[cfg(windows)]
fn set_process_affinity_by_pid(pid: u32, process_mask: usize) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, SetProcessAffinityMask, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION,
    };

    if process_mask == 0 {
        return Err("Mascara de afinidade invalida.".to_string());
    }

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(|error| error.to_string())?;
    let result = unsafe { SetProcessAffinityMask(handle, process_mask) };
    let _ = unsafe { CloseHandle(handle) };
    result.map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn set_process_affinity_by_pid(_pid: u32, _process_mask: usize) -> Result<(), String> {
    Err("Afinidade de processos esta disponivel apenas no Windows.".to_string())
}

pub fn restore_process_efficiency_by_pid(
    pid: u32,
    memory_priority: Option<u32>,
    power_control_mask: Option<u32>,
    power_state_mask: Option<u32>,
) -> Result<(), String> {
    set_process_efficiency_by_pid(pid, memory_priority, power_control_mask, power_state_mask)
}

#[cfg(windows)]
fn process_efficiency_state_by_pid(pid: u32) -> Result<ProcessEfficiencyState, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetProcessInformation, OpenProcess, ProcessMemoryPriority, ProcessPowerThrottling,
        MEMORY_PRIORITY_INFORMATION, PROCESS_POWER_THROTTLING_STATE,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(|error| error.to_string())?;

    let mut memory = MEMORY_PRIORITY_INFORMATION::default();
    let memory_priority = unsafe {
        GetProcessInformation(
            handle,
            ProcessMemoryPriority,
            &mut memory as *mut _ as *mut _,
            std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    }
    .map(|_| memory.MemoryPriority.0)
    .ok();

    let mut power = PROCESS_POWER_THROTTLING_STATE::default();
    let power_state = unsafe {
        GetProcessInformation(
            handle,
            ProcessPowerThrottling,
            &mut power as *mut _ as *mut _,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .map(|_| (power.ControlMask, power.StateMask))
    .ok();

    let _ = unsafe { CloseHandle(handle) };

    Ok(ProcessEfficiencyState {
        memory_priority,
        power_control_mask: power_state.map(|state| state.0),
        power_state_mask: power_state.map(|state| state.1),
    })
}

#[cfg(not(windows))]
fn process_efficiency_state_by_pid(_pid: u32) -> Result<ProcessEfficiencyState, String> {
    Err("Eficiencia de processos esta disponivel apenas no Windows.".to_string())
}

#[cfg(windows)]
fn set_process_efficiency_by_pid(
    pid: u32,
    memory_priority: Option<u32>,
    power_control_mask: Option<u32>,
    power_state_mask: Option<u32>,
) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, ProcessMemoryPriority, ProcessPowerThrottling, SetProcessInformation,
        MEMORY_PRIORITY, MEMORY_PRIORITY_INFORMATION, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(|error| error.to_string())?;

    if let Some(priority) = memory_priority {
        let memory = MEMORY_PRIORITY_INFORMATION {
            MemoryPriority: MEMORY_PRIORITY(priority),
        };
        unsafe {
            SetProcessInformation(
                handle,
                ProcessMemoryPriority,
                &memory as *const _ as *const _,
                std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
            )
        }
        .map_err(|error| {
            let _ = unsafe { CloseHandle(handle) };
            error.to_string()
        })?;
    }

    if let Some(state_mask) = power_state_mask {
        let power = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: power_control_mask.unwrap_or(BACKGROUND_POWER_THROTTLING_MASK),
            StateMask: state_mask,
        };
        unsafe {
            SetProcessInformation(
                handle,
                ProcessPowerThrottling,
                &power as *const _ as *const _,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        }
        .map_err(|error| {
            let _ = unsafe { CloseHandle(handle) };
            error.to_string()
        })?;
    }

    let _ = unsafe { CloseHandle(handle) };
    Ok(())
}

#[cfg(not(windows))]
fn set_process_efficiency_by_pid(
    _pid: u32,
    _memory_priority: Option<u32>,
    _power_control_mask: Option<u32>,
    _power_state_mask: Option<u32>,
) -> Result<(), String> {
    Err("Eficiencia de processos esta disponivel apenas no Windows.".to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        parse_affinity_mask, priority_class_from_name, BELOW_NORMAL_PRIORITY_CLASS_RAW,
        HIGH_PRIORITY_CLASS_RAW,
    };

    #[test]
    fn parses_priority_aliases() {
        assert_eq!(
            priority_class_from_name("below-normal"),
            Some(BELOW_NORMAL_PRIORITY_CLASS_RAW)
        );
        assert_eq!(
            priority_class_from_name("alta"),
            Some(HIGH_PRIORITY_CLASS_RAW)
        );
    }

    #[test]
    fn parses_affinity_masks_safely() {
        assert_eq!(parse_affinity_mask(&json!("0x0f")), Some(15));
        assert_eq!(parse_affinity_mask(&json!(3)), Some(3));
        assert_eq!(parse_affinity_mask(&json!("0")), None);
        assert_eq!(parse_affinity_mask(&json!("not-a-mask")), None);
    }
}
