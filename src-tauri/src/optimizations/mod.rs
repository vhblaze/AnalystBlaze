pub mod cleanup;
pub mod detection;
pub mod energy;
pub mod focus;
pub mod latency;
pub mod local_ai_policy;
pub mod memory;
pub mod privileged_helper;
pub mod processes;
pub mod protected_apps;
pub mod safety;
pub mod snapshot;
pub mod windows_actions;
pub mod windows_inventory;

use serde::Serialize;
use serde_json::{json, Value};
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
    let safety_context = SafetyContext {
        source,
        allowed_actions,
        local_confirmation,
        privileged_helper_available: false,
    };

    if let Err(error) = validate_command(action_name, payload.as_ref(), &safety_context) {
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
        "APPLY_GAME_MODE" => apply_game_mode(payload).await,
        "SET_PROCESS_PRIORITY" => processes::set_process_priority(payload).await,
        "EMPTY_TEMP" => cleanup::empty_temp(payload).await,
        "CLEAR_STANDBY_LIST" => memory::clear_standby_list(payload).await,
        "SET_POWER_PLAN_HIGH_PERFORMANCE" => energy::set_high_performance(payload).await,
        "SET_POWER_PLAN_BALANCED" => energy::set_balanced(payload).await,
        "SET_POWER_PLAN_POWER_SAVER" => energy::set_power_saver(payload).await,
        "APPLY_LATENCY_TWEAKS" => latency::apply_latency_tweaks(payload).await,
        "ENTER_FOCUS_MODE" => focus::enter_focus_mode(payload).await,
        "DETECT_FOREGROUND_GAME" => detection::detect_foreground_game(payload).await,
        "DISABLE_STARTUP_APP" => windows_actions::disable_startup_app(payload).await,
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
    let auto_restore = payload_bool(payload.as_ref(), "auto_restore", true);
    let detected_game = detection::detect_game_process();
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
    let foreground = detection::detect_foreground_game(payload).await;
    let snapshot_ids = collect_snapshot_ids([&power.details, &cleanup.details, &focus.details]);

    if auto_restore && detected_game.detected && !snapshot_ids.is_empty() {
        spawn_game_restore_monitor(
            detected_game.pid.clone(),
            detected_game.process_name.clone(),
            snapshot_ids.clone(),
        );
    }

    let success = power.success || cleanup.success || focus.success || foreground.success;
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
                "auto_restore": auto_restore,
            },
            "detected_game": detected_game,
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

fn spawn_game_restore_monitor(
    pid: Option<String>,
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

        for _ in 0..(12 * 60) {
            thread::sleep(Duration::from_secs(5));
            if detection::process_still_running(pid.as_deref(), process_name.as_deref()) {
                continue;
            }

            let report = snapshot::restore_snapshots_by_ids(&snapshot_ids);
            let _ = audit::record_event(
                "info",
                "game_mode.restored_after_game_exit",
                "Modo Gamer restaurado apos fechamento do jogo detectado.",
                serde_json::to_value(&report).unwrap_or_else(|_| Value::Null),
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
