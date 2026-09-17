pub mod active_use;
pub mod adaptive;
pub mod app_usage;
pub mod autostart;
pub mod cleanup;
pub mod detection;
pub mod disk_tree;
pub mod disk_usage;
pub mod energy;
pub mod focus;
pub mod frame_capture_control;
pub mod game_launch;
pub mod latency;
pub mod local_ai_policy;
pub mod memory;
pub mod network_admin;
pub mod network_tune;
pub mod os_version;
pub mod performance_suite;
pub mod privileged_helper;
pub mod processes;
pub mod protected_apps;
pub mod safety;
pub mod service_usage;
pub mod shadow_storage;
pub mod snapshot;
pub mod storage_media;
pub mod system_repair;
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
    /// Set once a PresentMon ground-truth frame capture has actually
    /// started for this session (see START_FRAME_CAPTURE below) - `None`
    /// for sessions with no capture attempt, a capture still starting, or a
    /// capture that failed to start. `#[serde(default)]` so a session file
    /// written by an older build (before this field existed) still
    /// deserializes.
    #[serde(default)]
    pub frame_capture_id: Option<String>,
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
                    let _ = audit::record_event(
                        "warn",
                        "optimization.helper.execute_failed",
                        "Chamada ao helper privilegiado falhou do lado do cliente.",
                        json!({
                            "action_name": action_name,
                            "source": source,
                            "helper_error": helper_error,
                        }),
                    );
                    // A dropped pipe connection (os error 233, "no process on
                    // the other end") almost always means the helper service
                    // was mid-restart/reinstall right when this request
                    // landed - a real but transient window, not a security
                    // rejection. Word it as such instead of the generic
                    // "recusado pela camada de seguranca" message, which
                    // reads as a hard block when retrying shortly after
                    // usually just works.
                    if helper_error.contains("233") || helper_error.contains("other end of the pipe")
                    {
                        return ExecutionResult {
                            success: false,
                            message: "O helper privilegiado estava reiniciando quando este comando chegou. Tente novamente em alguns segundos.".to_string(),
                            details: json!({
                                "action_name": action_name,
                                "blocked_by": "helper_transiently_unavailable",
                                "helper_error": helper_error,
                            }),
                        };
                    }
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
        "APPLY_GAME_MODE" => apply_game_mode(payload, source).await,
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
        "RENEW_DHCP_LEASE" => network_tune::renew_dhcp_lease(payload).await,
        "APPLY_NETWORK_TUNE" => network_tune::apply_network_tune(payload).await,
        "REVERT_NETWORK_TUNE" => network_tune::revert_network_tune(payload).await,
        "RESTART_WINDOWS_NOW" => network_tune::restart_windows(payload).await,
        "SET_DNS_SERVERS" => network_admin::set_dns_servers(payload).await,
        "SET_INTERFACE_METRIC" => network_admin::set_interface_metric(payload).await,
        "SET_ADAPTER_ENABLED" => network_admin::set_adapter_enabled(payload).await,
        "CHECK_ADAPTER_DISABLE_GUARD" => network_admin::check_adapter_disable_guard(payload).await,
        "RESET_WINSOCK_CATALOG" => network_admin::reset_winsock_catalog(payload).await,
        "ENABLE_SCHEDULED_DEFRAG" => storage_media::enable_scheduled_defrag(payload).await,
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
        "RESIZE_SHADOW_STORAGE" => shadow_storage::resize_shadow_storage(payload).await,
        "START_FRAME_CAPTURE" => frame_capture_control::start_frame_capture(payload).await,
        "STOP_FRAME_CAPTURE" => frame_capture_control::stop_frame_capture(payload).await,
        "START_SYSTEM_FILE_CHECK" => system_repair::start_system_file_check(payload).await,
        "SYSTEM_FILE_CHECK_STATUS" => system_repair::system_file_check_status(payload).await,
        "START_DISM_RESTORE_HEALTH" => system_repair::start_dism_restore_health(payload).await,
        "DISM_RESTORE_HEALTH_STATUS" => system_repair::system_file_check_status(payload).await,
        "RESTART_PNP_DEVICE" => windows_actions::restart_pnp_device(payload).await,
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

async fn apply_game_mode(payload: Option<Value>, source: CommandSource) -> ExecutionResult {
    let optimize_power_plan = payload_bool(payload.as_ref(), "optimize_power_plan", true);
    let enter_focus_mode = payload_bool(payload.as_ref(), "enter_focus_mode", true);
    let optimize_visual_effects = payload_bool(payload.as_ref(), "optimize_visual_effects", true);
    let optimize_process_priorities =
        payload_bool(payload.as_ref(), "optimize_process_priorities", true);
    // TEMP cleanup does its own scan/delete pass, competing for disk I/O
    // with whatever the newly-launched app is itself doing on startup -
    // harmless competition on an SSD, but on a spinning HDD (real seek
    // latency, one physical head) that competition is what turned a real
    // incident (2026-09, see storage_media.rs's docs) into a full freeze.
    // Skipped outright on HDD rather than reordered/delayed - simplest fix
    // for a machine already the most starved for I/O headroom, and its
    // benefit (freeing some disk space) isn't tied to gaming performance
    // anyway, so there's nothing lost by just doing it another time.
    let on_hdd = storage_media::system_drive_is_hdd();
    let safe_temp_cleanup = payload_bool(payload.as_ref(), "safe_temp_cleanup", true) && !on_hdd;
    let stop_nonessential_services =
        payload_bool(payload.as_ref(), "stop_nonessential_services", true);
    let close_nonessential_apps =
        payload_bool(payload.as_ref(), "close_nonessential_apps", true);
    let auto_restore = payload_bool(payload.as_ref(), "auto_restore", true);
    // A live "Ativar Modo Gamer" click (or a server RemoteCommand, which is
    // itself already vetted server-side) is a supervised decision - unlike
    // the unsupervised LocalPolicy auto-trigger this same detection feeds
    // in evaluate_local_policy, it's safe to let it target a heavy-workload
    // tool like Blender (see detection.rs's is_heavy_workload_tool docs).
    let detected_game = detection::detect_game_process_with_payload(
        payload.as_ref(),
        source != CommandSource::LocalPolicy,
    );
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
    } else if on_hdd {
        ExecutionResult::ok(
            "Limpeza TEMP ignorada - disco mecanico (HD) detectado, evitando disputa por I/O no inicio do jogo.",
            json!({ "implemented": true, "skipped_reason": "hdd_detected" }),
        )
    } else {
        ExecutionResult::ok(
            "Limpeza TEMP ignorada pela policy local.",
            json!({ "implemented": true, "skipped_by_policy": true }),
        )
    };
    let focus = if enter_focus_mode {
        focus::enter_focus_mode(Some(focus_payload_for_game_mode(payload.as_ref()))).await
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
    let (services, services_snapshot_ids) = if stop_nonessential_services {
        // On HDD, SysMain is spared (its caching genuinely helps there) but
        // WSearch/DiagTrack are still paused - they're pure background
        // overhead with no upside either way, so there's no reason to give
        // up that part of the benefit just because SysMain has to stay.
        windows_actions::stop_nonessential_services_for_game_mode(on_hdd).await
    } else {
        (
            ExecutionResult::ok(
                "Pausa de servicos ignorada pela policy local.",
                json!({ "implemented": true, "skipped_by_policy": true }),
            ),
            Vec::new(),
        )
    };
    let apps = if close_nonessential_apps {
        windows_actions::close_nonessential_apps_for_game_mode().await
    } else {
        ExecutionResult::ok(
            "Fechamento de apps ignorado pela policy local.",
            json!({ "implemented": true, "skipped_by_policy": true }),
        )
    };
    // Only when the target is a heavy-workload tool (Blender and the like),
    // not for ordinary gaming - real GPU memory headroom matters far more
    // on a small VRAM budget doing sustained rendering than during a
    // typical game session, and this wasn't reviewed/asked for as a
    // general gaming behavior change (see windows_actions.rs's
    // GAME_MODE_GPU_MEMORY_CLOSABLE_APPS docs).
    let is_heavy_workload = detection::is_heavy_workload_target(&detected_game);
    let gpu_memory_apps = if close_nonessential_apps && is_heavy_workload {
        windows_actions::close_gpu_memory_heavy_apps_for_game_mode().await
    } else {
        ExecutionResult::ok(
            "Fechamento de apps de RGB/iluminacao nao se aplica (alvo nao e uma ferramenta de carga pesada).",
            json!({ "implemented": true, "skipped_reason": "not_a_heavy_workload_target" }),
        )
    };
    let foreground = detection::detect_foreground_game(payload).await;
    let mut snapshot_ids = collect_snapshot_ids([
        &power.details,
        &cleanup.details,
        &focus.details,
        &visual_effects_result.details,
        &process_priorities.details,
    ]);
    snapshot_ids.extend(services_snapshot_ids);
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

    // A session is saved whenever there's something to restore, even without
    // a detected game process - otherwise a manual activation (no game
    // running) never becomes "active" and the UI toggle can't reflect or
    // reverse it. Only spawn the exit-watching monitor when there's an
    // actual process to watch, though: process_still_running(None, None)
    // returns false immediately, which would auto-restore the session
    // within seconds if the monitor ran with nothing to track.
    let restore_session = if auto_restore && !snapshot_ids.is_empty() {
        save_active_game_mode_session(
            target_pid,
            detected_game.process_name.clone(),
            snapshot_ids.clone(),
        )
        .ok()
    } else {
        None
    };

    // Ground-truth frame capture now starts for every activation source,
    // LocalPolicy (the unsupervised local fallback heuristic) included -
    // this used to be the one excluded source, since privileged_helper.rs's
    // validate_request_execution_policy hard-rejects LocalPolicy for any
    // OTHER helper-required action (an unsupervised heuristic must never
    // reach the elevated helper on its own for anything that changes system
    // state - see the Blender/heavy-workload incident this codebase already
    // guards against elsewhere). But LocalPolicy is also how most real Modo
    // Gamer activations happen day-to-day (a live user rarely clicks "Ativar
    // Modo Gamer" themselves once auto-detection is on), so excluding it
    // here meant almost no real capture data ever got collected - the
    // opposite of a security tradeoff, just wasted training signal for no
    // safety benefit. The reason it's safe to open specifically for this
    // action pair: spawn_frame_capture_start below always talks to the
    // helper as CommandSource::RemoteCommand with local_confirmation forced
    // true, regardless of what source activated Game Mode - so this was
    // never actually a hole in the helper's own LocalPolicy defense, only
    // an extra gate this function added on top of it. START_FRAME_CAPTURE/
    // STOP_FRAME_CAPTURE stay the only two actions with that carve-out: pure
    // ETW reads, nothing destructive, and self-terminating via
    // MAX_CAPTURE_SECONDS even if nothing ever calls STOP.
    if let (Some(pid), Some(session)) = (target_pid, restore_session.as_ref()) {
        spawn_frame_capture_start(session.id.clone(), pid);
    }

    if let Some(session) = restore_session.as_ref() {
        if session.target_pid.is_some() || session.target_process_name.is_some() {
            spawn_game_restore_monitor(
                session.id.clone(),
                session.target_pid,
                session.target_process_name.clone(),
                snapshot_ids.clone(),
                enter_focus_mode && focus.success,
            );
        }
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
                "stop_nonessential_services": stop_nonessential_services,
                "close_nonessential_apps": close_nonessential_apps,
                "auto_restore": auto_restore,
            },
            "detected_game": detected_game,
            "is_heavy_workload_target": is_heavy_workload,
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
            "restoreStatus": if auto_restore && !snapshot_ids.is_empty() {
                if detected_game.detected { "monitoring" } else { "manual_only" }
            } else {
                "not_started"
            },
            "restore_monitor": {
                "enabled": auto_restore && !snapshot_ids.is_empty() && detected_game.detected,
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
                "services": {
                    "success": services.success,
                    "message": services.message,
                    "details": services.details,
                },
                "gpu_memory_apps": {
                    "success": gpu_memory_apps.success,
                    "message": gpu_memory_apps.message,
                    "details": gpu_memory_apps.details,
                },
                "apps": {
                    "success": apps.success,
                    "message": apps.message,
                    "details": apps.details,
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

// Neither the manual "Modo Gamer" button nor the local-AI auto-trigger ever
// set a profile/mode/scenario key on the command payload, so
// focus::profile_from_payload always fell through to its generic Focus
// default (1h TTL, looser polling/upload throttles) instead of the Game
// profile that exists specifically for this (2h TTL, tighter throttles, and
// a background-app list that doesn't deprioritize the game's own launcher).
// Default to "game" here, but never override a profile the caller already
// chose explicitly.
fn focus_payload_for_game_mode(payload: Option<&Value>) -> Value {
    let mut merged = payload.cloned().unwrap_or_else(|| json!({}));
    match merged.as_object_mut() {
        Some(object) => {
            object.entry("profile").or_insert_with(|| json!("game"));
        }
        None => merged = json!({ "profile": "game" }),
    }
    merged
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
    let capture_id = session.frame_capture_id.take();
    session.status = "restored".to_string();
    session.restored_at = Some(chrono::Utc::now().timestamp());
    session.restore_reason = Some("manual_restore".to_string());
    let _ = write_active_game_mode_session(&session);
    if let Some(capture_id) = capture_id {
        // Called from an async Tauri command (a Tokio runtime is already
        // driving this call), so a detached spawn is enough here - contrast
        // with spawn_game_restore_monitor below, a plain OS thread with no
        // ambient runtime, which has to build its own to await the same
        // async stop call.
        let session_group_id = session.id.clone();
        tokio::spawn(async move {
            stop_frame_capture_and_queue_upload(capture_id, session_group_id).await;
        });
    }
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
        frame_capture_id: None,
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
    let restored_at = chrono::Utc::now().timestamp();
    session.restored_at = Some(restored_at);
    session.restore_reason = Some(reason.to_string());
    let _ = write_active_game_mode_session(&session);
    // Feeds the "game keeps closing right after opening" detector - see
    // game_launch.rs. Local file only; the process name never leaves the PC.
    game_launch::record_session_end(
        session.target_process_name.as_deref(),
        session.created_at,
        restored_at,
        reason,
    );
}

fn game_mode_session_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join("game-mode-session.json")
}

/// Fire-and-forget: starts a PresentMon capture for a just-activated Modo
/// Gamer session, routed through the normal helper client path
/// (execute_command_checked -> privileged_helper::execute over the signed
/// pipe) exactly like any other helper-required action. Detached instead of
/// awaited so APPLY_GAME_MODE's own response never waits on this extra
/// round-trip to the helper.
fn spawn_frame_capture_start(session_id: String, target_pid: u32) {
    tokio::spawn(async move {
        let result = execute_frame_capture_action_with_retry(
            "START_FRAME_CAPTURE",
            json!({ "targetPid": target_pid }),
        )
        .await;

        if !result.success {
            let _ = audit::record_event(
                "info",
                "frame_capture.start_failed",
                "Nao foi possivel iniciar a captura de frames para esta sessao de Modo Gamer.",
                json!({
                    "session_id": session_id,
                    "target_pid": target_pid,
                    "message": result.message,
                }),
            );
            return;
        }

        if let Some(capture_id) = result.details.get("captureId").and_then(Value::as_str) {
            attach_frame_capture_id_to_active_session(session_id, capture_id.to_string()).await;
        }
    });
}

/// Records the running capture's id on the still-active session so the
/// restore path (whichever one fires - process-exit monitor or manual
/// restore) knows to stop it. If the session already restored by the time
/// this helper round-trip came back (game closed unusually fast, or the
/// user hit "restore" manually mid-round-trip), there's nobody left who
/// will ever call STOP_FRAME_CAPTURE for this id - stop it right here
/// instead of leaving PresentMon attached to a process AnalystBlaze no
/// longer considers "in a game session".
async fn attach_frame_capture_id_to_active_session(session_id: String, capture_id: String) {
    let orphaned = match read_active_game_mode_session() {
        Some(mut session) if session.id == session_id && session.restored_at.is_none() => {
            session.frame_capture_id = Some(capture_id.clone());
            let _ = write_active_game_mode_session(&session);
            false
        }
        _ => true,
    };

    if orphaned {
        stop_frame_capture_and_queue_upload(capture_id, session_id).await;
    }
}

/// A dropped-pipe STOP_FRAME_CAPTURE (see execute_command_checked_with_helper's
/// "helper_transiently_unavailable" case) means the helper's named pipe
/// server was mid-cycle right when this call landed, not a real rejection -
/// a live capture (2026-09) hit exactly this window and lost its data
/// outright since nothing ever retried. Bounded to a few short attempts
/// because the capture keeps running (and its buffered samples keep
/// growing) on the helper side the whole time this loop waits, so there's
/// no harm in trying a bit longer - just not forever.
const FRAME_CAPTURE_HELPER_CALL_MAX_ATTEMPTS: u32 = 4;
const FRAME_CAPTURE_HELPER_CALL_RETRY_DELAY: Duration = Duration::from_secs(3);

fn is_transient_helper_unavailable(result: &ExecutionResult) -> bool {
    result
        .details
        .get("blocked_by")
        .and_then(Value::as_str)
        == Some("helper_transiently_unavailable")
}

/// Shared by both START_FRAME_CAPTURE and STOP_FRAME_CAPTURE: a dropped-pipe
/// call (see is_transient_helper_unavailable) means the helper's named pipe
/// server was mid-cycle right when this call landed, not a real rejection -
/// a live capture (2026-09) hit this on *both* ends (once on start, once on
/// stop, in separate sessions) before either had a retry, losing the data
/// outright each time. Only retries that specific, identified-safe case;
/// anything else (denied, invalid payload, ...) returns on the first try.
async fn execute_frame_capture_action_with_retry(
    action_name: &'static str,
    payload: Value,
) -> ExecutionResult {
    let mut result = ExecutionResult {
        success: false,
        message: String::new(),
        details: Value::Null,
    };
    for attempt in 1..=FRAME_CAPTURE_HELPER_CALL_MAX_ATTEMPTS {
        result = execute_command_checked(
            CommandSource::RemoteCommand,
            action_name,
            Some(payload.clone()),
            None,
            true,
        )
        .await;

        if result.success || !is_transient_helper_unavailable(&result) {
            break;
        }
        if attempt < FRAME_CAPTURE_HELPER_CALL_MAX_ATTEMPTS {
            tokio::time::sleep(FRAME_CAPTURE_HELPER_CALL_RETRY_DELAY).await;
        }
    }
    result
}

/// Stops a running frame capture (again via the normal helper client path)
/// and, if it produced any samples, queues the resulting stats for upload
/// on the telemetry engine's next tick (see
/// frame_capture_control::queue_frame_capture_upload - the engine owns the
/// only context with valid backend credentials, so this module can't POST
/// it directly).
async fn stop_frame_capture_and_queue_upload(capture_id: String, session_group_id: String) {
    let result = execute_frame_capture_action_with_retry(
        "STOP_FRAME_CAPTURE",
        json!({ "captureId": capture_id }),
    )
    .await;

    if !result.success {
        let _ = audit::record_event(
            "warn",
            "frame_capture.stop_failed",
            "Nao foi possivel finalizar a captura de frames.",
            json!({
                "capture_id": capture_id,
                "session_group_id": session_group_id,
                "message": result.message,
                "attempts": FRAME_CAPTURE_HELPER_CALL_MAX_ATTEMPTS,
            }),
        );
        return;
    }

    let details = &result.details;
    let sample_count = details.get("sampleCount").and_then(Value::as_u64).unwrap_or(0);
    if sample_count == 0 {
        // Nothing usable was captured (game closed before any frame
        // presented, PresentMon failed to attach, ...) - not worth a
        // training row.
        return;
    }

    let upload = json!({
        "sessionGroupId": session_group_id,
        "mode": "standalone",
        "actionName": "APPLY_GAME_MODE",
        "startedAt": details.get("startedAt"),
        "endedAt": details.get("endedAt"),
        "sampleCount": details.get("sampleCount"),
        "avgFps": details.get("avgFps"),
        "avgFrameTimeMs": details.get("avgFrameTimeMs"),
        "low1PctFps": details.get("low1PctFps"),
        "low0_1PctFps": details.get("low0_1PctFps"),
        "droppedFrameCount": details.get("droppedFrameCount"),
        "stutterCount": details.get("stutterCount"),
        "source": "presentmon",
    });
    if let Err(error) = frame_capture_control::queue_frame_capture_upload(upload) {
        let _ = audit::record_event(
            "warn",
            "frame_capture.queue_failed",
            "Nao foi possivel enfileirar o upload da captura de frames.",
            json!({ "error": error }),
        );
    }
}

/// How often the monitor polls whether the target process is still running.
const GAME_RESTORE_MONITOR_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// This used to be 12*60 cycles (1 hour) - short enough that any real play
/// session longer than that (extremely common) silently left the power plan
/// and process priorities altered forever, since the monitor just gave up
/// with no restore at all (see game_mode.monitor_timeout - it happened 4
/// times on this machine alone, and a much larger number of sessions never
/// even got that far because the monitor thread died with the app process
/// itself on restart/crash, with nothing to pick the session back up - see
/// reconcile_orphaned_game_mode_session_on_startup). 12 hours comfortably
/// outlasts any real session; run_named_pipe_server-style "give up
/// eventually rather than leak forever" logic still applies, but on
/// Part 2, see: the monitor thread now performs the restore instead of
/// abandoning it if this deadline is ever actually reached - a spurious
/// early restore while a session might technically still be running is a
/// minor, self-correcting inconvenience (click "Ativar Modo Gamer" again);
/// leaving the system permanently altered is a much worse failure that goes
/// unnoticed for days.
const GAME_RESTORE_MONITOR_MAX_CYCLES: u32 =
    (12 * 60 * 60) / GAME_RESTORE_MONITOR_POLL_INTERVAL.as_secs() as u32;

/// Shared by the normal "game process exited" path and the timeout safety
/// net: restores the session's snapshots, finalizes any running frame
/// capture, marks the session restored on disk, and tears down the linked
/// Modo Foco session (its own file/TTL - see focus.rs - so it doesn't keep
/// suppressing notifications/uploads for up to its full TTL after the game
/// is already gone).
fn restore_game_mode_session(
    session_id: &str,
    snapshot_ids: &[String],
    linked_focus_session: bool,
    restore_reason: &str,
) {
    let report = snapshot::restore_snapshots_by_ids(snapshot_ids);
    let capture_id = read_active_game_mode_session()
        .filter(|current| current.id == session_id)
        .and_then(|current| current.frame_capture_id);
    mark_game_mode_session_restored(session_id, restore_reason);
    if let Some(capture_id) = capture_id {
        // A plain OS thread (this function is called from thread::spawn),
        // not a Tokio task - no ambient runtime to tokio::spawn onto, so a
        // short-lived current-thread runtime is built just for this one
        // await, mirroring privileged_helper.rs's execute_request, which
        // faces the same "async call from a bare thread" situation.
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(stop_frame_capture_and_queue_upload(
                capture_id,
                session_id.to_string(),
            )),
            Err(error) => {
                let _ = audit::record_event(
                    "warn",
                    "frame_capture.stop_runtime_failed",
                    "Nao foi possivel criar runtime para finalizar a captura de frames.",
                    json!({ "error": error.to_string() }),
                );
            }
        }
    }
    let _ = audit::record_event(
        "info",
        "game_mode.restored_after_game_exit",
        "Modo Gamer restaurado apos fechamento do jogo detectado.",
        serde_json::to_value(&report).unwrap_or(Value::Null),
    );
    if linked_focus_session {
        let _ = focus::restore_focus_session(Some(restore_reason.to_string()));
    }
}

fn spawn_game_restore_monitor(
    session_id: String,
    pid: Option<u32>,
    process_name: Option<String>,
    snapshot_ids: Vec<String>,
    linked_focus_session: bool,
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
        for _ in 0..GAME_RESTORE_MONITOR_MAX_CYCLES {
            thread::sleep(GAME_RESTORE_MONITOR_POLL_INTERVAL);
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

            restore_game_mode_session(
                &session_id,
                &snapshot_ids,
                linked_focus_session,
                "target_process_exit",
            );
            return;
        }

        let _ = audit::record_event(
            "warn",
            "game_mode.monitor_timeout",
            "Monitor de Modo Gamer expirou sem detectar fechamento do jogo - restaurando por seguranca.",
            json!({
                "pid": pid,
                "process_name": process_name,
                "snapshot_ids": snapshot_ids,
            }),
        );
        restore_game_mode_session(
            &session_id,
            &snapshot_ids,
            linked_focus_session,
            "monitor_timeout_safety_restore",
        );
    });
}

/// Startup-time safety net for the other way a session can be abandoned:
/// the monitor above only lives as long as the app process does, so an app
/// restart or crash while Game Mode is active kills the watching thread
/// with nothing left to ever restore that session's snapshots - the same
/// silent "stuck at High Performance forever" outcome the timeout fix above
/// addresses, just via a different cause. Called once from lib.rs's
/// setup(). Two cases: the target process already closed while the app was
/// down (nobody will ever see it exit - restore right now), or it's still
/// running (the in-memory monitor for it is gone - start a fresh one so it
/// gets caught whenever it does close, and the previous one-hour-can't
/// exist here since spawn_game_restore_monitor now runs for 12h anyway).
pub fn reconcile_orphaned_game_mode_session_on_startup() {
    let Some(session) = read_active_game_mode_session() else {
        return;
    };
    if session.status != "monitoring" || session.restored_at.is_some() {
        return;
    }
    if session.target_pid.is_none() && session.target_process_name.is_none() {
        // Nothing to watch for - a manual activation with no detected
        // process leaves this Some(session) but with no exit signal ever
        // possible, same as before this fix (spawn_game_restore_monitor
        // was never called for it in the first place either).
        return;
    }

    let still_running = detection::process_still_running(
        session.target_pid.map(|pid| pid.to_string()).as_deref(),
        session.target_process_name.as_deref(),
    );

    // GameModeSession doesn't persist whether a Modo Foco session was
    // linked to this activation (only the in-memory call that originally
    // spawned the monitor knew that) - so this can't safely restore a
    // focus session here without risking ending an unrelated one that
    // happens to be active for some other reason. Not a real gap in
    // practice: a focus session already carries its own TTL/expiry (see
    // focus.rs) as a backstop independent of this reconciliation.
    if still_running {
        let _ = audit::record_event(
            "info",
            "game_mode.monitor_resumed_after_restart",
            "Sessao de Modo Gamer encontrada ainda ativa ao iniciar o agente - retomando monitoramento.",
            json!({
                "session_id": session.id,
                "pid": session.target_pid,
                "process_name": session.target_process_name,
            }),
        );
        spawn_game_restore_monitor(
            session.id,
            session.target_pid,
            session.target_process_name,
            session.snapshot_ids,
            false,
        );
    } else {
        let _ = audit::record_event(
            "warn",
            "game_mode.orphaned_session_found_at_startup",
            "Sessao de Modo Gamer ficou sem monitor (app fechado/travado) e o jogo ja havia encerrado - restaurando agora.",
            json!({
                "session_id": session.id,
                "pid": session.target_pid,
                "process_name": session.target_process_name,
            }),
        );
        restore_game_mode_session(
            &session.id,
            &session.snapshot_ids,
            false,
            "startup_reconciliation",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for the 2026-09 incident: a real capture's
    /// STOP_FRAME_CAPTURE landed exactly when the helper's named pipe was
    /// mid-cycle, got a "helper_transiently_unavailable" rejection, and
    /// (with no retry existing at the time) lost its data outright. This is
    /// the classifier is_transient_helper_unavailable relies on to decide
    /// whether stop_frame_capture_and_queue_upload's retry loop should keep
    /// trying versus give up immediately.
    #[test]
    fn only_the_transient_helper_unavailable_reason_is_retryable() {
        let transient = ExecutionResult {
            success: false,
            message: "O helper privilegiado estava reiniciando quando este comando chegou. Tente novamente em alguns segundos.".to_string(),
            details: json!({ "blocked_by": "helper_transiently_unavailable" }),
        };
        assert!(is_transient_helper_unavailable(&transient));

        let denied = ExecutionResult {
            success: false,
            message: "Acao recusada.".to_string(),
            details: json!({ "blocked_by": "privileged_helper_unavailable" }),
        };
        assert!(!is_transient_helper_unavailable(&denied));

        let no_details = ExecutionResult {
            success: false,
            message: "erro generico".to_string(),
            details: Value::Null,
        };
        assert!(!is_transient_helper_unavailable(&no_details));
    }

    #[test]
    fn focus_payload_for_game_mode_defaults_profile_to_game() {
        let merged = focus_payload_for_game_mode(None);
        assert_eq!(merged.get("profile").and_then(Value::as_str), Some("game"));

        let merged = focus_payload_for_game_mode(Some(&json!({
            "source": "local_policy",
            "activity": "gaming",
        })));
        assert_eq!(merged.get("profile").and_then(Value::as_str), Some("game"));
        assert_eq!(
            merged.get("source").and_then(Value::as_str),
            Some("local_policy")
        );
    }

    #[test]
    fn focus_payload_for_game_mode_never_overrides_an_explicit_profile() {
        let merged = focus_payload_for_game_mode(Some(&json!({ "profile": "work" })));
        assert_eq!(merged.get("profile").and_then(Value::as_str), Some("work"));
    }
}
