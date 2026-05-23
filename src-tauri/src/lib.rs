mod api;
mod audit;
mod auth;
mod config;
mod optimizations;
mod telemetry;

use std::sync::Mutex;

use serde::Serialize;
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
async fn complete_auth_from_deep_link(
    raw_url: String,
    state: State<'_, AgentState>,
    app: AppHandle,
) -> Result<AgentStatus, String> {
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
        collector.hardware_profile()
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
    }
}

async fn refresh_account_profile_if_needed(state: &AgentState) -> Result<(), String> {
    let credentials = state.store.load()?;
    let Some(access_token) = credentials.access_token.clone() else {
        return Ok(());
    };
    let Some(api_profile) = state
        .api
        .account_profile(&access_token)
        .await
        .ok()
        .flatten()
        .map(|value| profile_from_value(&value))
    else {
        return Ok(());
    };
    if profile_is_empty(&api_profile) {
        return Ok(());
    }

    state
        .store
        .save(&credentials_with_profile(credentials, api_profile))
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

    let result = optimizations::execute_command("APPLY_GAME_MODE", None).await;
    let status = status(&state)?;

    Ok(GameModeResult {
        success: result.success,
        message: result.message,
        details: result.details,
        status,
    })
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
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let _ = app.emit("single-instance", SingleInstancePayload { args, cwd });
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .setup(|app| {
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
            open_login,
            open_account_settings,
            complete_auth_from_deep_link,
            start_agent,
            activate_game_mode,
            restore_pending_optimizations,
            optimization_snapshots,
            audit_log,
            optimization_preview,
            windows_inventory,
            network_diagnostics,
            energy_diagnostics,
            protected_apps,
            add_protected_app,
            remove_protected_app,
            privileged_helper_status,
            local_ai_policy,
            save_local_ai_policy,
            disable_startup_app,
            restore_startup_app,
            stop_windows_service,
            restore_windows_service,
            set_power_plan_high_performance,
            set_power_plan_balanced,
            set_power_plan_power_saver,
            set_telemetry_mode,
            logout,
            collect_once,
            telemetry_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
        .tooltip("AnalystBlaze Agent")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id() == "show" {
                show_main_window(app);
            } else if event.id() == "quit" {
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
    })
}
