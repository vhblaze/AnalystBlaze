pub mod adaptive;
pub mod cleanup;
pub mod detection;
pub mod disk_usage;
pub mod energy;
pub mod focus;
pub mod latency;
pub mod local_ai_policy;
pub mod memory;
pub mod network_admin;
pub mod os_version;
pub mod performance_suite;
pub mod privileged_helper;
pub mod processes;
pub mod protected_apps;
pub mod safety;
pub mod snapshot;
pub mod visual_effects;
pub mod windows_actions;
pub mod windows_inventory;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::thread;
use std::time::Duration;

use crate::audit;
use safety::{validate_command, CommandSource, SafetyContext};

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub message: String,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameModeSession {
    pub id: String,
    pub target_pid: Option<u32>,
    pub target_process_name: Option<String>,
    pub snapshot_ids: Vec<String>,
    pub created_at: i64,
    pub restored_at: Option<i64>,
    pub status: String,
    pub restore_reason: Option<String>,
}

impl ExecutionResult {
    fn ok(message: impl Into<String>, details: Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            details,
        }
    }

    fn unsupported(action_name: &str) -> Self {
        Self {
            success: false,
            message: format!("Comando ainda nao implementado no agente: {action_name}"),
            details: json!({ "action_name": action_name }),
        }
    }

    pub(crate) fn rejected(action_name: &str, reason: impl Into<String>, details: Value) -> Self {
        Self {
            success: false,
            message: format!(
                "Comando recusado pela camada de seguranca local: {}",
                reason.into()
            ),
            details: json!({
                "action_name": action_name,
                "blocked_by": "local_safety_gate",
                "details": details,
            }),
        }
    }
}

pub async fn execute_command(action_name: &str, payload: Option<Value>) -> ExecutionResult {
    execute_command_checked(CommandSource::ManualUser, action_name, payload, None, true).await
}

pub async fn execute_command_checked(
    source: CommandSource,
    action_name: &str,
    payload: Option<Value>,
    allowed_actions: Option<&[String]>,
    local_confirmation: bool,
) -> ExecutionResult {
    execute_command_checked_with_helper(
        source,
        action_name,
        payload,
        allowed_actions,
        local_confirmation,
        false,
    )
    .await
}

pub async fn execute_privileged_helper_command(
    source: CommandSource,
    action_name: &str,
    payload: Option<Value>,
) -> ExecutionResult {
    execute_command_checked_with_helper(source, action_name, payload, None, true, true).await
}

async fn execute_command_checked_with_helper(
    source: CommandSource,
    action_name: &str,
    payload: Option<Value>,
    allowed_actions: Option<&[String]>,
    local_confirmation: bool,
    privileged_helper_available: bool,
) -> ExecutionResult {
    let safety_context = SafetyContext {
        source,
        allowed_actions,
        local_confirmation,
        privileged_helper_available,
    };

    if let Err(error) = validate_command(action_name, payload.as_ref(), &safety_context) {
        if error.reason == "privileged_helper_unavailable" && local_confirmation {
            match privileged_helper::execute(
                action_name,
                payload.clone(),
                source,
                local_confirmation,
            ) {
                Ok(result) => return result,
                Err(helper_error) => {
                    return ExecutionResult::rejected(
                        action_name,
                        "privileged_helper_unavailable",
                        json!({
                            "helper_error": helper_error,
                            "safety": error.details,
                        }),
                    );
                }
            }
        }

        let _ = audit::record_event(
            "warn",
            "optimization.command_rejected",
            "Comando recusado pela camada local de seguranca.",
            json!({
                "action_name": action_name,
                "source": source,
                "reason": error.reason,
                "details": error.details,
            }),
        );
        return ExecutionResult::rejected(action_name, error.reason, error.details);
    }

    let result = match action_name {
        "APPLY_ADAPTIVE_OPTIMIZATION" => adaptive::apply_adaptive_optimization(payload).await,
        "APPLY_GAME_MODE" => apply_game_mode(payload).await,
        "APPLY_PC_CLEAN_FAST_BACKGROUND_PRIORITIES" => {
            processes::optimize_background_process_priorities(payload).await
        }
        "APPLY_BACKGROUND_QUIET_MODE" => latency::apply_background_quiet_mode(payload).await,
        "APPLY_FOREGROUND_BURST_MODE" => latency::apply_foreground_burst_mode(payload).await,
        "APPLY_UPLINK_PRESSURE_RELIEF_STAGE1" => {
            latency::apply_uplink_pressure_relief_stage1(payload).await
        }
        "APPLY_PC_CLEAN_FAST_PROFILE" => {
            let options = payload
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or(performance_suite::PcCleanFastOptions {
                    include_startup: true,
                    include_cleanup: true,
                    include_background: true,
                    include_network: false,
                    include_gaming: true,
                });
            performance_suite::apply_pc_clean_fast_profile(options).await
        }
        "APPLY_CLEANUP_CATEGORY" => {
            let category = payload
                .as_ref()
                .and_then(|value| {
                    value
                        .get("category")
                        .or_else(|| value.get("id"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("user_temp")
                .to_string();
            let mode = payload
                .as_ref()
                .and_then(|value| value.get("mode"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            performance_suite::apply_cleanup_category(category, mode).await
        }
        "SET_PROCESS_PRIORITY" => processes::set_process_priority(payload).await,
        "DELETE_DISK_USAGE_ITEM" => {
            let path = payload
                .as_ref()
                .and_then(|value| value.get("path"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            disk_usage::delete_item(path).await
        }
        "EMPTY_TEMP" => cleanup::empty_temp(payload).await,
        "PURGE_CLEANUP_QUARANTINE" => cleanup::purge_cleanup_quarantine(payload).await,
        "CLEAR_STANDBY_LIST" => memory::clear_standby_list(payload).await,
        "FLUSH_DNS_CACHE" => network_admin::flush_dns_cache(payload).await,
        "SET_DNS_SERVERS" => network_admin::set_dns_servers(payload).await,
        "RESET_WINSOCK_CATALOG" => network_admin::reset_winsock_catalog(payload).await,
        "APPLY_VISUAL_PERFORMANCE_MODE" => {
            visual_effects::apply_visual_performance_mode(payload).await
        }
        "RESTORE_VISUAL_EFFECTS" => visual_effects::restore_visual_effects(payload).await,
        "RESTORE_PERFORMANCE_SESSION" => {
            let session_id = payload
                .as_ref()
                .and_then(|value| {
                    value
                        .get("sessionId")
                        .or_else(|| value.get("session_id"))
                        .and_then(Value::as_str)
                })
                .map(ToString::to_string);
            performance_suite::restore_performance_session(session_id)
        }
        "SET_POWER_PLAN_HIGH_PERFORMANCE" => energy::set_high_performance(payload).await,
        "SET_POWER_PLAN_BALANCED" => energy::set_balanced(payload).await,
        "SET_POWER_PLAN_POWER_SAVER" => energy::set_power_saver(payload).await,
        "APPLY_LATENCY_TWEAKS" => latency::apply_latency_tweaks(payload).await,
        "RESTORE_LATENCY_SESSION" => ExecutionResult::ok(
            "Sessao de latencia restaurada por snapshots locais.",
            serde_json::to_value(latency::restore_latency_session(Some(
                "command_restore".to_string(),
            )))
            .unwrap_or(Value::Null),
        ),
        "ENTER_FOCUS_MODE" => focus::enter_focus_mode(payload).await,
        "RESTORE_FOCUS_SESSION" => ExecutionResult::ok(
            "Sessao de Modo Foco restaurada por snapshots locais.",
            serde_json::to_value(focus::restore_focus_session(Some(
                "command_restore".to_string(),
            )))
            .unwrap_or(Value::Null),
        ),
        "DETECT_FOREGROUND_GAME" => detection::detect_foreground_game(payload).await,
        "DISABLE_STARTUP_APP" => windows_actions::disable_startup_app(payload).await,
        "DELAY_STARTUP_APP" => {
            let name = payload
                .as_ref()
                .and_then(|value| {
                    value
                        .get("name")
                        .or_else(|| value.get("target"))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default()
                .to_string();
            let location = payload
                .as_ref()
                .and_then(|value| value.get("location"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let delay_seconds = payload.as_ref().and_then(|value| {
                value
                    .get("delaySeconds")
                    .or_else(|| value.get("delay_seconds"))
                    .and_then(Value::as_u64)
            });
            performance_suite::delay_startup_app(name, location, delay_seconds).await
        }
        "RESTORE_DELAYED_STARTUP_APP" => {
            let name = payload
                .as_ref()
                .and_then(|value| {
                    value
                        .get("name")
                        .or_else(|| value.get("target"))
                        .and_then(Value::as_str)
                })
                .map(ToString::to_string);
            performance_suite::restore_delayed_startup_app(name).await
        }
        "RESTORE_STARTUP_APP" => windows_actions::restore_startup_app(payload).await,
        "STOP_SERVICE" => windows_actions::stop_service(payload).await,
        "RESTORE_SERVICE" => windows_actions::restore_service(payload).await,
        other => ExecutionResult::unsupported(other),
    };

    let _ = audit::record_event(
        if result.success { "info" } else { "warn" },
        "optimization.command_executed",
        "Comando de otimizacao processado pelo agente local.",
        json!({
            "action_name": action_name,
            "source": source,
            "success": result.success,
            "message": result.message,
            "details": result.details,
        }),
    );

    result
}

async fn apply_game_mode(payload: Option<Value>) -> ExecutionResult {
    let optimize_power_plan = payload_bool(payload.as_ref(), "optimize_power_plan", true);
    let safe_temp_cleanup = payload_bool(payload.as_ref(), "safe_temp_cleanup", true);
    let enter_focus_mode = payload_bool(payload.as_ref(), "enter_focus_mode", true);
    let optimize_visual_effects = payload_bool(payload.as_ref(), "optimize_visual_effects", true);
    let optimize_process_priorities =
        payload_bool(payload.as_ref(), "optimize_process_priorities", true);
    let auto_restore = payload_bool(payload.as_ref(), "auto_restore", true);
    let detected_game = detection::detect_game_process_with_payload(payload.as_ref());
    let target_pid = detected_game
        .pid
        .as_deref()
        .and_then(|pid| pid.parse::<u32>().ok());
    let before = json!({
        "powerPlan": current_power_plan_value(),
        "targetPid": target_pid,
        "targetProcess": detected_game.process_name.clone(),
        "targetPriority": target_pid.map(processes::process_priority_report),
        "visualEffects": visual_effects::current_visual_effects_summary(),
    });
    let power = if optimize_power_plan {
        energy::set_high_performance(payload.clone()).await
    } else {
        ExecutionResult::ok(
            "Plano de energia ignorado pela policy local.",
            json!({ "implemented": true, "skipped_by_policy": true }),
        )
    };
    let cleanup = if safe_temp_cleanup {
        cleanup::empty_temp(payload.clone()).await
    } else {
        ExecutionResult::ok(
            "Limpeza TEMP ignorada pela policy local.",
            json!({ "implemented": true, "skipped_by_policy": true }),
        )
    };
    let focus = if enter_focus_mode {
        focus::enter_focus_mode(payload.clone()).await
    } else {
        ExecutionResult::ok(
            "Modo foco ignorado pela policy local.",
            json!({ "implemented": true, "skipped_by_policy": true }),
        )
    };
    let visual_effects_result = if optimize_visual_effects {
        visual_effects::apply_visual_performance_mode(payload.clone()).await
    } else {
        ExecutionResult::ok(
            "Efeitos visuais ignorados pela policy local.",
            json!({ "implemented": true, "skipped_by_policy": true }),
        )
    };
    let process_priorities = if optimize_process_priorities {
        processes::optimize_game_process_priorities(payload.clone(), &detected_game).await
    } else {
        ExecutionResult::ok(
            "Prioridades de processos ignoradas pela policy local.",
            json!({ "implemented": true, "skipped_by_policy": true }),
        )
    };
    let foreground = detection::detect_foreground_game(payload).await;
    let snapshot_ids = collect_snapshot_ids([
        &power.details,
        &cleanup.details,
        &focus.details,
        &visual_effects_result.details,
        &process_priorities.details,
    ]);
    let after = json!({
        "powerPlan": current_power_plan_value(),
        "targetPid": target_pid,
        "targetProcess": detected_game.process_name.clone(),
        "targetPriority": target_pid.map(processes::process_priority_report),
        "visualEffects": visual_effects::current_visual_effects_summary(),
        "changedProcesses": process_priorities
            .details
            .get("changed_processes")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    });

    let restore_session = if auto_restore && detected_game.detected && !snapshot_ids.is_empty() {
        save_active_game_mode_session(
            target_pid,
            detected_game.process_name.clone(),
            snapshot_ids.clone(),
        )
        .ok()
    } else {
        None
    };

    if let Some(session) = restore_session.as_ref() {
        spawn_game_restore_monitor(
            session.id.clone(),
            session.target_pid,
            session.target_process_name.clone(),
            snapshot_ids.clone(),
        );
    }

    let success = power.success
        || cleanup.success
        || focus.success
        || visual_effects_result.success
        || process_priorities.success
        || foreground.success;
    ExecutionResult {
        success,
        message: if success {
            "Modo gamer aplicado com otimizacoes seguras locais.".to_string()
        } else {
            "Modo gamer nao conseguiu aplicar otimizacoes locais.".to_string()
        },
        details: json!({
            "profile": "game_mode",
            "manual": true,
            "policy": {
                "optimize_power_plan": optimize_power_plan,
                "safe_temp_cleanup": safe_temp_cleanup,
                "enter_focus_mode": enter_focus_mode,
                "optimize_visual_effects": optimize_visual_effects,
                "optimize_process_priorities": optimize_process_priorities,
                "auto_restore": auto_restore,
            },
            "detected_game": detected_game,
            "verification": {
                "before": before,
                "after": after,
            },
            "changedProcesses": process_priorities
                .details
                .get("changed_processes")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
            "restoreSession": restore_session,
            "restoreStatus": if auto_restore && !snapshot_ids.is_empty() { "monitoring" } else { "not_started" },
            "restore_monitor": {
                "enabled": auto_restore && !snapshot_ids.is_empty(),
                "snapshot_ids": snapshot_ids,
            },
            "steps": {
                "power": {
                    "success": power.success,
                    "message": power.message,
                    "details": power.details,
                },
                "cleanup": {
                    "success": cleanup.success,
                    "message": cleanup.message,
                    "details": cleanup.details,
                },
                "focus": {
                    "success": focus.success,
                    "message": focus.message,
                    "details": focus.details,
                },
                "visual_effects": {
                    "success": visual_effects_result.success,
                    "message": visual_effects_result.message,
                    "details": visual_effects_result.details,
                },
                "process_priorities": {
                    "success": process_priorities.success,
                    "message": process_priorities.message,
                    "details": process_priorities.details,
                },
                "foreground": {
                    "success": foreground.success,
                    "message": foreground.message,
                    "details": foreground.details,
                },
            },
            "pro_agent_note": "Planos Pro poderao aplicar ajustes adaptativos automaticamente por orquestracao.",
        }),
    }
}

fn payload_bool(payload: Option<&Value>, key: &str, default: bool) -> bool {
    payload
        .and_then(|payload| payload.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn collect_snapshot_ids<'a>(details: impl IntoIterator<Item = &'a Value>) -> Vec<String> {
    details
        .into_iter()
        .filter_map(|details| {
            details
                .pointer("/snapshot/id")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
        .collect()
}

pub fn restore_active_game_mode_session() -> snapshot::RestoreReport {
    let Some(mut session) = read_active_game_mode_session() else {
        return snapshot::RestoreReport {
            restored_snapshots: 0,
            failed_snapshots: 0,
            restored_entries: 0,
            failed_entries: 0,
            skipped_conflicts: 0,
            messages: vec!["Nenhuma sessao ativa de Modo Gamer encontrada.".to_string()],
        };
    };

    let report = snapshot::restore_snapshots_by_ids(&session.snapshot_ids);
    session.status = "restored".to_string();
    session.restored_at = Some(chrono::Utc::now().timestamp());
    session.restore_reason = Some("manual_restore".to_string());
    let _ = write_active_game_mode_session(&session);
    let _ = audit::record_event(
        "info",
        "game_mode.restored_manually",
        "Modo Gamer restaurado manualmente.",
        serde_json::to_value(&report).unwrap_or(Value::Null),
    );
    report
}

pub fn active_game_mode_session() -> Option<GameModeSession> {
    read_active_game_mode_session().filter(|session| {
        session.restored_at.is_none() && !session.status.eq_ignore_ascii_case("restored")
    })
}

fn current_power_plan_value() -> Value {
    match snapshot::active_power_plan() {
        Ok(plan) => json!({
            "schemeGuid": plan.scheme_guid,
            "schemeName": plan.scheme_name,
        }),
        Err(error) => json!({ "error": error }),
    }
}

fn save_active_game_mode_session(
    target_pid: Option<u32>,
    process_name: Option<String>,
    snapshot_ids: Vec<String>,
) -> Result<GameModeSession, String> {
    let session = GameModeSession {
        id: uuid::Uuid::new_v4().simple().to_string(),
        target_pid,
        target_process_name: process_name,
        snapshot_ids,
        created_at: chrono::Utc::now().timestamp(),
        restored_at: None,
        status: "monitoring".to_string(),
        restore_reason: None,
    };
    write_active_game_mode_session(&session)?;
    Ok(session)
}

fn read_active_game_mode_session() -> Option<GameModeSession> {
    let raw = fs::read_to_string(game_mode_session_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_active_game_mode_session(session: &GameModeSession) -> Result<(), String> {
    let path = game_mode_session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn mark_game_mode_session_restored(session_id: &str, reason: &str) {
    let Some(mut session) = read_active_game_mode_session() else {
        return;
    };
    if session.id != session_id || session.restored_at.is_some() {
        return;
    }
    session.status = "restored".to_string();
    session.restored_at = Some(chrono::Utc::now().timestamp());
    session.restore_reason = Some(reason.to_string());
    let _ = write_active_game_mode_session(&session);
}

fn game_mode_session_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join("game-mode-session.json")
}

fn spawn_game_restore_monitor(
    session_id: String,
    pid: Option<u32>,
    process_name: Option<String>,
    snapshot_ids: Vec<String>,
) {
    thread::spawn(move || {
        let _ = audit::record_event(
            "info",
            "game_mode.monitor_started",
            "Monitor de Modo Gamer iniciado para restaurar snapshots ao fechar o jogo.",
            json!({
                "pid": pid,
                "process_name": process_name,
                "snapshot_ids": snapshot_ids,
            }),
        );

        let mut missing_cycles = 0_u8;
        for _ in 0..(12 * 60) {
            thread::sleep(Duration::from_secs(5));
            if detection::process_still_running(
                pid.map(|pid| pid.to_string()).as_deref(),
                process_name.as_deref(),
            ) {
                missing_cycles = 0;
                continue;
            }

            missing_cycles = missing_cycles.saturating_add(1);
            if missing_cycles < 2 {
                continue;
            }

            let report = snapshot::restore_snapshots_by_ids(&snapshot_ids);
            mark_game_mode_session_restored(&session_id, "target_process_exit");
            let _ = audit::record_event(
                "info",
                "game_mode.restored_after_game_exit",
                "Modo Gamer restaurado apos fechamento do jogo detectado.",
                serde_json::to_value(&report).unwrap_or(Value::Null),
            );
            return;
        }

        let _ = audit::record_event(
            "warn",
            "game_mode.monitor_timeout",
            "Monitor de Modo Gamer expirou sem detectar fechamento do jogo.",
            json!({
                "pid": pid,
                "process_name": process_name,
                "snapshot_ids": snapshot_ids,
            }),
        );
    });
}
