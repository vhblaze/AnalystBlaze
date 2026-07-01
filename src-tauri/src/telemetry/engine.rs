use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{oneshot, watch};
use tokio::time::{interval, sleep, timeout, Duration, MissedTickBehavior};
use uuid::Uuid;

use crate::api::{AgentPolicyBundle, ApiClient, CommandAcknowledgement, CommandResponse};
use crate::auth::{SecureStore, StoredCredentials};
use crate::config::AgentConfig;
use crate::optimizations::{self, safety::CommandSource};

use super::collector::{TelemetryCollector, TelemetrySample};
use super::state::{
    SharedTelemetryState, TelemetryDashboardSnapshot, AGENT_SESSION_INVALIDATED_EVENT,
    TELEMETRY_UPDATE_EVENT,
};

pub const REMOTE_COMMAND_CONFIRMATION_EVENT: &str = "remote-command-confirmation-request";
const REMOTE_COMMAND_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(120);
const AGENT_EVENT_MAX_JSON_BYTES: usize = 10_000;
static PENDING_REMOTE_COMMAND_CONFIRMATIONS: OnceLock<Mutex<HashMap<Uuid, oneshot::Sender<bool>>>> =
    OnceLock::new();

struct AgentOptimizationEventInput<'a> {
    hw_id: Uuid,
    app_version: String,
    action_name: &'a str,
    command_id: Option<Uuid>,
    before: &'a TelemetrySample,
    after: &'a TelemetrySample,
    success: bool,
    execution_details: Value,
    privacy: TelemetryPrivacyPolicy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCommandConfirmationRequest {
    pub request_id: Uuid,
    pub command_id: Uuid,
    pub action_name: String,
    pub title: String,
    pub description: String,
    pub risk: String,
    pub snapshot: bool,
    pub authorization_mode: Option<String>,
    pub authorization_id: Option<String>,
    pub context_key: Option<String>,
}

pub fn resolve_remote_command_confirmation(request_id: Uuid, approved: bool) -> bool {
    let pending = PENDING_REMOTE_COMMAND_CONFIRMATIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut pending) = pending.lock() else {
        return false;
    };
    let Some(sender) = pending.remove(&request_id) else {
        return false;
    };
    sender.send(approved).is_ok()
}

fn focus_main_window_for_confirmation(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = window.request_user_attention(Some(tauri::UserAttentionType::Critical));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryMode {
    Normal,
    Realtime,
}

impl TelemetryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Realtime => "realtime",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub event_timestamp: i64,
    pub cpu_usage: f64,
    pub gpu_usage: f64,
    pub gpu_name: String,
    pub vram_gb: f64,
    pub ram_usage_mb: f64,
    pub context_state: serde_json::Value,
    pub action_taken: String,
    pub details: serde_json::Value,
    pub score_delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryBatch {
    pub hw_id: Uuid,
    pub app_version: String,
    pub decisions: Vec<DecisionRecord>,
    pub timestamp: i64,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeTelemetryPayload {
    pub hw_id: Uuid,
    pub app_version: String,
    pub event_timestamp: i64,
    pub cpu_usage: f64,
    pub gpu_usage: f64,
    pub gpu_name: String,
    pub vram_gb: f64,
    pub ram_usage_mb: f64,
    pub context_state: serde_json::Value,
    pub details: serde_json::Value,
    pub timestamp: i64,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationMetrics {
    pub cpu_usage: f64,
    pub gpu_usage: f64,
    pub ram_usage_mb: f64,
    pub ram_usage_percent: f64,
    pub cpu_temperature: f64,
    pub cpu_temperature_available: bool,
    pub gpu_temperature: f64,
    pub gpu_temperature_available: bool,
    pub vram_usage_percent: Option<f64>,
    pub disk_usage_percent: f64,
    pub active_processes: usize,
    pub idle_seconds: u64,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOptimizationEventPayload {
    pub hw_id: Uuid,
    pub app_version: String,
    pub action_name: String,
    pub command_id: Option<Uuid>,
    pub event_timestamp: i64,
    pub context_state: Value,
    pub before_metrics: OptimizationMetrics,
    pub after_metrics: OptimizationMetrics,
    pub delta_metrics: Value,
    pub execution_details: Value,
    pub success: bool,
    pub timestamp: i64,
    pub nonce: String,
}

#[derive(Debug, Clone)]
struct LocalPolicyDecision {
    action_name: String,
    action_payload: Value,
    confidence: f64,
    reason: String,
    cooldown_seconds: u64,
    requires_automatic_sensitive_consent: bool,
}

#[derive(Debug, Clone, Copy)]
struct TelemetryPrivacyPolicy {
    diagnostics_enabled: bool,
    include_ssid: bool,
    include_hostname: bool,
    family_detail_consent: bool,
    family_plan: bool,
}

impl TelemetryPrivacyPolicy {
    fn from_config_and_credentials(
        config: &AgentConfig,
        credentials: Option<&StoredCredentials>,
    ) -> Self {
        Self {
            diagnostics_enabled: config.telemetry_diagnostics_enabled,
            include_ssid: config.telemetry_include_ssid,
            include_hostname: config.telemetry_include_hostname,
            family_detail_consent: config.telemetry_family_detail_consent,
            family_plan: credentials.is_some_and(credentials_plan_is_family),
        }
    }

    fn detailed_process_allowed(self) -> bool {
        self.diagnostics_enabled && (!self.family_plan || self.family_detail_consent)
    }
}

#[derive(Clone)]
pub struct TelemetryEngineHandle {
    mode_tx: watch::Sender<TelemetryMode>,
    manual_mode_override_tx: watch::Sender<Option<TelemetryMode>>,
}

impl TelemetryEngineHandle {
    pub fn spawn(
        config: AgentConfig,
        api: ApiClient,
        store: SecureStore,
        telemetry_state: SharedTelemetryState,
        app_handle: AppHandle,
    ) -> Self {
        let (mode_tx, mode_rx) = watch::channel(TelemetryMode::Normal);
        let (manual_mode_override_tx, manual_mode_override_rx) =
            watch::channel(None::<TelemetryMode>);
        let engine = TelemetryEngine {
            config,
            api,
            store,
            app_handle,
            telemetry_state,
            mode_rx,
            mode_tx: mode_tx.clone(),
            manual_mode_override_rx,
            batch: Vec::with_capacity(128),
            collector: TelemetryCollector::new(),
            last_sample: None,
            policy: None,
            last_local_action_at: None,
            last_local_action_by_name: HashMap::new(),
            backend_backoff_until: None,
            batch_backoff_until: None,
            backend_failure_count: 0,
            last_realtime_push_at: None,
            last_realtime_status_at: None,
            coalesced_realtime_samples: 0,
        };

        tauri::async_runtime::spawn(async move {
            engine.run().await;
        });

        Self {
            mode_tx,
            manual_mode_override_tx,
        }
    }

    pub fn set_mode(&self, mode: TelemetryMode) -> Result<(), String> {
        self.manual_mode_override_tx
            .send(Some(mode))
            .map_err(|error| error.to_string())?;
        self.mode_tx.send(mode).map_err(|error| error.to_string())
    }

    pub fn mode(&self) -> TelemetryMode {
        *self.mode_tx.borrow()
    }
}

struct TelemetryEngine {
    config: AgentConfig,
    api: ApiClient,
    store: SecureStore,
    app_handle: AppHandle,
    telemetry_state: SharedTelemetryState,
    mode_rx: watch::Receiver<TelemetryMode>,
    mode_tx: watch::Sender<TelemetryMode>,
    manual_mode_override_rx: watch::Receiver<Option<TelemetryMode>>,
    batch: Vec<DecisionRecord>,
    collector: TelemetryCollector,
    last_sample: Option<TelemetrySample>,
    policy: Option<AgentPolicyBundle>,
    last_local_action_at: Option<i64>,
    last_local_action_by_name: HashMap<String, i64>,
    backend_backoff_until: Option<i64>,
    batch_backoff_until: Option<i64>,
    backend_failure_count: u32,
    last_realtime_push_at: Option<i64>,
    last_realtime_status_at: Option<i64>,
    coalesced_realtime_samples: u32,
}

impl TelemetryEngine {
    async fn run(mut self) {
        let mut dashboard_sample_tick = interval(self.config.dashboard_sample_interval);
        dashboard_sample_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut normal_sample_tick = interval(self.config.normal_sample_interval);
        normal_sample_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut batch_flush_tick = interval(self.config.batch_flush_interval);
        batch_flush_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        batch_flush_tick.tick().await;

        let mut realtime_push_tick = interval(self.config.realtime_push_interval);
        realtime_push_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut realtime_status_tick = interval(self.config.realtime_status_poll_interval);
        realtime_status_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut command_poll_tick = interval(self.config.command_poll_interval);
        command_poll_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut policy_refresh_tick = interval(self.config.policy_refresh_interval);
        policy_refresh_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = dashboard_sample_tick.tick() => {
                    if *self.mode_rx.borrow() == TelemetryMode::Normal {
                        self.collect_local_sample().await;
                    }
                }
                _ = normal_sample_tick.tick() => {
                    if *self.mode_rx.borrow() == TelemetryMode::Normal {
                        let sample = self.latest_or_collect().await;
                        self.batch.push(sample.into_decision("normal_observation"));
                    }
                }
                _ = batch_flush_tick.tick() => {
                    self.flush_batch().await;
                }
                _ = realtime_push_tick.tick() => {
                    if *self.mode_rx.borrow() == TelemetryMode::Realtime {
                        self.push_realtime_sample().await;
                    }
                }
                _ = realtime_status_tick.tick() => {
                    self.refresh_realtime_mode().await;
                }
                _ = command_poll_tick.tick() => {
                    self.poll_commands().await;
                }
                _ = policy_refresh_tick.tick() => {
                    self.refresh_agent_policy().await;
                }
                changed = self.mode_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    }

    async fn refresh_realtime_mode(&mut self) {
        if self.manual_mode_override_rx.borrow().is_some() {
            return;
        }

        if self.backend_in_backoff() {
            return;
        }

        let now = chrono::Utc::now().timestamp();
        if latency_sensitive_session_active()
            && self
                .last_realtime_status_at
                .is_some_and(|last| now.saturating_sub(last) < 15)
        {
            return;
        }
        self.last_realtime_status_at = Some(now);

        let Some((access_token, hw_id, _hw_secret)) = self.credentials() else {
            return;
        };

        match self.api.realtime_status(&access_token, hw_id).await {
            Ok(status) if status.active => {
                self.record_backend_success();
                let _ = self.mode_tx.send(TelemetryMode::Realtime);
            }
            Ok(_) => {
                self.record_backend_success();
                let _ = self.mode_tx.send(TelemetryMode::Normal);
            }
            Err(error) => {
                if self.clear_local_session_if_device_inactive(&error) {
                    return;
                }
                self.record_backend_failure("consultar modo realtime", &error);
            }
        }
    }

    async fn push_realtime_sample(&mut self) {
        let local_realtime =
            *self.manual_mode_override_rx.borrow() == Some(TelemetryMode::Realtime);
        let sample = self.collector.collect();
        self.publish_sample(&sample).await;

        if local_realtime || self.backend_in_backoff() {
            return;
        }

        let now = chrono::Utc::now().timestamp();
        let latency_session = latency_sensitive_session_active();
        if latency_session
            && self
                .last_realtime_push_at
                .is_some_and(|last| now.saturating_sub(last) < 5)
        {
            self.coalesced_realtime_samples = self.coalesced_realtime_samples.saturating_add(1);
            return;
        }

        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            return;
        };
        let privacy = self.telemetry_privacy_policy();
        let coalesced_samples = std::mem::take(&mut self.coalesced_realtime_samples);
        let mut details = backend_sample_details(&sample, privacy);
        if latency_session || coalesced_samples > 0 {
            let throttle = json!({
                "active": latency_session,
                "coalescedSamples": coalesced_samples,
                "pushIntervalSeconds": if latency_session { 5 } else { 1 },
                "reason": "latency_sensitive_session",
            });
            if let Some(object) = details.as_object_mut() {
                object.insert("agent_self_throttle".to_string(), throttle);
            } else {
                details = json!({ "agent_self_throttle": throttle });
            }
        }
        let payload = RealtimeTelemetryPayload {
            hw_id,
            app_version: self.config.app_version.clone(),
            event_timestamp: sample.event_timestamp,
            cpu_usage: sample.cpu_usage,
            gpu_usage: sample.gpu_usage,
            gpu_name: sample.gpu_name.clone(),
            vram_gb: sample.vram_gb,
            ram_usage_mb: sample.ram_usage_mb,
            context_state: backend_sample_context(&sample, privacy),
            details,
            timestamp: now,
            nonce: nonce(),
        };

        match self
            .api
            .push_realtime(&access_token, &hw_secret, &payload)
            .await
        {
            Ok(status) if !status.active && self.manual_mode_override_rx.borrow().is_none() => {
                self.record_backend_success();
                self.last_realtime_push_at = Some(now);
                let _ = self.mode_tx.send(TelemetryMode::Normal);
            }
            Ok(_) => {
                self.record_backend_success();
                self.last_realtime_push_at = Some(now);
            }
            Err(error) => {
                self.coalesced_realtime_samples = self
                    .coalesced_realtime_samples
                    .saturating_add(coalesced_samples);
                if self.clear_local_session_if_device_inactive(&error) {
                    return;
                }
                self.record_backend_failure("enviar telemetria realtime efemera", &error);
            }
        }
    }

    async fn flush_batch(&mut self) {
        if self.batch.is_empty() {
            return;
        }

        if self.batch_in_backoff() {
            return;
        }

        if self.backend_in_backoff() {
            return;
        }

        if optimizations::focus::should_delay_non_critical_uploads() {
            return;
        }

        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            return;
        };

        let decisions = std::mem::take(&mut self.batch);
        let privacy = self.telemetry_privacy_policy();
        let hourly_summary =
            minimize_decision_for_backend(hourly_summary_decision(&decisions), privacy);
        let batch = TelemetryBatch {
            hw_id,
            app_version: self.config.app_version.clone(),
            decisions: vec![hourly_summary],
            timestamp: chrono::Utc::now().timestamp(),
            nonce: nonce(),
        };

        if let Err(error) = self.api.post_batch(&access_token, &hw_secret, &batch).await {
            if self.clear_local_session_if_device_inactive(&error) {
                return;
            }
            if is_batch_cooldown_error(&error) {
                self.record_batch_cooldown(&error);
            } else {
                self.record_backend_failure("enviar lote de telemetria", &error);
            }
            self.batch = decisions;
        } else {
            self.record_backend_success();
        }
    }

    async fn refresh_agent_policy(&mut self) {
        if self.backend_in_backoff() {
            return;
        }

        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            self.policy = None;
            return;
        };

        match self
            .api
            .agent_policy(&access_token, hw_id, &hw_secret)
            .await
        {
            Ok(policy) => {
                self.record_backend_success();
                self.policy = Some(policy);
            }
            Err(error) => {
                if self.clear_local_session_if_device_inactive(&error) {
                    return;
                }
                self.record_backend_failure("atualizar policy bundle local", &error);
            }
        }
    }

    async fn collect_local_sample(&mut self) {
        if let Some(previous) = self.last_sample.as_ref() {
            let now = chrono::Utc::now().timestamp();
            if optimizations::focus::visual_polling_min_interval_seconds().is_some_and(|minimum| {
                now.saturating_sub(previous.event_timestamp) < minimum as i64
            }) {
                return;
            }
            if previous.idle_seconds >= 300 && now.saturating_sub(previous.event_timestamp) < 8 {
                return;
            }
        }

        let sample = self.collector.collect();
        self.publish_sample(&sample).await;
    }

    async fn latest_or_collect(&mut self) -> TelemetrySample {
        if let Some(sample) = self.last_sample.clone() {
            return sample;
        }

        let sample = self.collector.collect();
        self.publish_sample(&sample).await;
        sample
    }

    async fn publish_sample(&mut self, sample: &TelemetrySample) {
        let mode = self.mode_rx.borrow().as_str().to_string();
        let snapshot =
            TelemetryDashboardSnapshot::from_sample(sample, &mode, self.credentials().is_some());
        self.last_sample = Some(sample.clone());

        {
            let mut state = self.telemetry_state.write().await;
            *state = Some(snapshot.clone());
        }

        if let Err(error) = self.app_handle.emit(TELEMETRY_UPDATE_EVENT, snapshot) {
            eprintln!("Falha ao emitir telemetria local: {error}");
        }
    }

    async fn poll_commands(&mut self) {
        if self.backend_in_backoff() {
            return;
        }

        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            return;
        };

        if !self.has_usable_policy() {
            self.refresh_agent_policy().await;
        }

        let commands = match self.api.next_commands(&access_token, hw_id).await {
            Ok(commands) => commands,
            Err(error) => {
                if self.clear_local_session_if_device_inactive(&error) {
                    return;
                }
                self.record_backend_failure("buscar comandos", &error);
                return;
            }
        };
        self.record_backend_success();

        if commands.is_empty() {
            self.run_local_policy_fallback(&access_token, &hw_id, &hw_secret)
                .await;
            return;
        }

        for command in commands {
            let allowed_actions = self.usable_allowed_actions();
            let before = self.latest_or_collect().await;
            let (local_confirmation, execution_mode) =
                Self::remote_command_execution_mode(self.app_handle.clone(), &command).await;
            let execution = if command.hw_id != hw_id {
                optimizations::ExecutionResult::rejected(
                    &command.action_name,
                    "device_mismatch",
                    json!({
                        "command_hw_id": command.hw_id,
                        "local_hw_id": hw_id,
                    }),
                )
            } else if command.requires_confirmation && !local_confirmation {
                optimizations::ExecutionResult::rejected(
                    &command.action_name,
                    "local_confirmation_declined",
                    json!({
                        "command_id": command.id,
                        "authorization_id": command.authorization_id,
                        "context_key": command.context_key,
                    }),
                )
            } else {
                optimizations::execute_command_checked(
                    CommandSource::RemoteCommand,
                    &command.action_name,
                    command.action_payload.clone(),
                    allowed_actions.as_deref(),
                    local_confirmation,
                )
                .await
            };
            sleep(self.config.post_optimization_measurement_delay).await;
            let after = self.collector.collect();
            self.publish_sample(&after).await;

            let details = json!({
                "agent": "analystblaze-desktop",
                "message": execution.message,
                "data": execution.details,
            });
            let privacy = self.telemetry_privacy_policy();
            let backend_details = minimize_backend_value(&details, privacy);

            let event_payload =
                Self::agent_optimization_event_payload(AgentOptimizationEventInput {
                    hw_id,
                    app_version: self.config.app_version.clone(),
                    action_name: &command.action_name,
                    command_id: Some(command.id),
                    before: &before,
                    after: &after,
                    success: execution.success,
                    execution_details: backend_details.clone(),
                    privacy,
                });

            if let Err(error) = self
                .api
                .post_agent_event(&access_token, &hw_secret, &event_payload)
                .await
            {
                self.record_backend_failure("registrar evento de otimizacao", &error);
            }

            if let Err(error) = self
                .api
                .acknowledge_command(
                    &access_token,
                    CommandAcknowledgement {
                        command_id: command.id,
                        success: execution.success,
                        details: backend_details,
                        confirmed_locally: local_confirmation,
                        authorization_id: command.authorization_id.clone(),
                        context_key: command.context_key.clone(),
                        execution_mode: Some(execution_mode.to_string()),
                    },
                )
                .await
            {
                self.record_backend_failure(&format!("confirmar comando {}", command.id), &error);
            }
        }
    }

    async fn run_local_policy_fallback(
        &mut self,
        access_token: &str,
        hw_id: &Uuid,
        hw_secret: &str,
    ) {
        if optimizations::focus::should_pause_heavy_scans() {
            return;
        }

        let Some(policy) = self.policy.clone().filter(policy_is_usable) else {
            return;
        };

        if !policy.permissions.local_inference
            || !policy.permissions.automatic_agent_optimization
            || !policy.server_authority
        {
            return;
        }

        let local_ai_policy = optimizations::local_ai_policy::load_local_ai_policy();
        if !local_ai_policy.enabled {
            return;
        }

        let now = chrono::Utc::now().timestamp();
        if let Some(last_action_at) = self.last_local_action_at {
            if now.saturating_sub(last_action_at) < policy.cooldowns.local_decision_seconds as i64 {
                return;
            }
        }

        let before = self.latest_or_collect().await;
        let Some(decision) = evaluate_local_policy(&policy, &local_ai_policy, &before) else {
            return;
        };

        let min_confidence = if decision.action_name == "APPLY_GAME_MODE" {
            policy
                .thresholds
                .min_confidence
                .max(local_ai_policy.game_min_confidence)
        } else {
            policy.thresholds.min_confidence
        };
        if decision.confidence < min_confidence {
            return;
        }

        let credentials = self.store.load().ok();
        if decision.action_name == "APPLY_GAME_MODE"
            && !automatic_paid_plan_allowed(credentials.as_ref())
        {
            let _ = crate::audit::record_event(
                "info",
                "local_ai.game_mode_plan_blocked",
                "Modo Gamer automatico detectado, mas o plano atual nao permite automacao.",
                json!({
                    "action_name": decision.action_name,
                    "confidence": decision.confidence,
                    "reason": decision.reason,
                    "plan": credentials.as_ref().and_then(|credentials| credentials.plan.clone()).unwrap_or_else(|| "starter".to_string()),
                    "required_plan": "pro_or_family",
                }),
            );
            return;
        }
        if decision.action_name == "EMPTY_TEMP"
            && !automatic_paid_plan_allowed(credentials.as_ref())
        {
            let _ = crate::audit::record_event(
                "info",
                "local_ai.pc_clean_plan_blocked",
                "PC Limpo automatico detectado, mas o plano atual nao permite automacao.",
                json!({
                    "action_name": decision.action_name,
                    "confidence": decision.confidence,
                    "reason": decision.reason,
                    "plan": credentials.as_ref().and_then(|credentials| credentials.plan.clone()).unwrap_or_else(|| "starter".to_string()),
                    "required_plan": "pro_or_family",
                }),
            );
            return;
        }

        if let Some(last_action_at) = self.last_local_action_by_name.get(&decision.action_name) {
            if now.saturating_sub(*last_action_at) < decision.cooldown_seconds as i64 {
                return;
            }
        }

        if decision.requires_automatic_sensitive_consent
            && !local_ai_policy.allow_automatic_sensitive_actions
        {
            let _ = crate::audit::record_event(
                "info",
                "local_ai.recommendation_only",
                "IA local detectou oportunidade, mas automacao sensivel nao esta autorizada.",
                json!({
                    "action_name": decision.action_name,
                    "confidence": decision.confidence,
                    "reason": decision.reason,
                    "required_setting": "allow_automatic_sensitive_actions",
                    "payload": decision.action_payload,
                }),
            );
            return;
        }

        let execution = optimizations::execute_command_checked(
            CommandSource::LocalPolicy,
            &decision.action_name,
            Some(decision.action_payload.clone()),
            Some(&policy.allowed_actions),
            local_ai_policy.allow_automatic_sensitive_actions,
        )
        .await;
        sleep(self.config.post_optimization_measurement_delay).await;
        let after = self.collector.collect();
        self.publish_sample(&after).await;
        self.last_local_action_at = Some(now);
        self.last_local_action_by_name
            .insert(decision.action_name.clone(), now);

        let details = json!({
            "agent": "analystblaze-desktop",
            "source": "local_policy",
            "policy_version": policy.policy_version,
            "model_version": policy.model_version,
            "confidence": decision.confidence,
            "reason": decision.reason,
            "payload": decision.action_payload,
            "message": execution.message,
            "data": execution.details,
        });
        let privacy = self.telemetry_privacy_policy();
        let backend_details = minimize_backend_value(&details, privacy);

        let event_payload = Self::agent_optimization_event_payload(AgentOptimizationEventInput {
            hw_id: *hw_id,
            app_version: self.config.app_version.clone(),
            action_name: &decision.action_name,
            command_id: None,
            before: &before,
            after: &after,
            success: execution.success,
            execution_details: backend_details,
            privacy,
        });

        if let Err(error) = self
            .api
            .post_agent_event(access_token, hw_secret, &event_payload)
            .await
        {
            if self.clear_local_session_if_device_inactive(&error) {
                return;
            }
            if is_request_validation_error(&error) {
                eprintln!("Backend rejeitou o registro da decisao local do agente: {error}");
            } else {
                self.record_backend_failure("registrar decisao local do agente", &error);
            }
        }
    }

    async fn remote_command_execution_mode(
        app_handle: AppHandle,
        command: &CommandResponse,
    ) -> (bool, &'static str) {
        let learned_auto = command.authorization_mode.as_deref() == Some("learned_auto")
            && !command.requires_confirmation;
        if learned_auto {
            let policy = optimizations::local_ai_policy::load_local_ai_policy();
            if policy.enabled && policy.allow_automatic_sensitive_actions {
                return (true, "learned_auto");
            }
        }

        if command.requires_confirmation || command.authorization_mode.is_some() {
            let approved = Self::request_remote_command_confirmation(app_handle, command).await;
            return (approved, "manual_confirmed");
        }

        (false, "remote_command")
    }

    async fn request_remote_command_confirmation(
        app_handle: AppHandle,
        command: &CommandResponse,
    ) -> bool {
        let request_id = Uuid::new_v4();
        let (sender, receiver) = oneshot::channel();
        let pending =
            PENDING_REMOTE_COMMAND_CONFIRMATIONS.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut pending) = pending.lock() {
            pending.insert(request_id, sender);
        } else {
            return false;
        }

        let payload = RemoteCommandConfirmationRequest {
            request_id,
            command_id: command.id,
            action_name: command.action_name.clone(),
            title: command
                .confirmation_prompt
                .clone()
                .unwrap_or_else(|| command.action_name.replace('_', " ")),
            description: "O dashboard autorizou esta acao. Confirme neste PC para permitir que a IA aprenda este contexto.".to_string(),
            risk: command
                .risk_level
                .clone()
                .unwrap_or_else(|| "sensitive".to_string()),
            snapshot: true,
            authorization_mode: command.authorization_mode.clone(),
            authorization_id: command.authorization_id.clone(),
            context_key: command.context_key.clone(),
        };

        focus_main_window_for_confirmation(&app_handle);
        if app_handle
            .emit(REMOTE_COMMAND_CONFIRMATION_EVENT, payload)
            .is_err()
        {
            let _ = resolve_remote_command_confirmation(request_id, false);
            return false;
        }

        match timeout(REMOTE_COMMAND_CONFIRMATION_TIMEOUT, receiver).await {
            Ok(Ok(approved)) => approved,
            _ => {
                let _ = resolve_remote_command_confirmation(request_id, false);
                false
            }
        }
    }

    fn backend_in_backoff(&self) -> bool {
        self.backend_backoff_until
            .is_some_and(|until| chrono::Utc::now().timestamp() < until)
    }

    fn batch_in_backoff(&self) -> bool {
        self.batch_backoff_until
            .is_some_and(|until| chrono::Utc::now().timestamp() < until)
    }

    fn record_backend_success(&mut self) {
        self.backend_failure_count = 0;
        self.backend_backoff_until = None;
    }

    fn record_backend_failure(&mut self, operation: &str, error: &str) {
        self.backend_failure_count = self.backend_failure_count.saturating_add(1);
        let delay_seconds = backend_backoff_seconds(self.backend_failure_count);
        self.backend_backoff_until = Some(chrono::Utc::now().timestamp() + delay_seconds);

        if self.backend_failure_count == 1 || self.backend_failure_count.is_power_of_two() {
            eprintln!(
                "Backend indisponivel ao {operation}: {error}. Nova tentativa em {delay_seconds}s."
            );
        }
    }

    fn record_batch_cooldown(&mut self, error: &str) {
        self.batch_backoff_until = Some(
            chrono::Utc::now().timestamp() + self.config.batch_flush_interval.as_secs() as i64,
        );
        eprintln!(
            "Servidor recusou lote por cooldown horario: {error}. O lote sera mantido e reenviado na proxima janela."
        );
    }

    fn has_usable_policy(&self) -> bool {
        self.policy.as_ref().is_some_and(policy_is_usable)
    }

    fn usable_allowed_actions(&self) -> Option<Vec<String>> {
        self.policy
            .as_ref()
            .filter(|policy| policy_is_usable(policy))
            .map(|policy| policy.allowed_actions.clone())
    }

    fn agent_optimization_event_payload(
        input: AgentOptimizationEventInput<'_>,
    ) -> AgentOptimizationEventPayload {
        AgentOptimizationEventPayload {
            hw_id: input.hw_id,
            app_version: input.app_version,
            action_name: input.action_name.to_string(),
            command_id: input.command_id,
            event_timestamp: input.after.event_timestamp,
            context_state: json!({
                "before": compact_sample_context(input.before, input.privacy),
                "after": compact_sample_context(input.after, input.privacy),
                "active_window_changed": input.before.active_window != input.after.active_window,
            }),
            before_metrics: OptimizationMetrics::from(input.before),
            after_metrics: OptimizationMetrics::from(input.after),
            delta_metrics: optimization_delta(input.before, input.after),
            execution_details: bounded_private_agent_event_value(
                &input.execution_details,
                input.privacy,
            ),
            success: input.success,
            timestamp: chrono::Utc::now().timestamp(),
            nonce: nonce(),
        }
    }

    fn credentials(&self) -> Option<(String, Uuid, String)> {
        let credentials = self.store.load().ok()?;
        Some((
            credentials.access_token?,
            credentials.hw_id?,
            credentials.hw_secret?,
        ))
    }

    fn telemetry_privacy_policy(&self) -> TelemetryPrivacyPolicy {
        let credentials = self.store.load().ok();
        TelemetryPrivacyPolicy::from_config_and_credentials(&self.config, credentials.as_ref())
    }

    fn clear_local_session_if_device_inactive(&mut self, error: &str) -> bool {
        if !is_device_inactive_error(error) {
            return false;
        }

        eprintln!("Sessao desktop desativada pelo servidor: {error}");
        let _ = self.store.clear();
        let _ = self.mode_tx.send(TelemetryMode::Normal);
        self.last_sample = None;
        if let Ok(mut state) = self.telemetry_state.try_write() {
            *state = None;
        }
        let _ = self.app_handle.emit(AGENT_SESSION_INVALIDATED_EVENT, ());
        true
    }
}

impl TelemetrySample {
    fn into_decision(self, action_taken: &str) -> DecisionRecord {
        DecisionRecord {
            event_timestamp: self.event_timestamp,
            cpu_usage: self.cpu_usage,
            gpu_usage: self.gpu_usage,
            gpu_name: self.gpu_name,
            vram_gb: self.vram_gb,
            ram_usage_mb: self.ram_usage_mb,
            context_state: self.context_state,
            action_taken: action_taken.to_string(),
            details: self.details,
            score_delta: 0.0,
        }
    }
}

fn nonce() -> String {
    Uuid::new_v4().simple().to_string()
}

fn hourly_summary_decision(decisions: &[DecisionRecord]) -> DecisionRecord {
    let first_timestamp = decisions
        .first()
        .map(|decision| decision.event_timestamp)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    let last_timestamp = decisions
        .last()
        .map(|decision| decision.event_timestamp)
        .unwrap_or(first_timestamp);
    let last = decisions.last();

    let cpu = metric_summary(decisions.iter().map(|decision| decision.cpu_usage));
    let gpu = metric_summary(decisions.iter().map(|decision| decision.gpu_usage));
    let ram = metric_summary(decisions.iter().map(|decision| decision.ram_usage_mb));
    let ram_percent = details_metric_summary(decisions, "ram_usage_percent");
    let cpu_temp = details_metric_summary(decisions, "cpu_temperature");
    let gpu_temp = details_metric_summary(decisions, "gpu_temperature");
    let disk = details_metric_summary(decisions, "disk_usage_percent");
    let latency = details_metric_summary(decisions, "latency_ms");
    let vram = details_metric_summary(decisions, "vram_usage_percent");
    let processes = details_metric_summary(decisions, "active_processes");

    let cpu_avg = summary_avg(&cpu);
    let gpu_avg = summary_avg(&gpu);
    let ram_avg = summary_avg(&ram);

    DecisionRecord {
        event_timestamp: last_timestamp,
        cpu_usage: cpu_avg,
        gpu_usage: gpu_avg,
        gpu_name: last
            .map(|decision| decision.gpu_name.clone())
            .unwrap_or_default(),
        vram_gb: last.map(|decision| decision.vram_gb).unwrap_or_default(),
        ram_usage_mb: ram_avg,
        context_state: json!({
            "aggregation": "hourly",
            "bucket_start": first_timestamp,
            "bucket_end": last_timestamp,
            "sample_count": decisions.len(),
            "last_context": last.map(|decision| decision.context_state.clone()),
        }),
        action_taken: "hourly_telemetry_summary".to_string(),
        details: json!({
            "aggregation": {
                "kind": "hourly_min_avg_max",
                "storage_policy": "persist_only_hourly_summary",
                "sample_count": decisions.len(),
                "bucket_start": first_timestamp,
                "bucket_end": last_timestamp,
            },
            "hourly_summary": {
                "cpu_usage": cpu,
                "gpu_usage": gpu,
                "ram_usage_mb": ram,
                "ram_usage_percent": ram_percent,
                "cpu_temperature": cpu_temp,
                "gpu_temperature": gpu_temp,
                "disk_usage_percent": disk,
                "latency_ms": latency,
                "vram_usage_percent": vram,
                "active_processes": processes,
            },
            "last_sample_details": last.map(|decision| decision.details.clone()),
        }),
        score_delta: 0.0,
    }
}

fn metric_summary(values: impl Iterator<Item = f64>) -> Value {
    let values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return Value::Null;
    }

    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let avg = values.iter().sum::<f64>() / values.len() as f64;
    json!({
        "min": round_metric(min),
        "avg": round_metric(avg),
        "max": round_metric(max),
    })
}

fn details_metric_summary(decisions: &[DecisionRecord], key: &str) -> Value {
    metric_summary(
        decisions
            .iter()
            .filter_map(|decision| decision.details.get(key).and_then(Value::as_f64)),
    )
}

fn summary_avg(summary: &Value) -> f64 {
    summary
        .get("avg")
        .and_then(Value::as_f64)
        .unwrap_or_default()
}

fn round_metric(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

impl From<&TelemetrySample> for OptimizationMetrics {
    fn from(sample: &TelemetrySample) -> Self {
        Self {
            cpu_usage: sample.cpu_usage,
            gpu_usage: sample.gpu_usage,
            ram_usage_mb: sample.ram_usage_mb,
            ram_usage_percent: sample.ram_usage_percent,
            cpu_temperature: sample.cpu_temperature,
            cpu_temperature_available: sample.cpu_temperature_available,
            gpu_temperature: sample.gpu_temperature,
            gpu_temperature_available: sample.gpu_temperature_available,
            vram_usage_percent: sample.vram_usage_percent,
            disk_usage_percent: sample.disk_usage_percent,
            active_processes: sample.active_processes,
            idle_seconds: sample.idle_seconds,
            latency_ms: sample.latency_ms,
        }
    }
}

fn optimization_delta(before: &TelemetrySample, after: &TelemetrySample) -> Value {
    json!({
        "cpu_usage": after.cpu_usage - before.cpu_usage,
        "gpu_usage": after.gpu_usage - before.gpu_usage,
        "ram_usage_mb": after.ram_usage_mb - before.ram_usage_mb,
        "ram_usage_percent": after.ram_usage_percent - before.ram_usage_percent,
        "cpu_temperature": after.cpu_temperature - before.cpu_temperature,
        "gpu_temperature": after.gpu_temperature - before.gpu_temperature,
        "disk_usage_percent": after.disk_usage_percent - before.disk_usage_percent,
        "active_processes": after.active_processes as i64 - before.active_processes as i64,
        "idle_seconds": after.idle_seconds as i64 - before.idle_seconds as i64,
        "latency_ms": after.latency_ms - before.latency_ms,
    })
}

fn is_device_inactive_error(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("not active")
        || normalized.contains("inactive")
        || normalized.contains("nao esta ativo")
        || normalized.contains("no esta activo")
        || normalized.contains("hardware not found")
        || normalized.contains("invalid hardware")
        || normalized.contains("user not found")
        || normalized.contains("usuario nao encontrado")
        || normalized.contains("usuário não encontrado")
        || normalized.contains("invalid access token")
        || normalized.contains("invalid_access_token")
}

fn is_batch_cooldown_error(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("cooldown period is active")
        || normalized.contains("cooldown_period_active")
        || normalized.contains("periodo de cooldown")
        || normalized.contains("período de cooldown")
}

fn is_request_validation_error(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("status 400")
        || normalized.contains("status 422")
        || normalized.contains("check the submitted data")
        || normalized.contains("dados enviados")
}

fn latency_sensitive_session_active() -> bool {
    optimizations::latency::active_latency_session().is_some()
        || optimizations::active_game_mode_session().is_some()
        || optimizations::focus::active_focus_session().is_some()
}

fn backend_backoff_seconds(failure_count: u32) -> i64 {
    let exponent = failure_count.saturating_sub(1).min(6);
    (15_i64 * 2_i64.pow(exponent)).min(300)
}

fn minimize_decision_for_backend(
    mut decision: DecisionRecord,
    privacy: TelemetryPrivacyPolicy,
) -> DecisionRecord {
    decision.context_state = minimize_backend_value(&decision.context_state, privacy);
    decision.details = minimize_backend_value(&decision.details, privacy);
    decision
}

fn backend_sample_context(sample: &TelemetrySample, privacy: TelemetryPrivacyPolicy) -> Value {
    compact_sample_context(sample, privacy)
}

fn backend_sample_details(sample: &TelemetrySample, privacy: TelemetryPrivacyPolicy) -> Value {
    let mut details = json!({
        "privacy": privacy_metadata(privacy),
        "gpu_name": sample.gpu_name,
        "vram_gb": sample.vram_gb,
        "vram_used_gb": sample.vram_used_gb,
        "vram_usage_percent": sample.vram_usage_percent,
        "cpu_temperature": sample.cpu_temperature,
        "cpu_temperature_available": sample.cpu_temperature_available,
        "cpu_temperature_source": sample.cpu_temperature_source,
        "cpu_temperature_methods": sample.cpu_temperature_methods,
        "ram_total_mb": sample.ram_total_mb,
        "ram_usage_percent": sample.ram_usage_percent,
        "gpu_temperature": sample.gpu_temperature,
        "gpu_temperature_available": sample.gpu_temperature_available,
        "gpu_temperature_source": sample.gpu_temperature_source,
        "gpu_temperature_methods": sample.gpu_temperature_methods,
        "thermal_sensors": sensor_backend_summary(&sample.thermal_sensors, 24),
        "power_sensors": sensor_backend_summary(&sample.power_sensors, 18),
        "fan_sensors": sensor_backend_summary(&sample.fan_sensors, 12),
        "thermal_state": sample.thermal_state,
        "thermal_trend": sample.thermal_trend,
        "throttling_suspected": sample.throttling_suspected,
        "watts": sample.watts,
        "cpu_watts": sample.cpu_watts,
        "gpu_watts": sample.gpu_watts,
        "estimated_kwh": sample.watts.map(|watts| watts / 1000.0),
        "energy_confidence": sample.energy_confidence,
        "is_estimated": sample.energy_is_estimated,
        "energy_source": sample.energy_source,
        "power_profile": sample.power_profile,
        "gpu_usage_available": sample.gpu_usage_available,
        "disk_used_gb": sample.disk_used_gb,
        "disk_total_gb": sample.disk_total_gb,
        "disk_usage_percent": sample.disk_usage_percent,
        "active_processes": sample.active_processes,
        "system_uptime_seconds": sample.system_uptime_seconds,
        "idle_seconds": sample.idle_seconds,
        "latency_ms": sample.latency_ms,
        "network": backend_network_summary(&sample.network, privacy),
        "advanced": backend_advanced_summary(sample),
        "source": "sysinfo_minimized",
    });

    if privacy.detailed_process_allowed() {
        if let Some(object) = details.as_object_mut() {
            object.insert(
                "diagnostic".to_string(),
                json!({
                    "mode": "explicit",
                    "active_window": "local_only",
                    "active_window_present": sample.active_window.is_some(),
                    "process_sample_masked": sample
                        .details
                        .get("process_sample")
                        .and_then(Value::as_array)
                        .map(|values| mask_process_array(values))
                        .unwrap_or_default(),
                }),
            );
        }
    }

    details
}

fn backend_advanced_summary(sample: &TelemetrySample) -> Value {
    json!({
        "battery_percent": sample.advanced.battery_percent,
        "battery_status": sample.advanced.battery_status,
        "defender_status": sample.advanced.defender_status,
        "defender_realtime_enabled": sample.advanced.defender_realtime_enabled,
        "windows_update_reboot_pending": sample.advanced.windows_update_reboot_pending,
        "event_log_critical_errors_24h": sample.advanced.event_log_critical_errors_24h,
        "thermal_throttling_suspected": sample.advanced.thermal_throttling_suspected,
        "disk_predict_failure": sample.advanced.disk_predict_failure,
    })
}

fn sensor_backend_summary(
    sensors: &[super::collector::HardwareSensorReading],
    limit: usize,
) -> Value {
    json!(sensors
        .iter()
        .take(limit)
        .map(|sensor| {
            json!({
                "source": &sensor.source,
                "type": &sensor.sensor_type,
                "hardware_type": &sensor.hardware_type,
                "hardware_name": &sensor.hardware_name,
                "label": &sensor.label,
                "value": sensor.value,
                "unit": &sensor.unit,
            })
        })
        .collect::<Vec<_>>())
}

fn backend_network_summary(
    network: &super::network::NetworkDiagnostics,
    privacy: TelemetryPrivacyPolicy,
) -> Value {
    let dns_summary = network_dns_summary(&network.dns_servers);
    json!({
        "connected": network.connected,
        "adapter_type": network.adapter_type,
        "adapter_status": network.adapter_status,
        "link_speed": network.link_speed,
        "gateway": network.gateway.as_deref().map(network_endpoint_summary),
        "dns": dns_summary,
        "wifi_ssid": if privacy.include_ssid { json!(network.wifi_ssid) } else { Value::Null },
        "wifi_ssid_policy": if privacy.include_ssid { "included_by_local_opt_in" } else { "local_only" },
        "wifi_signal_percent": network.wifi_signal_percent,
        "wifi_radio_type": network.wifi_radio_type,
        "wifi_channel": network.wifi_channel,
        "gateway_latency_ms": network.gateway_latency_ms,
        "dns_latency_ms": network.dns_latency_ms,
        "external_latency_ms": network.external_latency_ms,
        "jitter_ms": network.jitter_ms,
        "packet_loss_percent": network.packet_loss_percent,
        "probes": network.probes.iter().map(network_probe_summary).collect::<Vec<_>>(),
        "recommendations": network.recommendations,
        "refreshed_at": network.refreshed_at,
    })
}

fn network_dns_summary(dns_servers: &[String]) -> Value {
    let mut private = 0;
    let mut public = 0;
    let mut loopback = 0;
    let mut unknown = 0;

    for server in dns_servers {
        match network_endpoint_scope(server) {
            "private" => private += 1,
            "public" => public += 1,
            "loopback" => loopback += 1,
            _ => unknown += 1,
        }
    }

    json!({
        "count": dns_servers.len(),
        "private_count": private,
        "public_count": public,
        "loopback_count": loopback,
        "unknown_count": unknown,
    })
}

fn network_endpoint_summary(value: &str) -> Value {
    json!({
        "present": !value.trim().is_empty(),
        "scope": network_endpoint_scope(value),
    })
}

fn network_probe_summary(probe: &super::network::NetworkProbe) -> Value {
    json!({
        "label": probe.label,
        "target_scope": network_endpoint_scope(&probe.target),
        "sent": probe.sent,
        "received": probe.received,
        "packet_loss_percent": probe.packet_loss_percent,
        "avg_ms": probe.avg_ms,
        "min_ms": probe.min_ms,
        "max_ms": probe.max_ms,
        "jitter_ms": probe.jitter_ms,
    })
}

fn network_endpoint_scope(value: &str) -> &'static str {
    let target = value
        .trim()
        .trim_matches(['[', ']'])
        .split('%')
        .next()
        .unwrap_or(value.trim());
    match target.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) if address.is_loopback() => "loopback",
        Ok(IpAddr::V4(address)) if ipv4_is_private_or_link_local(address) => "private",
        Ok(IpAddr::V4(_)) => "public",
        Ok(IpAddr::V6(address)) if address.is_loopback() => "loopback",
        Ok(IpAddr::V6(address)) if ipv6_is_private_or_link_local(address) => "private",
        Ok(IpAddr::V6(_)) => "public",
        Err(_) => "unknown",
    }
}

fn ipv4_is_private_or_link_local(address: Ipv4Addr) -> bool {
    address.is_private()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_documentation()
        || address.octets()[0] == 0
}

fn ipv6_is_private_or_link_local(address: Ipv6Addr) -> bool {
    address.is_unique_local()
        || address.is_unicast_link_local()
        || address.is_unspecified()
        || address.segments()[0] & 0xffc0 == 0xff80
}

fn privacy_metadata(privacy: TelemetryPrivacyPolicy) -> Value {
    json!({
        "mode": if privacy.diagnostics_enabled { "diagnostic_opt_in" } else { "minimized_default" },
        "active_window": "local_only",
        "process_detail": if privacy.detailed_process_allowed() { "masked_diagnostic" } else { "local_only" },
        "ssid": if privacy.include_ssid { "included_by_local_opt_in" } else { "local_only" },
        "hostname": if privacy.include_hostname { "included_by_local_opt_in" } else { "local_only" },
        "family_detail_consent": privacy.family_detail_consent,
    })
}

fn compact_sample_context(sample: &TelemetrySample, privacy: TelemetryPrivacyPolicy) -> Value {
    json!({
        "privacy": privacy_metadata(privacy),
        "activity": sample.context_state.pointer("/local_context/activity").cloned()
            .or_else(|| sample.context_state.get("activity").cloned())
            .unwrap_or_else(|| json!("unknown")),
        "signals": bounded_agent_event_value(
            sample.context_state.pointer("/local_context/signals").unwrap_or(&Value::Null)
        ),
        "cpu_temperature": sample.cpu_temperature,
        "cpu_temperature_available": sample.cpu_temperature_available,
        "gpu_temperature": sample.gpu_temperature,
        "gpu_temperature_available": sample.gpu_temperature_available,
        "cpu_temperature_source": sample.cpu_temperature_source.clone(),
        "gpu_temperature_source": sample.gpu_temperature_source.clone(),
        "thermal_sensor_count": sample.thermal_sensors.len(),
        "power_sensor_count": sample.power_sensors.len(),
        "fan_sensor_count": sample.fan_sensors.len(),
        "thermal_state": sample.thermal_state.clone(),
        "thermal_trend": sample.thermal_trend.clone(),
        "throttling_suspected": sample.throttling_suspected,
        "power_profile": sample.power_profile.clone(),
        "ram_usage_percent": sample.ram_usage_percent,
        "disk_usage_percent": sample.disk_usage_percent,
        "idle_seconds": sample.idle_seconds,
        "host_name": if privacy.include_hostname {
            sample.context_state.get("host_name").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        },
        "network": backend_network_summary(&sample.network, privacy),
        "advanced": {
            "battery_percent": sample.advanced.battery_percent,
            "battery_status": sample.advanced.battery_status.clone(),
            "defender_status": sample.advanced.defender_status.clone(),
            "defender_realtime_enabled": sample.advanced.defender_realtime_enabled,
            "windows_update_reboot_pending": sample.advanced.windows_update_reboot_pending,
            "event_log_critical_errors_24h": sample.advanced.event_log_critical_errors_24h,
            "thermal_throttling_suspected": sample.advanced.thermal_throttling_suspected,
        },
    })
}

fn bounded_agent_event_value(value: &Value) -> Value {
    let compact = compact_agent_event_value(value, 0);
    let size = serde_json::to_string(&compact)
        .map(|raw| raw.len())
        .unwrap_or(usize::MAX);
    if size <= AGENT_EVENT_MAX_JSON_BYTES {
        return compact;
    }

    json!({
        "truncated": true,
        "reason": "agent_event_payload_limit",
        "original_type": match value {
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::String(_) => "string",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::Null => "null",
        }
    })
}

fn bounded_private_agent_event_value(value: &Value, privacy: TelemetryPrivacyPolicy) -> Value {
    bounded_agent_event_value(&minimize_backend_value(value, privacy))
}

fn minimize_backend_value(value: &Value, privacy: TelemetryPrivacyPolicy) -> Value {
    minimize_backend_value_inner(value, privacy, None, 0)
}

fn minimize_backend_value_inner(
    value: &Value,
    privacy: TelemetryPrivacyPolicy,
    key: Option<&str>,
    depth: usize,
) -> Value {
    if depth >= 5 {
        return json!("[nested]");
    }

    let normalized_key = key.unwrap_or_default().to_ascii_lowercase();
    if normalized_key.contains("token")
        || normalized_key.contains("secret")
        || normalized_key.contains("password")
        || normalized_key.contains("signature")
        || normalized_key.contains("authorization")
    {
        return json!("[redacted]");
    }
    if normalized_key == "active_window"
        || normalized_key == "activewindow"
        || normalized_key.contains("window_title")
        || normalized_key.contains("windowtitle")
    {
        return json!("[local_only]");
    }
    if normalized_key == "host_name" || normalized_key == "hostname" {
        return if privacy.include_hostname {
            value.clone()
        } else {
            json!("[local_only]")
        };
    }
    if normalized_key == "wifi_ssid" || normalized_key == "wifissid" || normalized_key == "ssid" {
        return if privacy.include_ssid {
            value.clone()
        } else {
            json!("[local_only]")
        };
    }
    if normalized_key == "gateway" {
        return value
            .as_str()
            .map(network_endpoint_summary)
            .unwrap_or_else(|| json!({ "present": !value.is_null(), "scope": "unknown" }));
    }
    if normalized_key == "dns_servers" || normalized_key == "dnsservers" {
        let servers = value
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return network_dns_summary(&servers);
    }
    if normalized_key == "target" && value.as_str().is_some() {
        return json!("[network_target_redacted]");
    }
    if normalized_key == "process_sample" || normalized_key == "processsample" {
        return value
            .as_array()
            .map(|values| {
                if privacy.detailed_process_allowed() {
                    json!(mask_process_array(values))
                } else {
                    json!({
                        "count": values.len(),
                        "detail": "local_only",
                    })
                }
            })
            .unwrap_or_else(|| json!("[local_only]"));
    }
    if normalized_key.contains("process") || normalized_key == "exe" || normalized_key == "pid" {
        return match value {
            Value::String(raw) if privacy.detailed_process_allowed() => mask_process_name(raw),
            Value::Array(values) if privacy.detailed_process_allowed() => Value::Array(
                values
                    .iter()
                    .map(|value| minimize_backend_value_inner(value, privacy, key, depth + 1))
                    .collect(),
            ),
            Value::Object(_) => minimize_backend_object(value, privacy, depth),
            _ => json!("[local_only]"),
        };
    }

    match value {
        Value::Object(_) => minimize_backend_object(value, privacy, depth),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(30)
                .map(|value| minimize_backend_value_inner(value, privacy, None, depth + 1))
                .collect(),
        ),
        Value::String(value) => json!(value.chars().take(300).collect::<String>()),
        primitive => primitive.clone(),
    }
}

fn minimize_backend_object(value: &Value, privacy: TelemetryPrivacyPolicy, depth: usize) -> Value {
    let Some(map) = value.as_object() else {
        return value.clone();
    };

    Value::Object(
        map.iter()
            .take(30)
            .map(|(key, value)| {
                (
                    key.clone(),
                    minimize_backend_value_inner(value, privacy, Some(key), depth + 1),
                )
            })
            .collect(),
    )
}

fn mask_process_array(values: &[Value]) -> Vec<Value> {
    values
        .iter()
        .filter_map(Value::as_str)
        .take(30)
        .map(mask_process_name)
        .collect()
}

fn mask_process_name(raw: &str) -> Value {
    let normalized = raw
        .trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(raw)
        .to_ascii_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let digest = hex::encode(hasher.finalize());
    json!({
        "category": process_category(&normalized),
        "hash": &digest[..16],
    })
}

fn process_category(name: &str) -> &'static str {
    if name.contains("defender")
        || name.contains("security")
        || name.contains("antivirus")
        || name.contains("endpoint")
        || matches!(
            name,
            "lsass.exe" | "winlogon.exe" | "csrss.exe" | "services.exe" | "svchost.exe"
        )
    {
        "security_or_system"
    } else if name.contains("chrome")
        || name.contains("edge")
        || name.contains("firefox")
        || name.contains("browser")
    {
        "browser"
    } else if name.contains("steam")
        || name.contains("valorant")
        || name.contains("fortnite")
        || name.contains("game")
    {
        "gaming"
    } else if name.contains("spotify")
        || name.contains("vlc")
        || name.contains("discord")
        || name.contains("teams")
        || name.contains("zoom")
    {
        "media_or_communication"
    } else {
        "other"
    }
}

fn compact_agent_event_value(value: &Value, depth: usize) -> Value {
    if depth >= 4 {
        return json!("[nested]");
    }

    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .take(24)
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let value = if normalized.contains("token")
                        || normalized.contains("secret")
                        || normalized.contains("password")
                        || normalized.contains("signature")
                        || normalized.contains("authorization")
                    {
                        json!("[redacted]")
                    } else {
                        compact_agent_event_value(value, depth + 1)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(20)
                .map(|value| compact_agent_event_value(value, depth + 1))
                .collect(),
        ),
        Value::String(value) => json!(value.chars().take(500).collect::<String>()),
        primitive => primitive.clone(),
    }
}

fn policy_is_usable(policy: &AgentPolicyBundle) -> bool {
    chrono::DateTime::parse_from_rfc3339(&policy.expires_at)
        .map(|expires_at| expires_at.timestamp() > chrono::Utc::now().timestamp())
        .unwrap_or(false)
}

fn evaluate_local_policy(
    policy: &AgentPolicyBundle,
    local_ai_policy: &optimizations::local_ai_policy::LocalAiPolicy,
    sample: &TelemetrySample,
) -> Option<LocalPolicyDecision> {
    let activity = sample
        .context_state
        .pointer("/local_context/activity")
        .and_then(Value::as_str)
        .unwrap_or("general");
    let gaming_signal = sample
        .context_state
        .pointer("/local_context/signals/gaming")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let video_signal = sample
        .context_state
        .pointer("/local_context/signals/video")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let music_signal = sample
        .context_state
        .pointer("/local_context/signals/music")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let process_detection = optimizations::detection::detect_game_process();
    let known_game_process = process_detection.detected;
    let high_gpu = sample.gpu_usage >= policy.thresholds.high_gpu;
    let high_cpu = sample.cpu_usage >= 25.0;
    let active_window_game_hint = sample
        .active_window
        .as_deref()
        .map(window_looks_like_game)
        .unwrap_or(false);

    let gaming_detected = activity == "gaming"
        || gaming_signal
        || known_game_process
        || active_window_game_hint
        || (high_gpu && high_cpu);
    if gaming_detected
        && local_ai_policy.auto_game_mode
        && action_allowed(policy, "APPLY_GAME_MODE")
    {
        let gpu_confidence = ratio(sample.gpu_usage, policy.thresholds.high_gpu);
        let process_confidence = if known_game_process {
            process_detection.confidence * 0.24
        } else {
            0.0
        };
        let window_confidence = if active_window_game_hint { 0.10 } else { 0.0 };
        let signal_confidence = if gaming_signal || activity == "gaming" {
            0.14
        } else {
            0.0
        };
        let confidence = (0.36
            + (policy.user_weights.gaming_priority * 0.18)
            + (gpu_confidence * 0.18)
            + process_confidence
            + window_confidence
            + signal_confidence)
            .clamp(0.0, 0.98);
        return Some(LocalPolicyDecision {
            action_name: "APPLY_GAME_MODE".to_string(),
            action_payload: json!({
                "source": "local_policy",
                "activity": activity,
                "optimize_power_plan": local_ai_policy.optimize_power_plan,
                "safe_temp_cleanup": local_ai_policy.safe_temp_cleanup,
                "enter_focus_mode": local_ai_policy.reduce_background_processes,
                "optimize_process_priorities": local_ai_policy.reduce_background_processes,
                "lower_background_processes": local_ai_policy.reduce_background_processes,
                "auto_restore": local_ai_policy.auto_restore_game_mode,
                "signals": {
                    "gaming": gaming_signal,
                    "video": video_signal,
                    "music": music_signal,
                    "known_game_process": known_game_process,
                    "active_window_game_hint": active_window_game_hint,
                },
                "detected_game": process_detection,
                "cpu_usage": sample.cpu_usage,
                "gpu_usage": sample.gpu_usage,
                "confidence": confidence,
            }),
            confidence,
            reason: "Jogo ou carga grafica detectada localmente sem ordem recente do servidor."
                .to_string(),
            cooldown_seconds: local_ai_policy
                .game_cooldown_seconds
                .max(policy.cooldowns.game_mode_seconds),
            requires_automatic_sensitive_consent: true,
        });
    }

    let cleanup_candidate = sample.idle_seconds
        >= local_ai_policy
            .cleanup_min_idle_seconds
            .max(policy.thresholds.idle_seconds)
        && sample.disk_usage_percent >= local_ai_policy.cleanup_disk_threshold_percent
        && local_ai_policy.auto_pc_clean
        && local_ai_policy.safe_temp_cleanup
        && !gaming_detected
        && !video_signal
        && sample.thermal_state != "hot"
        && sample.thermal_state != "critical"
        && action_allowed(policy, "EMPTY_TEMP");
    if cleanup_candidate {
        let disk_confidence = ratio(sample.disk_usage_percent, 100.0);
        let confidence = (0.46
            + (policy.user_weights.background_cleanup_priority * 0.30)
            + (disk_confidence * 0.24))
            .clamp(0.0, 0.90);
        return Some(LocalPolicyDecision {
            action_name: "EMPTY_TEMP".to_string(),
            action_payload: json!({
                "source": "local_policy",
                "mode": "safe",
                "min_age_minutes": 60,
                "idle_seconds": sample.idle_seconds,
                "disk_usage_percent": sample.disk_usage_percent,
                "thermal_state": sample.thermal_state,
                "confidence": confidence,
            }),
            confidence,
            reason:
                "PC ocioso com uso de disco alto; limpeza temporaria segura permitida pela policy."
                    .to_string(),
            cooldown_seconds: local_ai_policy
                .pc_clean_cooldown_seconds
                .max(policy.cooldowns.cleanup_seconds),
            requires_automatic_sensitive_consent: false,
        });
    }

    let cpu_hot = sample.cpu_temperature_available
        && sample.cpu_temperature >= local_ai_policy.thermal_cpu_limit_c;
    let gpu_hot = sample.gpu_temperature_available
        && sample.gpu_temperature >= local_ai_policy.thermal_gpu_limit_c;
    if (cpu_hot || gpu_hot)
        && local_ai_policy.optimize_power_plan
        && action_allowed(policy, "SET_POWER_PLAN_BALANCED")
    {
        let confidence =
            (0.66 + policy.user_weights.thermal_protection_priority * 0.24).clamp(0.0, 0.94);
        return Some(LocalPolicyDecision {
            action_name: "SET_POWER_PLAN_BALANCED".to_string(),
            action_payload: json!({
                "source": "local_policy",
                "reason": "thermal_protection",
                "cpu_temperature": sample.cpu_temperature,
                "gpu_temperature": sample.gpu_temperature,
                "cpu_hot": cpu_hot,
                "gpu_hot": gpu_hot,
                "confidence": confidence,
            }),
            confidence,
            reason:
                "Temperatura alta detectada; perfil equilibrado reduz pressao termica com reversao."
                    .to_string(),
            cooldown_seconds: policy.cooldowns.local_decision_seconds.max(10 * 60),
            requires_automatic_sensitive_consent: true,
        });
    }

    let battery_percent = sample.advanced.battery_percent;
    if local_ai_policy.optimize_power_plan
        && action_allowed(policy, "SET_POWER_PLAN_POWER_SAVER")
        && sample
            .advanced
            .battery_percent
            .is_some_and(|percent| percent <= local_ai_policy.battery_saver_threshold_percent)
    {
        let confidence =
            (0.72 + policy.user_weights.energy_saving_priority * 0.18).clamp(0.0, 0.94);
        return Some(LocalPolicyDecision {
            action_name: "SET_POWER_PLAN_POWER_SAVER".to_string(),
            action_payload: json!({
                "source": "local_policy",
                "reason": "battery_low",
                "battery_percent": battery_percent,
                "threshold": local_ai_policy.battery_saver_threshold_percent,
                "confidence": confidence,
            }),
            confidence,
            reason: "Notebook com bateria baixa; economia de energia permitida pela policy."
                .to_string(),
            cooldown_seconds: policy.cooldowns.local_decision_seconds.max(15 * 60),
            requires_automatic_sensitive_consent: true,
        });
    }

    if gaming_detected
        && local_ai_policy.reduce_background_processes
        && sample.latency_ms >= local_ai_policy.network_latency_threshold_ms
        && action_allowed(policy, "ENTER_FOCUS_MODE")
    {
        let confidence = (0.58 + ratio(sample.latency_ms, 180.0) * 0.22).clamp(0.0, 0.88);
        return Some(LocalPolicyDecision {
            action_name: "ENTER_FOCUS_MODE".to_string(),
            action_payload: json!({
                "source": "local_policy",
                "reason": "gaming_network_latency",
                "latency_ms": sample.latency_ms,
                "threshold": local_ai_policy.network_latency_threshold_ms,
                "confidence": confidence,
            }),
            confidence,
            reason:
                "Jogo com latencia elevada; modo foco pode reduzir interferencia de segundo plano."
                    .to_string(),
            cooldown_seconds: policy.cooldowns.local_decision_seconds.max(10 * 60),
            requires_automatic_sensitive_consent: true,
        });
    }

    None
}

fn action_allowed(policy: &AgentPolicyBundle, action: &str) -> bool {
    policy
        .allowed_actions
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(action))
}

fn automatic_paid_plan_allowed(credentials: Option<&StoredCredentials>) -> bool {
    let Some(credentials) = credentials else {
        return false;
    };
    let normalized_plan = credentials
        .plan
        .as_deref()
        .unwrap_or("starter")
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_");
    let paid = credentials.has_paid_plan.unwrap_or({
        matches!(
            normalized_plan.as_str(),
            "pro" | "family" | "family_friends" | "familyfriends"
        )
    });
    paid && matches!(
        normalized_plan.as_str(),
        "pro" | "family" | "family_friends" | "familyfriends"
    )
}

fn credentials_plan_is_family(credentials: &StoredCredentials) -> bool {
    matches!(
        credentials
            .plan
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .replace(['-', ' '], "_")
            .as_str(),
        "family" | "family_friends" | "familyfriends"
    )
}

fn ratio(value: f64, baseline: f64) -> f64 {
    if baseline <= 0.0 {
        return 0.0;
    }
    (value / baseline).clamp(0.0, 1.0)
}

fn window_looks_like_game(title: &str) -> bool {
    let normalized = title.to_ascii_lowercase();
    [
        "valorant",
        "counter-strike",
        "cs2",
        "fortnite",
        "league of legends",
        "minecraft",
        "roblox",
        "apex legends",
        "rocket league",
        "dota 2",
        "overwatch",
        "elden ring",
        "call of duty",
        "warframe",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::advanced::AdvancedTelemetry;
    use crate::telemetry::network::{NetworkDiagnostics, NetworkProbe};

    fn credentials(plan: &str, has_paid_plan: Option<bool>) -> StoredCredentials {
        StoredCredentials {
            plan: Some(plan.to_string()),
            has_paid_plan,
            ..StoredCredentials::default()
        }
    }

    fn sample_with_context(context_state: Value, details: Value) -> TelemetrySample {
        TelemetrySample {
            event_timestamp: chrono::Utc::now().timestamp(),
            cpu_usage: 42.0,
            cpu_temperature: 61.0,
            cpu_temperature_available: true,
            cpu_temperature_source: Some("test".to_string()),
            cpu_temperature_methods: Vec::new(),
            gpu_usage: 31.0,
            gpu_usage_available: true,
            gpu_name: "Test GPU".to_string(),
            vram_gb: 8.0,
            vram_used_gb: Some(2.0),
            vram_usage_percent: Some(25.0),
            ram_usage_mb: 4096.0,
            ram_total_mb: 16384.0,
            ram_usage_percent: 25.0,
            gpu_temperature: 55.0,
            gpu_temperature_available: true,
            gpu_temperature_source: Some("test".to_string()),
            gpu_temperature_methods: Vec::new(),
            thermal_sensors: Vec::new(),
            power_sensors: Vec::new(),
            fan_sensors: Vec::new(),
            thermal_state: "normal".to_string(),
            thermal_trend: "stable".to_string(),
            throttling_suspected: false,
            watts: Some(90.0),
            cpu_watts: Some(45.0),
            gpu_watts: Some(35.0),
            energy_confidence: 0.7,
            energy_is_estimated: true,
            energy_source: "test".to_string(),
            power_profile: "balanced".to_string(),
            latency_ms: 18.0,
            disk_used_gb: 120.0,
            disk_total_gb: 512.0,
            disk_usage_percent: 23.0,
            active_processes: 80,
            system_uptime_seconds: 3600,
            active_window: Some("Test Window".to_string()),
            idle_seconds: 10,
            advanced: AdvancedTelemetry::default(),
            network: NetworkDiagnostics::default(),
            context_state,
            details,
        }
    }

    fn default_privacy() -> TelemetryPrivacyPolicy {
        TelemetryPrivacyPolicy {
            diagnostics_enabled: false,
            include_ssid: false,
            include_hostname: false,
            family_detail_consent: false,
            family_plan: false,
        }
    }

    fn sensitive_sample() -> TelemetrySample {
        let mut sample = sample_with_context(
            json!({
                "host_name": "VITOR-PC",
                "local_context": {
                    "activity": "general",
                    "signals": {
                        "gaming": false,
                        "video": false,
                        "music": false,
                    }
                }
            }),
            json!({
                "active_window": "Banco - Extrato familiar",
                "process_sample": ["lsass.exe", "chrome.exe", "discord.exe"],
            }),
        );
        sample.network.gateway = Some("192.168.1.1".to_string());
        sample.network.dns_servers = vec!["192.168.1.1".to_string(), "8.8.8.8".to_string()];
        sample.network.wifi_ssid = Some("Casa Familia".to_string());
        sample.network.probes = vec![NetworkProbe {
            label: "dns_google".to_string(),
            target: "8.8.8.8".to_string(),
            sent: 2,
            received: 2,
            packet_loss_percent: 0.0,
            avg_ms: Some(20.0),
            min_ms: Some(19.0),
            max_ms: Some(21.0),
            jitter_ms: Some(2.0),
        }];
        sample
    }

    #[test]
    fn paid_automation_allows_pro_and_family_plans() {
        assert!(automatic_paid_plan_allowed(Some(&credentials(
            "pro",
            Some(true),
        ))));
        assert!(automatic_paid_plan_allowed(Some(&credentials(
            "family_friends",
            Some(true),
        ))));
        assert!(automatic_paid_plan_allowed(Some(&credentials(
            "family", None,
        ))));
    }

    #[test]
    fn paid_automation_blocks_starter_and_unpaid_plans() {
        assert!(!automatic_paid_plan_allowed(Some(&credentials(
            "starter",
            Some(false),
        ))));
        assert!(!automatic_paid_plan_allowed(Some(&credentials(
            "pro",
            Some(false),
        ))));
        assert!(!automatic_paid_plan_allowed(None));
    }

    #[test]
    fn local_agent_event_payload_is_compacted_for_backend_validation() {
        let long_text = "x".repeat(40_000);
        let context_state = json!({
            "local_context": {
                "activity": "gaming",
                "signals": {
                    "gaming": true,
                    "video": false,
                    "music": false,
                    "oversized": long_text.clone(),
                }
            },
            "advanced": {
                "driver_inventory": vec![long_text.clone(); 4],
            }
        });
        let before = sample_with_context(context_state.clone(), json!({ "raw": context_state }));
        let after = sample_with_context(context_state, json!({ "raw": "after" }));
        let execution_details = json!({
            "message": "ok",
            "token": "must-not-survive",
            "data": {
                "large": "y".repeat(40_000),
            }
        });

        let payload =
            TelemetryEngine::agent_optimization_event_payload(AgentOptimizationEventInput {
                hw_id: Uuid::new_v4(),
                app_version: "security-test".to_string(),
                action_name: "APPLY_GAME_MODE",
                command_id: None,
                before: &before,
                after: &after,
                success: true,
                execution_details,
                privacy: default_privacy(),
            });

        let context_size = serde_json::to_string(&payload.context_state).unwrap().len();
        let details_size = serde_json::to_string(&payload.execution_details)
            .unwrap()
            .len();
        let details_text = payload.execution_details.to_string();

        assert!(context_size < 12_000);
        assert!(details_size <= AGENT_EVENT_MAX_JSON_BYTES);
        assert!(!details_text.contains("must-not-survive"));
        assert!(!payload
            .context_state
            .to_string()
            .contains(&"x".repeat(2_000)));
    }

    #[test]
    fn backend_sample_details_are_minimized_by_default() {
        let sample = sensitive_sample();
        let details = backend_sample_details(&sample, default_privacy());
        let text = details.to_string();

        assert!(!text.contains("Banco"));
        assert!(!text.contains("lsass"));
        assert!(!text.contains("chrome"));
        assert!(!text.contains("Casa Familia"));
        assert!(!text.contains("192.168.1.1"));
        assert!(!text.contains("8.8.8.8"));
        assert_eq!(
            details
                .pointer("/network/dns/count")
                .and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            details
                .pointer("/network/gateway/scope")
                .and_then(Value::as_str),
            Some("private")
        );
        assert_eq!(
            details
                .pointer("/privacy/active_window")
                .and_then(Value::as_str),
            Some("local_only")
        );
    }

    #[test]
    fn diagnostic_mode_masks_processes_without_window_names() {
        let sample = sensitive_sample();
        let privacy = TelemetryPrivacyPolicy {
            diagnostics_enabled: true,
            ..default_privacy()
        };
        let details = backend_sample_details(&sample, privacy);
        let text = details.to_string();

        assert!(text.contains("process_sample_masked"));
        assert!(text.contains("security_or_system"));
        assert!(!text.contains("lsass.exe"));
        assert!(!text.contains("Banco"));
    }

    #[test]
    fn family_plan_suppresses_diagnostic_process_detail_without_consent() {
        let sample = sensitive_sample();
        let privacy = TelemetryPrivacyPolicy {
            diagnostics_enabled: true,
            family_plan: true,
            family_detail_consent: false,
            ..default_privacy()
        };
        let details = backend_sample_details(&sample, privacy);

        assert!(details.get("diagnostic").is_none());
        assert_eq!(
            details
                .pointer("/privacy/process_detail")
                .and_then(Value::as_str),
            Some("local_only")
        );
    }

    #[test]
    fn ssid_and_hostname_are_only_sent_after_local_opt_in() {
        let sample = sensitive_sample();
        let privacy = TelemetryPrivacyPolicy {
            include_ssid: true,
            include_hostname: true,
            ..default_privacy()
        };
        let context = backend_sample_context(&sample, privacy);
        let details = backend_sample_details(&sample, privacy);

        assert_eq!(
            context.get("host_name").and_then(Value::as_str),
            Some("VITOR-PC")
        );
        assert_eq!(
            details
                .pointer("/network/wifi_ssid")
                .and_then(Value::as_str),
            Some("Casa Familia")
        );
    }
}
