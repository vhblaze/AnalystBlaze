use std::fs;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, Url};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

use crate::audit;
use crate::optimizations::snapshot::app_data_dir;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(4 * 60);
const PERIODIC_CHECK_INTERVAL: Duration = Duration::from_secs(8 * 60 * 60);
const DISMISS_COOLDOWN_SECONDS: i64 = 24 * 60 * 60;
const MANIFEST_PATH_AND_QUERY: &str =
    "/api/v1/updates/manifest?target={{target}}&arch={{arch}}&current_version={{current_version}}";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedAgentUpdaterState {
    dismissed_until: Option<i64>,
    dismissed_version: Option<String>,
    pending_installed_version: Option<String>,
    previous_version: Option<String>,
}

impl PersistedAgentUpdaterState {
    fn load() -> Self {
        fs::read_to_string(state_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let Ok(raw) = serde_json::to_string(self) else {
            return;
        };
        let path = state_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, raw);
    }
}

fn state_path() -> std::path::PathBuf {
    app_data_dir().join("updater-state.json")
}

struct UpdaterRuntimeState {
    persisted: PersistedAgentUpdaterState,
    pending_update: Option<Update>,
    downloaded_bytes: Option<Vec<u8>>,
    checking: bool,
    installing: bool,
    last_checked_at: Option<i64>,
    last_error: Option<String>,
}

pub struct AgentUpdaterState(Mutex<UpdaterRuntimeState>);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub checking: bool,
    pub installing: bool,
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
    pub minimum_version: Option<String>,
    pub mandatory: bool,
    pub downloaded: bool,
    pub last_checked_at: Option<i64>,
    pub last_error: Option<String>,
    pub dismissed_until: Option<i64>,
}

pub fn new_shared_updater_state() -> AgentUpdaterState {
    AgentUpdaterState(Mutex::new(UpdaterRuntimeState {
        persisted: PersistedAgentUpdaterState::load(),
        pending_update: None,
        downloaded_bytes: None,
        checking: false,
        installing: false,
        last_checked_at: None,
        last_error: None,
    }))
}

/// Runs once at startup, before any check. Compares the version we expected to
/// be running after the last user-consented install against what's actually
/// running now, so a successful update (or a silent failure to apply one)
/// shows up in the local audit trail without the user having to ask.
pub fn reconcile_startup_outcome() {
    let mut persisted = PersistedAgentUpdaterState::load();
    let Some(pending_version) = persisted.pending_installed_version.take() else {
        return;
    };
    let previous_version = persisted.previous_version.take();

    if pending_version == APP_VERSION {
        let _ = audit::record_event(
            "info",
            "update.installed_successfully",
            format!("Atualizacao para a versao {APP_VERSION} concluida com sucesso."),
            json!({
                "previousVersion": previous_version,
                "installedVersion": APP_VERSION,
            }),
        );
    } else {
        let _ = audit::record_event(
            "warn",
            "update.install_did_not_apply",
            format!(
                "Esperava-se reiniciar na versao {pending_version}, mas o app segue na versao {APP_VERSION}."
            ),
            json!({
                "expectedVersion": pending_version,
                "runningVersion": APP_VERSION,
            }),
        );
    }

    persisted.save();
}

fn build_manifest_endpoint(api_base_url: &str) -> Result<Url, String> {
    let url = format!("{}{MANIFEST_PATH_AND_QUERY}", api_base_url.trim_end_matches('/'));
    Url::parse(&url).map_err(|error| format!("Endpoint de atualizacao invalido: {error}"))
}

fn minimum_version_of(update: &Update) -> Option<String> {
    minimum_version_from_raw_json(&update.raw_json)
}

fn minimum_version_from_raw_json(raw_json: &serde_json::Value) -> Option<String> {
    raw_json
        .get("minimum_version")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn is_mandatory_update(current_version: &str, minimum_version: Option<&str>) -> bool {
    minimum_version.is_some_and(|minimum| version_lt(current_version, minimum))
}

fn version_tuple(value: &str) -> (u64, u64, u64) {
    let core = value
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or_default();
    let mut parts = core.split('.').map(|chunk| {
        chunk
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn version_lt(candidate: &str, baseline: &str) -> bool {
    version_tuple(candidate) < version_tuple(baseline)
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

async fn build_status(_app: &AppHandle, runtime: &UpdaterRuntimeState) -> UpdateStatus {
    let current_version = APP_VERSION.to_string();
    let (available, version, notes, pub_date, minimum_version, mandatory) =
        match &runtime.pending_update {
            Some(update) => {
                let minimum_version = minimum_version_of(update);
                let mandatory = is_mandatory_update(&current_version, minimum_version.as_deref());
                (
                    true,
                    Some(update.version.clone()),
                    update.body.clone(),
                    update.date.map(|date| date.to_string()),
                    minimum_version,
                    mandatory,
                )
            }
            None => (false, None, None, None, None, false),
        };

    // A dismissal only applies to the specific version the user dismissed -
    // otherwise dismissing "0.1.3 available" would also silently hide a
    // later, different "0.1.4 available" notice for the rest of the 24h
    // cooldown, even though the user never saw or dismissed that one.
    let dismissed_until = if runtime.persisted.dismissed_version.as_deref() == version.as_deref() {
        runtime.persisted.dismissed_until
    } else {
        None
    };

    UpdateStatus {
        current_version,
        checking: runtime.checking,
        installing: runtime.installing,
        available,
        version,
        notes,
        pub_date,
        minimum_version,
        mandatory,
        downloaded: runtime.downloaded_bytes.is_some(),
        last_checked_at: runtime.last_checked_at,
        last_error: runtime.last_error.clone(),
        dismissed_until,
    }
}

fn emit_status_changed(app: &AppHandle, status: &UpdateStatus) {
    let _ = app.emit("update-status-changed", status.clone());
}

/// Checks the manifest endpoint and, if a newer build is available, kicks off
/// a background download so "Atualizar agora" is instant. Never surfaces a
/// user-facing error for network/check failures - only an invalid signature
/// (caught inside the download) is treated as a security event worth telling
/// the user about, since something real was rejected rather than just being
/// unreachable.
pub async fn check_and_maybe_download(app: AppHandle, api_base_url: String) -> UpdateStatus {
    let state = app.state::<AgentUpdaterState>();
    {
        let mut runtime = state.0.lock().await;
        runtime.checking = true;
        runtime.last_error = None;
    }

    let endpoint = build_manifest_endpoint(&api_base_url);
    let check_result = match endpoint {
        Ok(url) => match app.updater_builder().endpoints(vec![url]) {
            Ok(builder) => match builder.build() {
                Ok(updater) => updater.check().await.map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            },
            Err(error) => Err(error.to_string()),
        },
        Err(error) => Err(error),
    };

    let mut should_download: Option<Update> = None;
    {
        let mut runtime = state.0.lock().await;
        runtime.checking = false;
        runtime.last_checked_at = Some(now_ts());

        match check_result {
            Ok(Some(update)) => {
                let is_new_version = runtime
                    .pending_update
                    .as_ref()
                    .map(|existing| existing.version != update.version)
                    .unwrap_or(true);
                if is_new_version {
                    runtime.downloaded_bytes = None;
                    let minimum_version = minimum_version_of(&update);
                    let _ = audit::record_event(
                        "info",
                        "update.detected",
                        format!("Nova versao {} disponivel.", update.version),
                        json!({
                            "currentVersion": update.current_version,
                            "newVersion": update.version,
                            "minimumVersion": minimum_version,
                        }),
                    );
                    should_download = Some(update.clone());
                }
                runtime.pending_update = Some(update);
            }
            Ok(None) => {
                runtime.pending_update = None;
                runtime.downloaded_bytes = None;
            }
            Err(error) => {
                let _ = audit::record_event(
                    "warn",
                    "update.check_failed",
                    "Falha ao verificar atualizacoes; nova tentativa no proximo ciclo.",
                    json!({ "error": error }),
                );
                runtime.last_error = Some(error);
            }
        }
    }

    if let Some(update) = should_download {
        download_in_background(app.clone(), update).await;
    }

    let runtime = state.0.lock().await;
    let status = build_status(&app, &runtime).await;
    drop(runtime);
    emit_status_changed(&app, &status);
    status
}

async fn download_in_background(app: AppHandle, update: Update) {
    let target_version = update.version.clone();
    let download_result = update.download(|_chunk, _total| {}, || {}).await;

    let state = app.state::<AgentUpdaterState>();
    let mut runtime = state.0.lock().await;
    match download_result {
        Ok(bytes) => {
            runtime.downloaded_bytes = Some(bytes);
            let _ = audit::record_event(
                "info",
                "update.download_completed",
                format!("Download da versao {target_version} concluido."),
                json!({ "version": target_version }),
            );
        }
        Err(error) => {
            let message = error.to_string();
            let is_signature_error = message.to_lowercase().contains("signature");
            if is_signature_error {
                let _ = audit::record_event(
                    "error",
                    "update.signature_invalid",
                    "Nao foi possivel validar a atualizacao - nada foi instalado.",
                    json!({ "version": target_version, "error": message }),
                );
                runtime.last_error =
                    Some("Nao foi possivel validar a atualizacao - nada foi instalado.".to_string());
            } else {
                let _ = audit::record_event(
                    "warn",
                    "update.download_failed",
                    "Falha ao baixar atualizacao em segundo plano; nova tentativa no proximo ciclo.",
                    json!({ "version": target_version, "error": message }),
                );
                runtime.last_error = Some(message);
            }
        }
    }
    let status = build_status(&app, &runtime).await;
    drop(runtime);
    emit_status_changed(&app, &status);
}

pub async fn get_status(app: AppHandle) -> UpdateStatus {
    let state = app.state::<AgentUpdaterState>();
    let runtime = state.0.lock().await;
    build_status(&app, &runtime).await
}

/// User pressed "Atualizar agora": installs from the already-downloaded bytes
/// when available, otherwise downloads first. Only ever runs on explicit
/// consent - nothing here is reachable from the background check path.
pub async fn apply_update(app: AppHandle) -> Result<UpdateStatus, String> {
    let state = app.state::<AgentUpdaterState>();

    let (update, bytes) = {
        let mut runtime = state.0.lock().await;
        let Some(update) = runtime.pending_update.clone() else {
            return Err("Nenhuma atualizacao disponivel para instalar.".to_string());
        };
        let bytes = runtime.downloaded_bytes.take();
        runtime.installing = true;
        (update, bytes)
    };

    let bytes = match bytes {
        Some(bytes) => bytes,
        None => match update.download(|_chunk, _total| {}, || {}).await {
            Ok(bytes) => bytes,
            Err(error) => {
                let message = error.to_string();
                let mut runtime = state.0.lock().await;
                runtime.installing = false;
                if message.to_lowercase().contains("signature") {
                    let _ = audit::record_event(
                        "error",
                        "update.signature_invalid",
                        "Nao foi possivel validar a atualizacao - nada foi instalado.",
                        json!({ "version": update.version, "error": message }),
                    );
                    return Err("Nao foi possivel validar a atualizacao - nada foi instalado.".to_string());
                }
                return Err(message);
            }
        },
    };

    let mut persisted = PersistedAgentUpdaterState::load();
    persisted.previous_version = Some(APP_VERSION.to_string());
    persisted.pending_installed_version = Some(update.version.clone());
    persisted.save();

    let _ = audit::record_event(
        "info",
        "update.install_started",
        format!("Instalacao da versao {} iniciada apos confirmacao do usuario.", update.version),
        json!({ "fromVersion": APP_VERSION, "toVersion": update.version }),
    );

    let install_result = tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result.map_err(|error| error.to_string()));

    if let Err(error) = install_result {
        let mut runtime = state.0.lock().await;
        runtime.installing = false;
        runtime.last_error = Some(error.clone());
        let mut persisted = PersistedAgentUpdaterState::load();
        persisted.pending_installed_version = None;
        persisted.save();
        let _ = audit::record_event(
            "error",
            "update.install_failed",
            "Falha ao instalar a atualizacao baixada.",
            json!({ "error": error }),
        );
        return Err(error);
    }

    app.request_restart();
    let runtime = state.0.lock().await;
    Ok(build_status(&app, &runtime).await)
}

/// User pressed "Depois": stop re-prompting for this version for a cooldown
/// window. Availability stays visible passively (badge) - callers just keep
/// reading `get_status`, which still reports `available: true`.
pub async fn dismiss_update(app: AppHandle) -> UpdateStatus {
    let state = app.state::<AgentUpdaterState>();
    let mut runtime = state.0.lock().await;
    let version = runtime
        .pending_update
        .as_ref()
        .map(|update| update.version.clone());

    runtime.persisted.dismissed_until = Some(now_ts() + DISMISS_COOLDOWN_SECONDS);
    runtime.persisted.dismissed_version = version.clone();
    runtime.persisted.save();

    let _ = audit::record_event(
        "info",
        "update.dismissed",
        "Usuario adiou a atualizacao disponivel.",
        json!({ "version": version, "cooldownSeconds": DISMISS_COOLDOWN_SECONDS }),
    );

    build_status(&app, &runtime).await
}

/// Startup-delayed + periodic background checks. Runs for the whole life of
/// the app; failures are logged and retried on the next tick (see
/// `check_and_maybe_download`), never surfaced as a hard error to the user.
pub fn spawn_background_checks(app: AppHandle, api_base_url: String) {
    let _handle = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_CHECK_DELAY).await;
        loop {
            check_and_maybe_download(app.clone(), api_base_url.clone()).await;
            tokio::time::sleep(PERIODIC_CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn version_tuple_parses_plain_semver() {
        assert_eq!(version_tuple("0.1.0"), (0, 1, 0));
        assert_eq!(version_tuple("1.2.3"), (1, 2, 3));
    }

    #[test]
    fn version_tuple_ignores_v_prefix_and_prerelease_suffix() {
        assert_eq!(version_tuple("v0.3.0-beta.1"), (0, 3, 0));
        assert_eq!(version_tuple("V2.0.0+build.5"), (2, 0, 0));
    }

    #[test]
    fn version_tuple_defaults_missing_segments_to_zero() {
        assert_eq!(version_tuple("1"), (1, 0, 0));
        assert_eq!(version_tuple("1.2"), (1, 2, 0));
        assert_eq!(version_tuple(""), (0, 0, 0));
    }

    #[test]
    fn version_lt_compares_numerically_not_lexicographically() {
        // A naive string compare would say "0.9.0" > "0.10.0".
        assert!(version_lt("0.9.0", "0.10.0"));
        assert!(!version_lt("0.10.0", "0.9.0"));
        assert!(!version_lt("1.0.0", "1.0.0"));
    }

    #[test]
    fn minimum_version_from_raw_json_reads_extra_field() {
        let raw = json!({ "version": "0.5.0", "minimum_version": "0.3.0" });
        assert_eq!(minimum_version_from_raw_json(&raw), Some("0.3.0".to_string()));

        let without_field = json!({ "version": "0.5.0" });
        assert_eq!(minimum_version_from_raw_json(&without_field), None);
    }

    #[test]
    fn is_mandatory_update_true_when_current_below_minimum() {
        assert!(is_mandatory_update("0.1.0", Some("0.2.0")));
        assert!(!is_mandatory_update("0.2.0", Some("0.2.0")));
        assert!(!is_mandatory_update("0.3.0", Some("0.2.0")));
    }

    #[test]
    fn is_mandatory_update_false_when_no_minimum_declared() {
        assert!(!is_mandatory_update("0.1.0", None));
    }

    #[test]
    fn dismiss_cooldown_is_24_hours() {
        assert_eq!(DISMISS_COOLDOWN_SECONDS, 24 * 60 * 60);
    }

    #[test]
    fn persisted_state_round_trips_through_json() {
        let state = PersistedAgentUpdaterState {
            dismissed_until: Some(1_700_000_000),
            dismissed_version: Some("0.4.0".to_string()),
            pending_installed_version: Some("0.4.0".to_string()),
            previous_version: Some("0.3.0".to_string()),
        };
        let raw = serde_json::to_string(&state).expect("state should serialize");
        let decoded: PersistedAgentUpdaterState =
            serde_json::from_str(&raw).expect("state should deserialize");
        assert_eq!(decoded.dismissed_until, state.dismissed_until);
        assert_eq!(decoded.pending_installed_version, state.pending_installed_version);
    }

    #[test]
    fn build_manifest_endpoint_substitutes_base_url_and_keeps_placeholders() {
        let url = build_manifest_endpoint("https://api.analystblaze.com").expect("valid url");
        let serialized = url.to_string();
        assert!(serialized.starts_with("https://api.analystblaze.com/api/v1/updates/manifest"));
        assert!(serialized.contains("target="));
        assert!(serialized.contains("current_version="));
    }
}
