#![recursion_limit = "256"]

mod api;
mod audit;
mod auth;
mod config;
mod optimizations;
mod process_ext;
mod telemetry;
mod updater;

use std::sync::Mutex;

use serde::Serialize;
use serde_json::json;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_deep_link::DeepLinkExt;
use telemetry::collector::TelemetryCollector;
use telemetry::engine::{TelemetryEngineHandle, TelemetryMode};
use telemetry::state::{
    new_shared_telemetry_state, SharedTelemetryState, TelemetryDashboardSnapshot,
};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::auth::{
    auth_callback_from_deep_link, profile_from_credentials, profile_from_token, profile_from_value,
    AuthCallback, AuthProfile, AuthTokens, SecureStore, StoredCredentials,
};
use crate::config::AgentConfig;

struct AgentState {
    config: AgentConfig,
    api: ApiClient,
    store: SecureStore,
    telemetry: Mutex<Option<TelemetryEngineHandle>>,
    telemetry_state: SharedTelemetryState,
    /// Category of the last background plan-sync failure ("network", "tls",
    /// "timeout", "dns", "unavailable", "unknown"), if any. Cleared on the
    /// next successful sync. Never used to alter the cached plan itself -
    /// see refresh_account_profile_if_needed.
    plan_sync_error: Mutex<Option<String>>,
    /// Last completed disk-usage scan, kept in memory only (cleared on
    /// restart) so repeated UI visits don't force a rescan every time.
    disk_usage_cache: Mutex<Option<optimizations::disk_usage::DiskUsageSummary>>,
    /// Set while a scan is in flight so cancel_disk_usage_scan has
    /// something to signal; cleared when the scan finishes either way.
    disk_usage_cancel: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    /// Last completed all-files disk-tree scan (D6 "Explorador de Disco"),
    /// kept in memory only - never serialized wholesale, only browsed via
    /// get_disk_tree_children/get_disk_tree_node.
    disk_tree_cache: Mutex<Option<optimizations::disk_tree::DiskTree>>,
    /// Set while a disk-tree scan is in flight so cancel_disk_tree_scan has
    /// something to signal; cleared when the scan finishes either way.
    disk_tree_cancel: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    /// Rolling ~10 minute window of network samples collected only while
    /// Modo Live is active - see spawn_live_mode_loop.
    live_mode_samples: Mutex<std::collections::VecDeque<telemetry::live_mode::LiveModeSample>>,
    /// Set while the Modo Live loop is running so stop_live_mode has
    /// something to signal; None means the loop isn't active.
    live_mode_cancel: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    live_mode_last_incident: Mutex<Option<telemetry::live_mode::IncidentReport>>,
    /// Latest starter-plan weekly automation budget reported by the server
    /// on the last commands poll (see telemetry::engine::poll_commands).
    /// `None` for paid plans (server sends no limit) or before the first
    /// poll after startup.
    weekly_ai_usage: Mutex<Option<api::WeeklyAiTelemetryUsage>>,
    /// Latest admin-broadcast announcements from the last commands poll
    /// (see telemetry::engine::poll_commands). Empty before the first poll.
    announcements: Mutex<Vec<api::Announcement>>,
    /// Set while the free-plan Game Mode usage checkpoint loop is running,
    /// so a manual restore can signal it to stop immediately instead of
    /// waiting for its own ~60s self-check. See spawn_game_mode_usage_checkpoint_loop.
    game_mode_usage_cancel: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

#[derive(Debug, Clone, Serialize)]
struct AgentStatus {
    authenticated: bool,
    registered: bool,
    hw_id: Option<String>,
    user_name: Option<String>,
    user_email: Option<String>,
    plan: String,
    has_paid_plan: bool,
    mode: String,
    api_base_url: String,
    web_login_url: String,
    account_settings_url: String,
    billing_url: String,
    insights_url: String,
    focus_session: Option<optimizations::focus::FocusSession>,
    /// Unix seconds of the last confirmed server-side plan check, or `None`
    /// if it has never succeeded since pairing (plan shown is still the
    /// last known-good value from login/registration - never blank).
    plan_synced_at: Option<i64>,
    /// Set when the most recent background/manual sync attempt failed, so
    /// the UI can show "not synced" instead of silently implying freshness.
    plan_sync_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SingleInstancePayload {
    args: Vec<String>,
    cwd: String,
}

#[derive(Debug, Clone, Serialize)]
struct GameModeResult {
    success: bool,
    message: String,
    details: serde_json::Value,
    status: AgentStatus,
}

#[derive(Debug, Clone, Serialize)]
struct OptimizationPreview {
    action_name: String,
    risk: String,
    requires_local_confirmation: bool,
    requires_snapshot: bool,
    requires_privileged_helper: bool,
    allowed_without_helper: bool,
    message: String,
}

#[tauri::command]
async fn restore_pending_optimizations() -> Result<optimizations::snapshot::RestoreReport, String> {
    tokio::task::spawn_blocking(optimizations::snapshot::restore_pending_snapshots)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn optimization_snapshots(
    limit: Option<usize>,
) -> Result<Vec<optimizations::snapshot::OptimizationSnapshot>, String> {
    tokio::task::spawn_blocking(move || {
        optimizations::snapshot::list_snapshots(limit.unwrap_or(80))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn audit_log(limit: Option<usize>) -> Result<Vec<audit::AuditEvent>, String> {
    tokio::task::spawn_blocking(move || audit::recent_events(limit.unwrap_or(120)))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn optimization_preview(
    action_name: String,
    payload: Option<serde_json::Value>,
) -> OptimizationPreview {
    let Some(profile) = optimizations::safety::command_profile(&action_name) else {
        return OptimizationPreview {
            action_name,
            risk: "unknown".to_string(),
            requires_local_confirmation: true,
            requires_snapshot: false,
            requires_privileged_helper: false,
            allowed_without_helper: false,
            message: "Acao desconhecida pela allowlist local.".to_string(),
        };
    };

    let context = optimizations::safety::SafetyContext {
        source: optimizations::safety::CommandSource::ManualUser,
        allowed_actions: None,
        local_confirmation: true,
        privileged_helper_available: false,
    };
    let allowed_without_helper =
        optimizations::safety::validate_command(&action_name, payload.as_ref(), &context).is_ok();

    OptimizationPreview {
        action_name,
        risk: format!("{:?}", profile.risk).to_ascii_lowercase(),
        requires_local_confirmation: profile.requires_local_confirmation,
        requires_snapshot: profile.requires_snapshot,
        requires_privileged_helper: profile.requires_privileged_helper,
        allowed_without_helper,
        message: if profile.requires_privileged_helper {
            "Esta acao exige helper privilegiado com UAC explicito.".to_string()
        } else if profile.requires_snapshot {
            "Esta acao altera o Windows e deve criar snapshot antes de executar.".to_string()
        } else {
            "Acao permitida pela camada local de seguranca.".to_string()
        },
    }
}

#[tauri::command]
fn resolve_remote_command_confirmation(request_id: String, approved: bool) -> bool {
    let Ok(request_id) = Uuid::parse_str(&request_id) else {
        return false;
    };
    telemetry::engine::resolve_remote_command_confirmation(request_id, approved)
}

#[tauri::command]
async fn windows_inventory() -> Result<optimizations::windows_inventory::WindowsInventory, String> {
    tokio::task::spawn_blocking(optimizations::windows_inventory::collect_windows_inventory)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn network_diagnostics() -> Result<telemetry::network::NetworkDiagnostics, String> {
    tokio::task::spawn_blocking(telemetry::network::collect_network_diagnostics)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn energy_diagnostics() -> Result<optimizations::energy::EnergyDiagnostics, String> {
    tokio::task::spawn_blocking(optimizations::energy::collect_energy_diagnostics)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn protected_apps() -> Result<Vec<optimizations::protected_apps::ProtectedApp>, String> {
    tokio::task::spawn_blocking(optimizations::protected_apps::list_protected_apps)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn add_protected_app(
    name: String,
    reason: Option<String>,
) -> Result<Vec<optimizations::protected_apps::ProtectedApp>, String> {
    tokio::task::spawn_blocking(move || {
        optimizations::protected_apps::add_protected_app(name, reason)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn remove_protected_app(
    name: String,
) -> Result<Vec<optimizations::protected_apps::ProtectedApp>, String> {
    tokio::task::spawn_blocking(move || optimizations::protected_apps::remove_protected_app(name))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn privileged_helper_status() -> optimizations::privileged_helper::PrivilegedHelperStatus {
    optimizations::privileged_helper::status()
}

#[tauri::command]
fn install_privileged_helper(
) -> Result<optimizations::privileged_helper::PrivilegedHelperStatus, String> {
    optimizations::privileged_helper::install()
}

#[tauri::command]
fn uninstall_privileged_helper(
) -> Result<optimizations::privileged_helper::PrivilegedHelperStatus, String> {
    optimizations::privileged_helper::uninstall()
}

#[tauri::command]
fn restart_privileged_helper(
) -> Result<optimizations::privileged_helper::PrivilegedHelperStatus, String> {
    optimizations::privileged_helper::restart()
}

#[tauri::command]
fn start_privileged_helper(
) -> Result<optimizations::privileged_helper::PrivilegedHelperStatus, String> {
    optimizations::privileged_helper::start()
}

#[tauri::command]
fn stop_privileged_helper(
) -> Result<optimizations::privileged_helper::PrivilegedHelperStatus, String> {
    optimizations::privileged_helper::stop()
}

#[tauri::command]
fn test_privileged_helper(
) -> Result<optimizations::privileged_helper::PrivilegedHelperHandshake, String> {
    optimizations::privileged_helper::handshake()
}

#[tauri::command]
async fn deep_clean_temp() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "EMPTY_TEMP",
        Some(serde_json::json!({
            "mode": "deep_confirmed",
            "min_age_minutes": 5,
            "include_windows_temp": true,
        })),
    )
    .await)
}

#[tauri::command]
async fn purge_cleanup_quarantine() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "PURGE_CLEANUP_QUARANTINE",
        Some(serde_json::json!({
            "user_confirmed_purge": true,
            "confirmation": "purge_cleanup_quarantine",
        })),
    )
    .await)
}

#[tauri::command]
async fn restore_active_game_mode(
    state: State<'_, AgentState>,
) -> Result<optimizations::snapshot::RestoreReport, String> {
    let report = optimizations::restore_active_game_mode_session();

    // Best-effort: signal the checkpoint loop (if any) to stop immediately
    // instead of waiting up to ~60s for its own self-check to notice the
    // session is gone, and tell the server right away so the remaining-time
    // display updates promptly. Never lets a network hiccup block the
    // (already-local) restore itself.
    if let Ok(mut guard) = state.game_mode_usage_cancel.lock() {
        if let Some(cancel) = guard.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    if let Ok(current_status) = status(&state) {
        if !current_status.has_paid_plan {
            if let Ok(credentials) = state.store.load() {
                if let (Some(access_token), Some(hw_id)) = (credentials.access_token, credentials.hw_id) {
                    let _ = state.api.stop_game_mode_usage(&access_token, hw_id).await;
                }
            }
        }
    }

    Ok(report)
}

#[tauri::command]
fn active_game_mode_session() -> Option<optimizations::GameModeSession> {
    optimizations::active_game_mode_session()
}

#[tauri::command]
async fn activate_focus_mode(
    profile: Option<String>,
    duration_seconds: Option<i64>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "ENTER_FOCUS_MODE",
        Some(serde_json::json!({
            "profile": profile.unwrap_or_else(|| "focus".to_string()),
            "durationSeconds": duration_seconds,
        })),
    )
    .await)
}

#[tauri::command]
async fn restore_focus_session() -> Result<optimizations::snapshot::RestoreReport, String> {
    tokio::task::spawn_blocking(|| {
        optimizations::focus::restore_focus_session(Some("user_undo".to_string()))
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn active_focus_session() -> Option<optimizations::focus::FocusSession> {
    optimizations::focus::active_focus_session()
}

#[tauri::command]
async fn restore_latency_session() -> Result<optimizations::snapshot::RestoreReport, String> {
    tokio::task::spawn_blocking(|| {
        optimizations::latency::restore_latency_session(Some("user_undo".to_string()))
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn active_latency_session() -> Option<optimizations::latency::LatencySession> {
    optimizations::latency::active_latency_session()
}

#[tauri::command]
async fn run_performance_scan(
    mode: String,
    state: State<'_, AgentState>,
) -> Result<optimizations::performance_suite::PerformanceReport, String> {
    let credentials = state.store.load().unwrap_or_default();
    let device_id = credentials.hw_id.map(|hw_id| hw_id.to_string());
    let report = optimizations::performance_suite::run_performance_scan(mode, device_id).await?;
    let _ = sync_performance_report_summary(&state, &report).await;
    Ok(report)
}

#[tauri::command]
async fn apply_pc_clean_fast_profile(
    include_startup: Option<bool>,
    include_cleanup: Option<bool>,
    include_background: Option<bool>,
    include_network: Option<bool>,
    include_gaming: Option<bool>,
    state: State<'_, AgentState>,
) -> Result<optimizations::ExecutionResult, String> {
    let options = optimizations::performance_suite::PcCleanFastOptions {
        include_startup: include_startup.unwrap_or(true),
        include_cleanup: include_cleanup.unwrap_or(true),
        include_background: include_background.unwrap_or(true),
        include_network: include_network.unwrap_or(false),
        include_gaming: include_gaming.unwrap_or(true),
    };
    let result = optimizations::performance_suite::apply_pc_clean_fast_profile(options).await;
    if let Some(after_report) = result.details.get("afterReport").cloned() {
        if let Ok(mut report) = serde_json::from_value::<
            optimizations::performance_suite::PerformanceReport,
        >(after_report)
        {
            let credentials = state.store.load().unwrap_or_default();
            report.device_id = credentials.hw_id.map(|hw_id| hw_id.to_string());
            let _ = sync_performance_report_summary(&state, &report).await;
        }
    }
    Ok(result)
}

#[tauri::command]
async fn restore_performance_session(
    session_id: Option<String>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::performance_suite::restore_performance_session(session_id))
}

#[tauri::command]
async fn scan_cleanup_categories(
) -> Result<Vec<optimizations::performance_suite::CleanupCategory>, String> {
    optimizations::performance_suite::scan_cleanup_categories().await
}

#[tauri::command]
async fn apply_cleanup_category(
    category: String,
    mode: Option<String>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::performance_suite::apply_cleanup_category(category, mode).await)
}

const DISK_USAGE_CACHE_TTL_SECONDS: i64 = 10 * 60;

#[tauri::command]
async fn disk_usage_summary(
    force_refresh: bool,
    state: State<'_, AgentState>,
    app: AppHandle,
) -> Result<optimizations::disk_usage::DiskUsageSummary, String> {
    if !force_refresh {
        let cached = state
            .disk_usage_cache
            .lock()
            .map_err(|_| "Estado do agente bloqueado.".to_string())?
            .clone();
        if let Some(summary) = cached {
            let age_seconds = chrono::Utc::now().timestamp() - summary.scanned_at;
            if (0..DISK_USAGE_CACHE_TTL_SECONDS).contains(&age_seconds) {
                return Ok(summary);
            }
        }
    }

    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut guard = state
            .disk_usage_cancel
            .lock()
            .map_err(|_| "Estado do agente bloqueado.".to_string())?;
        *guard = Some(cancel.clone());
    }

    let summary = optimizations::disk_usage::scan_disk_usage(app, cancel).await;

    if let Ok(mut guard) = state.disk_usage_cancel.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = state.disk_usage_cache.lock() {
        *guard = Some(summary.clone());
    }

    Ok(summary)
}

#[tauri::command]
async fn cancel_disk_usage_scan(state: State<'_, AgentState>) -> Result<bool, String> {
    let guard = state
        .disk_usage_cancel
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?;
    match guard.as_ref() {
        Some(cancel) => {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(true)
        }
        None => Ok(false),
    }
}

#[tauri::command]
async fn delete_disk_usage_item(path: String) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "DELETE_DISK_USAGE_ITEM",
        Some(serde_json::json!({ "path": path })),
    )
    .await)
}

#[tauri::command]
fn list_disk_volumes() -> Vec<optimizations::disk_tree::DiskVolumeInfo> {
    optimizations::disk_tree::list_volumes()
}

/// Full all-files scan of `root` (D6 "Explorador de Disco"). Unlike
/// disk_usage_summary this has no TTL cache reuse - a fresh scan is a
/// deliberate user action (picking/re-scanning a drive), not something
/// that happens incidentally on every screen visit.
#[tauri::command]
async fn scan_disk_tree(
    root: String,
    state: State<'_, AgentState>,
    app: AppHandle,
) -> Result<optimizations::disk_tree::DiskTreeScanSummary, String> {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut guard = state
            .disk_tree_cancel
            .lock()
            .map_err(|_| "Estado do agente bloqueado.".to_string())?;
        *guard = Some(cancel.clone());
    }

    let (tree, summary) = optimizations::disk_tree::scan_disk_tree(app, cancel, root).await?;

    if let Ok(mut guard) = state.disk_tree_cancel.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = state.disk_tree_cache.lock() {
        *guard = Some(tree);
    }

    Ok(summary)
}

#[tauri::command]
async fn cancel_disk_tree_scan(state: State<'_, AgentState>) -> Result<bool, String> {
    let guard = state
        .disk_tree_cancel
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?;
    match guard.as_ref() {
        Some(cancel) => {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(true)
        }
        None => Ok(false),
    }
}

#[tauri::command]
fn get_disk_tree_node(
    path: String,
    state: State<'_, AgentState>,
) -> Result<optimizations::disk_tree::DiskTreeNodeSummary, String> {
    let guard = state
        .disk_tree_cache
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?;
    let tree = guard.as_ref().ok_or_else(|| "no_scan_cached".to_string())?;
    optimizations::disk_tree::node_summary(tree, &path)
}

#[tauri::command]
fn get_disk_tree_children(
    path: String,
    state: State<'_, AgentState>,
) -> Result<Vec<optimizations::disk_tree::DiskTreeNodeSummary>, String> {
    let guard = state
        .disk_tree_cache
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?;
    let tree = guard.as_ref().ok_or_else(|| "no_scan_cached".to_string())?;
    optimizations::disk_tree::children_of(tree, &path)
}

const LIVE_MODE_SAMPLE_EVENT: &str = "live-mode-sample";
const LIVE_MODE_INCIDENT_EVENT: &str = "live-mode-incident";
const LIVE_MODE_MAX_SAMPLES: usize = 300;
const LIVE_MODE_SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
const LIVE_MODE_INCIDENT_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(120);

const GAME_MODE_USAGE_EVENT: &str = "game-mode-usage-updated";
const GAME_MODE_CHECKPOINT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
const GAME_MODE_CHECKPOINT_MAX_MISSES: u8 = 2;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveModeStatus {
    active: bool,
    samples: Vec<telemetry::live_mode::LiveModeSample>,
    bitrate_recommendation: Option<telemetry::live_mode::BitrateRecommendation>,
    last_incident: Option<telemetry::live_mode::IncidentReport>,
}

#[tauri::command]
async fn detect_live_mode_streaming_app() -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(telemetry::live_mode::detect_foreground_streaming_app)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_live_mode(state: State<'_, AgentState>, app: AppHandle) -> Result<(), String> {
    let mut guard = state
        .live_mode_cancel
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?;
    if guard.is_some() {
        return Ok(());
    }
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    *guard = Some(cancel.clone());
    drop(guard);

    if let Ok(mut samples) = state.live_mode_samples.lock() {
        samples.clear();
    }
    spawn_live_mode_loop(app, cancel);
    let _ = audit::record_event(
        "info",
        "live_mode.started",
        "Modo Live ativado.",
        serde_json::json!({}),
    );
    Ok(())
}

#[tauri::command]
async fn stop_live_mode(state: State<'_, AgentState>) -> Result<(), String> {
    let mut guard = state
        .live_mode_cancel
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?;
    if let Some(cancel) = guard.take() {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = audit::record_event(
            "info",
            "live_mode.stopped",
            "Modo Live desativado.",
            serde_json::json!({}),
        );
    }
    Ok(())
}

#[tauri::command]
async fn live_mode_status(state: State<'_, AgentState>) -> Result<LiveModeStatus, String> {
    let active = state
        .live_mode_cancel
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?
        .is_some();
    let samples: std::collections::VecDeque<_> = state
        .live_mode_samples
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?
        .clone();
    let bitrate_recommendation = telemetry::live_mode::recommend_bitrate(&samples);
    let last_incident = state
        .live_mode_last_incident
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?
        .clone();

    Ok(LiveModeStatus {
        active,
        samples: samples.into_iter().collect(),
        bitrate_recommendation,
        last_incident,
    })
}

#[tauri::command]
fn weekly_automation_usage(
    state: State<'_, AgentState>,
) -> Result<Option<api::WeeklyAiTelemetryUsage>, String> {
    Ok(state
        .weekly_ai_usage
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?
        .clone())
}

#[tauri::command]
fn active_announcements(state: State<'_, AgentState>) -> Result<Vec<api::Announcement>, String> {
    Ok(state
        .announcements
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?
        .clone())
}

#[tauri::command]
async fn weekly_game_mode_usage(
    state: State<'_, AgentState>,
) -> Result<Option<api::GameModeUsage>, String> {
    let current_status = status(&state)?;
    if current_status.has_paid_plan {
        return Ok(None);
    }
    let credentials = state.store.load()?;
    let (Some(access_token), Some(hw_id)) = (credentials.access_token, credentials.hw_id) else {
        return Ok(None);
    };
    Ok(state
        .api
        .weekly_game_mode_usage(&access_token, hw_id)
        .await
        .ok())
}

#[tauri::command]
async fn apply_insight_action(
    action_name: String,
    title: Option<String>,
    reason: Option<String>,
    state: State<'_, AgentState>,
) -> Result<(), String> {
    ensure_registered(&state)?;
    let credentials = state.store.load()?;
    let (Some(access_token), Some(hw_id)) = (credentials.access_token, credentials.hw_id) else {
        return Err("Faca login pela Web antes de aplicar um insight.".to_string());
    };
    state
        .api
        .apply_insight_action(&access_token, hw_id, &action_name, title.as_deref(), reason.as_deref())
        .await
}

#[tauri::command]
async fn generate_live_mode_incident_report(
    state: State<'_, AgentState>,
) -> Result<telemetry::live_mode::IncidentReport, String> {
    let samples = state
        .live_mode_samples
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?
        .clone();
    let report = telemetry::live_mode::build_incident_report(&samples);
    if let Ok(mut guard) = state.live_mode_last_incident.lock() {
        *guard = Some(report.clone());
    }
    let _ = audit::record_event(
        "warning",
        "live_mode.incident_report",
        "Relatorio de incidente do Modo Live gerado manualmente.",
        serde_json::json!({ "causes": report.causes, "sampleCount": report.sample_count }),
    );
    Ok(report)
}

/// Runs only while Modo Live is active (see start_live_mode/stop_live_mode).
/// Every tick is a single lightweight network sample - never touches
/// Windows state, never calls execute_command, never escalates to the
/// safety/execution pipeline. Auto-generates an incident report (with a
/// cooldown, so one bad tick doesn't spam the audit log) when the latest
/// sample looks like a real anomaly.
fn spawn_live_mode_loop(app: AppHandle, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    tauri::async_runtime::spawn(async move {
        let mut last_incident_at: Option<std::time::Instant> = None;
        loop {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }

            if let Ok(sample) = tokio::task::spawn_blocking(telemetry::live_mode::sample_now).await
            {
                let state = app.state::<AgentState>();
                let pushed = {
                    let mut samples = state.live_mode_samples.lock().ok();
                    samples.as_mut().map(|samples| {
                        samples.push_back(sample.clone());
                        while samples.len() > LIVE_MODE_MAX_SAMPLES {
                            samples.pop_front();
                        }
                        let anomaly = telemetry::live_mode::detect_anomaly(samples);
                        (anomaly, (**samples).clone())
                    })
                };
                let _ = app.emit(LIVE_MODE_SAMPLE_EVENT, &sample);

                if let Some((anomaly, samples_snapshot)) = pushed {
                    let should_report = anomaly
                        && last_incident_at
                            .map(|at| at.elapsed() >= LIVE_MODE_INCIDENT_COOLDOWN)
                            .unwrap_or(true);
                    if should_report {
                        last_incident_at = Some(std::time::Instant::now());
                        let report = telemetry::live_mode::build_incident_report(&samples_snapshot);
                        if let Ok(mut guard) = state.live_mode_last_incident.lock() {
                            *guard = Some(report.clone());
                        }
                        let _ = audit::record_event(
                            "warning",
                            "live_mode.incident_report_auto",
                            "Anomalia de rede detectada automaticamente pelo Modo Live.",
                            serde_json::json!({ "causes": report.causes, "sampleCount": report.sample_count }),
                        );
                        let _ = app.emit(LIVE_MODE_INCIDENT_EVENT, &report);
                    }
                }
            }

            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            tokio::time::sleep(LIVE_MODE_SAMPLE_INTERVAL).await;
        }
    });
}

#[tauri::command]
async fn scan_startup_impact(
) -> Result<Vec<optimizations::performance_suite::StartupImpact>, String> {
    optimizations::performance_suite::scan_startup_impact().await
}

#[tauri::command]
async fn delay_startup_app(
    name: String,
    location: Option<String>,
    delay_seconds: Option<u64>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::performance_suite::delay_startup_app(name, location, delay_seconds).await)
}

#[tauri::command]
async fn restore_delayed_startup_app(
    name: Option<String>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::performance_suite::restore_delayed_startup_app(name).await)
}

#[tauri::command]
async fn local_ai_policy() -> Result<optimizations::local_ai_policy::LocalAiPolicy, String> {
    tokio::task::spawn_blocking(optimizations::local_ai_policy::load_local_ai_policy)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_local_ai_policy(
    policy: optimizations::local_ai_policy::LocalAiPolicy,
) -> Result<optimizations::local_ai_policy::LocalAiPolicy, String> {
    tokio::task::spawn_blocking(move || {
        optimizations::local_ai_policy::save_local_ai_policy(policy)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn disable_startup_app(
    name: String,
    location: Option<String>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "DISABLE_STARTUP_APP",
        Some(serde_json::json!({
            "name": name,
            "location": location,
        })),
    )
    .await)
}

#[tauri::command]
async fn restore_startup_app(
    name: Option<String>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "RESTORE_STARTUP_APP",
        Some(serde_json::json!({
            "name": name,
        })),
    )
    .await)
}

#[tauri::command]
async fn stop_windows_service(name: String) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "STOP_SERVICE",
        Some(serde_json::json!({
            "service_name": name,
        })),
    )
    .await)
}

#[tauri::command]
async fn restore_windows_service(
    name: Option<String>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "RESTORE_SERVICE",
        Some(serde_json::json!({
            "service_name": name,
        })),
    )
    .await)
}

#[tauri::command]
async fn flush_dns_cache() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command("FLUSH_DNS_CACHE", None).await)
}

#[tauri::command]
async fn set_dns_servers(
    adapter_name: String,
    dns_servers: Vec<String>,
) -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "SET_DNS_SERVERS",
        Some(serde_json::json!({
            "adapterName": adapter_name,
            "dnsServers": dns_servers,
        })),
    )
    .await)
}

#[tauri::command]
async fn reset_winsock_catalog() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command(
        "RESET_WINSOCK_CATALOG",
        Some(serde_json::json!({ "confirm": "RESET_WINSOCK" })),
    )
    .await)
}

#[tauri::command]
async fn list_network_adapters() -> Result<Vec<telemetry::network::NetworkAdapterSummary>, String>
{
    tokio::task::spawn_blocking(telemetry::network::list_network_adapters)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_visual_performance_mode() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command("APPLY_VISUAL_PERFORMANCE_MODE", None).await)
}

#[tauri::command]
async fn restore_visual_effects() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command("RESTORE_VISUAL_EFFECTS", None).await)
}

#[tauri::command]
async fn set_power_plan_high_performance() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command("SET_POWER_PLAN_HIGH_PERFORMANCE", None).await)
}

#[tauri::command]
async fn set_power_plan_balanced() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command("SET_POWER_PLAN_BALANCED", None).await)
}

#[tauri::command]
async fn set_power_plan_power_saver() -> Result<optimizations::ExecutionResult, String> {
    Ok(optimizations::execute_command("SET_POWER_PLAN_POWER_SAVER", None).await)
}

#[tauri::command]
async fn agent_status(state: State<'_, AgentState>) -> Result<AgentStatus, String> {
    // Deliberately does NOT block on a network call: on some Windows 10
    // machines a TLS/cert-store hiccup can make that request hang, which
    // used to freeze the whole "is my plan verified" flow on every app
    // open. Freshness comes from the background sync loop (see
    // spawn_plan_sync_loop) and the manual sync_account_plan command.
    status(&state)
}

#[tauri::command]
async fn sync_account_plan(state: State<'_, AgentState>) -> Result<AgentStatus, String> {
    refresh_account_profile_if_needed(&state).await?;
    status(&state)
}

#[tauri::command]
async fn open_login(state: State<'_, AgentState>) -> Result<String, String> {
    tauri_plugin_opener::open_url(&state.config.web_login_url, None::<&str>)
        .map_err(|error| error.to_string())?;
    Ok(state.config.web_login_url.clone())
}

#[tauri::command]
async fn open_account_settings(state: State<'_, AgentState>) -> Result<String, String> {
    tauri_plugin_opener::open_url(&state.config.web_account_url, None::<&str>)
        .map_err(|error| error.to_string())?;
    Ok(state.config.web_account_url.clone())
}

#[tauri::command]
async fn open_billing(state: State<'_, AgentState>) -> Result<String, String> {
    tauri_plugin_opener::open_url(&state.config.web_billing_url, None::<&str>)
        .map_err(|error| error.to_string())?;
    Ok(state.config.web_billing_url.clone())
}

#[tauri::command]
async fn open_web_insights(state: State<'_, AgentState>) -> Result<String, String> {
    tauri_plugin_opener::open_url(&state.config.web_insights_url, None::<&str>)
        .map_err(|error| error.to_string())?;
    Ok(state.config.web_insights_url.clone())
}

#[tauri::command]
async fn complete_auth_from_deep_link(
    raw_url: String,
    state: State<'_, AgentState>,
    app: AppHandle,
) -> Result<AgentStatus, String> {
    show_main_window(&app);
    let tokens = match auth_callback_from_deep_link(&raw_url)? {
        AuthCallback::Tokens(tokens) => tokens,
        AuthCallback::PairingCode(code) => state.api.exchange_desktop_pairing_code(&code).await?,
    };
    complete_auth_tokens(tokens, state, app).await
}

async fn complete_auth_tokens(
    tokens: AuthTokens,
    state: State<'_, AgentState>,
    app: AppHandle,
) -> Result<AgentStatus, String> {
    let profile = {
        let collector = TelemetryCollector::new();
        collector.hardware_profile(state.config.telemetry_include_hostname)
    };
    let registration = state
        .api
        .register_hardware(&tokens.access_token, &profile)
        .await?;
    let api_profile = state
        .api
        .account_profile(&tokens.access_token)
        .await
        .ok()
        .flatten()
        .map(|value| profile_from_value(&value))
        .unwrap_or_default();
    let tokens = AuthTokens {
        profile: tokens.profile.merge(api_profile),
        ..tokens
    };
    let existing = state.store.load()?;
    let credentials = credentials_from_registration(tokens, registration, existing);

    state.store.save(&credentials)?;
    if credentials_complete(&credentials) {
        ensure_agent_running(&state, &app)?;
    }
    status(&state)
}

fn credentials_from_registration(
    tokens: AuthTokens,
    registration: api::HardwareRegistration,
    existing: StoredCredentials,
) -> StoredCredentials {
    let existing_profile = profile_from_credentials(&existing);
    let existing_secret = if existing.hw_id == Some(registration.id) {
        existing.hw_secret.clone()
    } else {
        None
    };
    let hw_secret = if registration.hw_secret == "REDACTED" {
        existing_secret
    } else {
        Some(registration.hw_secret)
    };

    StoredCredentials {
        access_token: Some(tokens.access_token),
        refresh_token: tokens.refresh_token,
        hw_id: Some(registration.id),
        hw_secret,
        user_name: tokens.profile.user_name.or(existing_profile.user_name),
        user_email: tokens.profile.user_email.or(existing_profile.user_email),
        plan: tokens
            .profile
            .plan
            .or(existing_profile.plan)
            .or_else(|| Some("starter".to_string())),
        has_paid_plan: tokens
            .profile
            .has_paid_plan
            .or(existing_profile.has_paid_plan)
            .or(Some(false)),
        // Not yet actively confirmed via refresh_account_profile_if_needed -
        // the background sync loop populates this on its first tick.
        plan_synced_at: None,
    }
}

/// Confirms the cached plan against the server and persists it on success.
/// Never removes or downgrades the cached plan on failure - a network drop,
/// TLS error, or a temporarily unreachable API leaves the last known-good
/// value in place; the failure is only recorded for diagnostics/UI display.
async fn refresh_account_profile_if_needed(state: &AgentState) -> Result<(), String> {
    let credentials = state.store.load()?;
    let Some(access_token) = credentials.access_token.clone() else {
        return Ok(());
    };

    match state.api.account_profile(&access_token).await {
        Ok(Some(value)) => {
            let api_profile = profile_from_value(&value);
            if profile_is_empty(&api_profile) {
                record_plan_sync_outcome(
                    state,
                    Some("empty_profile".to_string()),
                    "Resposta da API sem dados de plano reconheciveis.",
                );
                return Ok(());
            }
            let mut updated = credentials_with_profile(credentials, api_profile);
            updated.plan_synced_at = Some(current_unix_time());
            state.store.save(&updated)?;
            record_plan_sync_outcome(state, None, "Plano sincronizado com o servidor.");
        }
        Ok(None) => {
            record_plan_sync_outcome(
                state,
                Some("unavailable".to_string()),
                "Nenhum endpoint de conta respondeu com sucesso.",
            );
        }
        Err(error) => {
            let category = categorize_plan_sync_error(&error);
            record_plan_sync_outcome(state, Some(category.to_string()), &error);
        }
    }

    Ok(())
}

fn current_unix_time() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Best-effort classification of a reqwest error string into a stable,
/// loggable category. reqwest's Display output reliably contains these
/// substrings across TLS/DNS/timeout failure modes on Windows.
fn categorize_plan_sync_error(message: &str) -> &'static str {
    let lower = message.to_lowercase();
    if lower.contains("certificate") || lower.contains("tls") || lower.contains("ssl") {
        "tls"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else if lower.contains("dns") || lower.contains("resolve") || lower.contains("lookup") {
        "dns"
    } else if lower.contains("connect") {
        "network"
    } else {
        "unknown"
    }
}

fn record_plan_sync_outcome(state: &AgentState, category: Option<String>, message: &str) {
    if let Ok(mut guard) = state.plan_sync_error.lock() {
        *guard = category.clone();
    }
    let level = if category.is_some() { "warning" } else { "info" };
    let event = if category.is_some() {
        "account.plan_sync_failed"
    } else {
        "account.plan_synced"
    };
    let _ = audit::record_event(
        level,
        event,
        message,
        serde_json::json!({ "category": category }),
    );
}

fn credentials_with_profile(
    credentials: StoredCredentials,
    profile: AuthProfile,
) -> StoredCredentials {
    let merged = profile.merge(profile_from_credentials(&credentials));

    StoredCredentials {
        user_name: merged.user_name,
        user_email: merged.user_email,
        plan: merged.plan.or_else(|| Some("starter".to_string())),
        has_paid_plan: merged.has_paid_plan.or(Some(false)),
        ..credentials
    }
}

fn profile_is_empty(profile: &AuthProfile) -> bool {
    profile.user_name.is_none()
        && profile.user_email.is_none()
        && profile.plan.is_none()
        && profile.has_paid_plan.is_none()
}

#[tauri::command]
fn start_agent(state: State<'_, AgentState>, app: AppHandle) -> Result<AgentStatus, String> {
    ensure_registered(&state)?;
    ensure_agent_running(&state, &app)?;
    status(&state)
}

#[tauri::command]
async fn activate_game_mode(
    state: State<'_, AgentState>,
    app: AppHandle,
) -> Result<GameModeResult, String> {
    ensure_registered(&state)?;
    ensure_agent_running(&state, &app)?;

    let current_status = status(&state)?;

    if !current_status.has_paid_plan {
        let credentials = state.store.load()?;
        let (Some(access_token), Some(hw_id)) = (credentials.access_token, credentials.hw_id) else {
            return Err("Faca login pela Web antes de ativar o Modo Gamer.".to_string());
        };

        // Fail closed: without the server confirming remaining weekly
        // budget, the free plan can't prove it has time left, so Game Mode
        // simply doesn't activate rather than silently allowing unlimited
        // use whenever the server/network is unreachable.
        let usage = match state.api.start_game_mode_usage(&access_token, hw_id).await {
            Ok(usage) => usage,
            Err(error) => {
                return Ok(GameModeResult {
                    success: false,
                    message: "Nao foi possivel confirmar o saldo semanal de Modo Gamer com o servidor."
                        .to_string(),
                    details: json!({
                        "implemented": true,
                        "blocked_reason": "server_unreachable",
                        "error": error,
                    }),
                    status: current_status,
                });
            }
        };

        if usage.limit_reached {
            return Ok(GameModeResult {
                success: false,
                message: "Limite semanal de Modo Gamer do plano gratuito atingido.".to_string(),
                details: json!({
                    "implemented": true,
                    "blocked_reason": "weekly_limit_reached",
                    "usage": usage,
                }),
                status: current_status,
            });
        }

        let result = optimizations::execute_command("APPLY_GAME_MODE", None).await;
        let status_after = status(&state)?;

        if result.success {
            spawn_game_mode_usage_checkpoint_loop(app, access_token, hw_id);
        } else {
            let _ = state.api.stop_game_mode_usage(&access_token, hw_id).await;
        }

        return Ok(GameModeResult {
            success: result.success,
            message: result.message,
            details: result.details,
            status: status_after,
        });
    }

    let result = optimizations::execute_command("APPLY_GAME_MODE", None).await;
    let status_after = status(&state)?;

    Ok(GameModeResult {
        success: result.success,
        message: result.message,
        details: result.details,
        status: status_after,
    })
}

/// Runs only while a free-plan Game Mode session is active. Every tick
/// either checkpoints elapsed usage with the server or, if the session was
/// already restored through some other path (manual button, the game-exit
/// auto-restore monitor in optimizations::mod), reports the final stop and
/// exits quietly. Two consecutive unreachable checkpoints, or the server
/// reporting the weekly budget exhausted mid-session, force a local restore
/// - see the "fail closed" discussion in RELEASING.md-adjacent design notes.
fn spawn_game_mode_usage_checkpoint_loop(app: AppHandle, access_token: String, hw_id: Uuid) {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let state = app.state::<AgentState>();
        if let Ok(mut guard) = state.game_mode_usage_cancel.lock() {
            *guard = Some(cancel.clone());
        };
    }

    tauri::async_runtime::spawn(async move {
        let mut misses: u8 = 0;
        loop {
            tokio::time::sleep(GAME_MODE_CHECKPOINT_INTERVAL).await;
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }

            if optimizations::active_game_mode_session().is_none() {
                let _ = app
                    .state::<AgentState>()
                    .api
                    .stop_game_mode_usage(&access_token, hw_id)
                    .await;
                break;
            }

            match app
                .state::<AgentState>()
                .api
                .checkpoint_game_mode_usage(&access_token, hw_id)
                .await
            {
                Ok(usage) => {
                    misses = 0;
                    let _ = app.emit(GAME_MODE_USAGE_EVENT, &usage);
                    if usage.limit_reached {
                        let _ = audit::record_event(
                            "warn",
                            "game_mode.weekly_limit_reached",
                            "Limite semanal de Modo Gamer do plano gratuito atingido; restaurando automaticamente.",
                            json!({}),
                        );
                        optimizations::restore_active_game_mode_session();
                        break;
                    }
                }
                Err(_) => {
                    misses += 1;
                    if misses >= GAME_MODE_CHECKPOINT_MAX_MISSES {
                        let _ = audit::record_event(
                            "warn",
                            "game_mode.checkpoint_unreachable",
                            "Nao foi possivel confirmar o saldo semanal de Modo Gamer com o servidor; restaurando por seguranca.",
                            json!({}),
                        );
                        optimizations::restore_active_game_mode_session();
                        break;
                    }
                }
            }
        }
    });
}

#[tauri::command]
fn set_telemetry_mode(mode: String, state: State<'_, AgentState>) -> Result<AgentStatus, String> {
    let mode = match mode.as_str() {
        "normal" => TelemetryMode::Normal,
        "realtime" => TelemetryMode::Realtime,
        _ => return Err("Modo de telemetria invalido.".to_string()),
    };

    let guard = state
        .telemetry
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?;
    if let Some(engine) = guard.as_ref() {
        engine.set_mode(mode)?;
    }
    drop(guard);
    status(&state)
}

#[tauri::command]
fn logout(state: State<'_, AgentState>) -> Result<AgentStatus, String> {
    state.store.clear()?;
    if let Ok(mut telemetry_state) = state.telemetry_state.try_write() {
        *telemetry_state = None;
    }
    status(&state)
}

#[tauri::command]
fn collect_once() -> telemetry::collector::TelemetrySample {
    let mut collector = TelemetryCollector::new();
    collector.collect()
}

#[tauri::command]
async fn telemetry_snapshot(
    state: State<'_, AgentState>,
) -> Result<Option<TelemetryDashboardSnapshot>, String> {
    Ok(state.telemetry_state.read().await.clone())
}

#[tauri::command]
async fn update_status(app: AppHandle) -> updater::UpdateStatus {
    updater::get_status(app).await
}

#[tauri::command]
async fn check_for_update(app: AppHandle, state: State<'_, AgentState>) -> Result<updater::UpdateStatus, String> {
    Ok(updater::check_and_maybe_download(app, state.config.api_base_url.clone()).await)
}

#[tauri::command]
async fn apply_update(app: AppHandle) -> Result<updater::UpdateStatus, String> {
    updater::apply_update(app).await
}

#[tauri::command]
async fn dismiss_update(app: AppHandle) -> Result<updater::UpdateStatus, String> {
    Ok(updater::dismiss_update(app).await)
}

#[tauri::command]
async fn fetch_authenticated_insights(
    accept_language: Option<String>,
    state: State<'_, AgentState>,
) -> Result<serde_json::Value, String> {
    let credentials = state.store.load()?;
    let access_token = credentials
        .access_token
        .as_deref()
        .ok_or_else(|| "Faca login antes de buscar insights.".to_string())?;
    let hw_id = credentials
        .hw_id
        .ok_or_else(|| "Hardware ainda nao registrado para buscar insights.".to_string())?;

    state
        .api
        .insights(access_token, hw_id, accept_language.as_deref())
        .await
}

async fn sync_performance_report_summary(
    state: &AgentState,
    report: &optimizations::performance_suite::PerformanceReport,
) -> Result<(), String> {
    let credentials = state.store.load()?;
    let access_token = credentials
        .access_token
        .as_deref()
        .ok_or_else(|| "Login ausente para sincronizar performance summary.".to_string())?;
    let hw_id = credentials
        .hw_id
        .ok_or_else(|| "Hardware ainda nao registrado para performance summary.".to_string())?;
    let mut summary = optimizations::performance_suite::performance_summary_payload(report);
    if let Some(object) = summary.as_object_mut() {
        object.insert("deviceId".to_string(), serde_json::json!(hw_id.to_string()));
    }
    state
        .api
        .post_performance_summary(access_token, hw_id, &summary)
        .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = AgentConfig::from_env();
    let store = SecureStore::new().expect("Falha ao inicializar Windows Credential Manager");
    let api = ApiClient::new(config.api_base_url.clone());

    let state = AgentState {
        config,
        api,
        store,
        telemetry: Mutex::new(None),
        telemetry_state: new_shared_telemetry_state(),
        plan_sync_error: Mutex::new(None),
        disk_usage_cache: Mutex::new(None),
        disk_usage_cancel: Mutex::new(None),
        disk_tree_cache: Mutex::new(None),
        disk_tree_cancel: Mutex::new(None),
        live_mode_samples: Mutex::new(std::collections::VecDeque::new()),
        live_mode_cancel: Mutex::new(None),
        live_mode_last_incident: Mutex::new(None),
        weekly_ai_usage: Mutex::new(None),
        announcements: Mutex::new(Vec::new()),
        game_mode_usage_cancel: Mutex::new(None),
    };

    let updater_api_base_url = state.config.api_base_url.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            // A relaunch attempt (deep-link auth included) means the user is
            // actively trying to interact with the app right now - closing
            // the window only hides it (see CloseRequested below), so
            // without this the app can silently finish pairing in the tray
            // and the user never sees it happen.
            show_main_window(app);
            let _ = app.emit("single-instance", SingleInstancePayload { args, cwd });
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .manage(updater::new_shared_updater_state())
        .setup(move |app| {
            configure_tray(app)?;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.center();
            }

            #[cfg(desktop)]
            {
                let _ = app.deep_link().register("analystblaze");
            }

            let state = app.state::<AgentState>();
            if state
                .store
                .load()
                .map(|credentials| credentials_complete(&credentials))
                .unwrap_or(false)
            {
                let _ = ensure_agent_running(&state, app.handle());
            }
            spawn_plan_sync_loop(app.handle().clone());
            optimizations::performance_suite::spawn_delayed_startup_runner();

            // Syncs the Run-key entry to match the saved preference (default
            // enabled for new installs) on every launch, so it self-heals if
            // the registry entry was ever removed some other way.
            std::thread::spawn(|| {
                let policy = optimizations::local_ai_policy::load_local_ai_policy();
                let _ = optimizations::autostart::set_autostart_enabled(policy.autostart_enabled);
            });

            updater::reconcile_startup_outcome();
            updater::spawn_background_checks(app.handle().clone(), updater_api_base_url.clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            agent_status,
            sync_account_plan,
            open_login,
            open_account_settings,
            open_billing,
            open_web_insights,
            complete_auth_from_deep_link,
            start_agent,
            activate_game_mode,
            restore_pending_optimizations,
            optimization_snapshots,
            audit_log,
            optimization_preview,
            resolve_remote_command_confirmation,
            windows_inventory,
            network_diagnostics,
            energy_diagnostics,
            flush_dns_cache,
            set_dns_servers,
            reset_winsock_catalog,
            list_network_adapters,
            protected_apps,
            add_protected_app,
            remove_protected_app,
            privileged_helper_status,
            install_privileged_helper,
            uninstall_privileged_helper,
            restart_privileged_helper,
            start_privileged_helper,
            stop_privileged_helper,
            test_privileged_helper,
            deep_clean_temp,
            purge_cleanup_quarantine,
            restore_active_game_mode,
            active_game_mode_session,
            activate_focus_mode,
            restore_focus_session,
            active_focus_session,
            restore_latency_session,
            active_latency_session,
            run_performance_scan,
            apply_pc_clean_fast_profile,
            restore_performance_session,
            scan_cleanup_categories,
            apply_cleanup_category,
            disk_usage_summary,
            cancel_disk_usage_scan,
            delete_disk_usage_item,
            list_disk_volumes,
            scan_disk_tree,
            cancel_disk_tree_scan,
            get_disk_tree_node,
            get_disk_tree_children,
            detect_live_mode_streaming_app,
            start_live_mode,
            stop_live_mode,
            live_mode_status,
            generate_live_mode_incident_report,
            weekly_automation_usage,
            active_announcements,
            weekly_game_mode_usage,
            apply_insight_action,
            scan_startup_impact,
            delay_startup_app,
            restore_delayed_startup_app,
            local_ai_policy,
            save_local_ai_policy,
            disable_startup_app,
            restore_startup_app,
            stop_windows_service,
            restore_windows_service,
            apply_visual_performance_mode,
            restore_visual_effects,
            set_power_plan_high_performance,
            set_power_plan_balanced,
            set_power_plan_power_saver,
            set_telemetry_mode,
            logout,
            collect_once,
            telemetry_snapshot,
            fetch_authenticated_insights,
            update_status,
            check_for_update,
            apply_update,
            dismiss_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub fn run_privileged_helper_service() {
    optimizations::privileged_helper::run_service();
}

fn configure_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Abrir AnalystBlaze").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&quit)
        .build()?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("AnalystBlaze")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id() == "show" {
                show_main_window(app);
            } else if event.id() == "quit" {
                let _ =
                    optimizations::latency::restore_latency_session(Some("app_exit".to_string()));
                let _ = optimizations::focus::restore_focus_session(Some("app_exit".to_string()));
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

const PLAN_SYNC_PERIODIC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Runs for the whole life of the app once credentials exist: an immediate
/// sync right after startup (so a stale/changed plan gets corrected without
/// requiring a manual action), then a retry every 30 minutes. Failures are
/// logged via record_plan_sync_outcome and simply retried on the next tick -
/// never surfaced as a hard error (see refresh_account_profile_if_needed).
fn spawn_plan_sync_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let state = app.state::<AgentState>();
            let credentials_complete = state
                .store
                .load()
                .map(|credentials| credentials_complete(&credentials))
                .unwrap_or(false);
            if credentials_complete {
                let _ = refresh_account_profile_if_needed(&state).await;
                if let Ok(fresh_status) = status(&state) {
                    let _ = app.emit("plan-synced", fresh_status);
                }
            }
            tokio::time::sleep(PLAN_SYNC_PERIODIC_INTERVAL).await;
        }
    });
}

fn ensure_agent_running(state: &AgentState, app: &AppHandle) -> Result<(), String> {
    let mut guard = state
        .telemetry
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?;
    if guard.is_none() {
        let engine = TelemetryEngineHandle::spawn(
            state.config.clone(),
            state.api.clone(),
            state.store.clone(),
            state.telemetry_state.clone(),
            app.clone(),
        );
        *guard = Some(engine);
    }
    Ok(())
}

fn ensure_registered(state: &AgentState) -> Result<(), String> {
    let credentials = state.store.load()?;
    if credentials_complete(&credentials) {
        Ok(())
    } else {
        Err("Faca login pela Web antes de iniciar o agente desktop.".to_string())
    }
}

fn credentials_complete(credentials: &StoredCredentials) -> bool {
    credentials.access_token.is_some()
        && credentials.hw_id.is_some()
        && credentials.hw_secret.is_some()
}

fn status(state: &AgentState) -> Result<AgentStatus, String> {
    let credentials = state.store.load()?;
    let token_profile = credentials
        .access_token
        .as_deref()
        .map(profile_from_token)
        .unwrap_or_default();
    let account_profile = profile_from_credentials(&credentials).merge(token_profile);
    let mode = state
        .telemetry
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?
        .as_ref()
        .map(|engine| engine.mode().as_str().to_string())
        .unwrap_or_else(|| "stopped".to_string());
    let plan_sync_error = state
        .plan_sync_error
        .lock()
        .map_err(|_| "Estado do agente bloqueado.".to_string())?
        .clone();

    Ok(AgentStatus {
        authenticated: credentials.access_token.is_some(),
        registered: credentials_complete(&credentials),
        hw_id: credentials.hw_id.map(|value| value.to_string()),
        user_name: account_profile.user_name,
        user_email: account_profile.user_email,
        plan: account_profile
            .plan
            .unwrap_or_else(|| "starter".to_string()),
        has_paid_plan: account_profile.has_paid_plan.unwrap_or(false),
        mode,
        api_base_url: state.config.api_base_url.clone(),
        web_login_url: state.config.web_login_url.clone(),
        account_settings_url: state.config.web_account_url.clone(),
        billing_url: state.config.web_billing_url.clone(),
        insights_url: state.config.web_insights_url.clone(),
        focus_session: optimizations::focus::active_focus_session(),
        plan_synced_at: credentials.plan_synced_at,
        plan_sync_error,
    })
}

#[cfg(test)]
mod plan_sync_tests {
    use super::*;

    #[test]
    fn categorizes_tls_and_certificate_failures() {
        assert_eq!(
            categorize_plan_sync_error("invalid peer certificate: UnknownIssuer"),
            "tls"
        );
        assert_eq!(categorize_plan_sync_error("SSL routines failure"), "tls");
    }

    #[test]
    fn categorizes_timeouts_separately_from_generic_connect_errors() {
        assert_eq!(
            categorize_plan_sync_error("error sending request: operation timed out"),
            "timeout"
        );
        assert_eq!(
            categorize_plan_sync_error("error sending request: tcp connect error"),
            "network"
        );
    }

    #[test]
    fn categorizes_dns_failures() {
        assert_eq!(
            categorize_plan_sync_error("error trying to connect: dns error: failed to lookup address"),
            "dns"
        );
    }

    #[test]
    fn unrecognized_errors_fall_back_to_unknown() {
        assert_eq!(categorize_plan_sync_error("something unexpected"), "unknown");
    }

    #[test]
    fn credentials_from_registration_starts_with_unsynced_plan() {
        let tokens = AuthTokens {
            access_token: "token".to_string(),
            refresh_token: None,
            profile: AuthProfile::default(),
        };
        let registration = api::HardwareRegistration {
            id: Uuid::nil(),
            status: "active".to_string(),
            hw_secret: "secret".to_string(),
            message: "ok".to_string(),
        };
        let credentials =
            credentials_from_registration(tokens, registration, StoredCredentials::default());
        assert_eq!(credentials.plan_synced_at, None);
    }
}
