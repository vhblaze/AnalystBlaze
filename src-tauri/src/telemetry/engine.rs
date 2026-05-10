use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use tokio::time::{interval, sleep, MissedTickBehavior};
use uuid::Uuid;

use crate::api::{AgentPolicyBundle, ApiClient};
use crate::auth::SecureStore;
use crate::config::AgentConfig;
use crate::optimizations;

use super::collector::{TelemetryCollector, TelemetrySample};
use super::state::{
    SharedTelemetryState, TelemetryDashboardSnapshot, AGENT_SESSION_INVALIDATED_EVENT,
    TELEMETRY_UPDATE_EVENT,
};

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
}

#[derive(Clone)]
pub struct TelemetryEngineHandle {
    mode_tx: watch::Sender<TelemetryMode>,
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
        let engine = TelemetryEngine {
            config,
            api,
            store,
            app_handle,
            telemetry_state,
            mode_rx,
            mode_tx: mode_tx.clone(),
            batch: Vec::with_capacity(128),
            collector: TelemetryCollector::new(),
            last_sample: None,
            policy: None,
            last_local_action_at: None,
        };

        tauri::async_runtime::spawn(async move {
            engine.run().await;
        });

        Self { mode_tx }
    }

    pub fn set_mode(&self, mode: TelemetryMode) -> Result<(), String> {
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
    batch: Vec<DecisionRecord>,
    collector: TelemetryCollector,
    last_sample: Option<TelemetrySample>,
    policy: Option<AgentPolicyBundle>,
    last_local_action_at: Option<i64>,
}

impl TelemetryEngine {
    async fn run(mut self) {
        let mut dashboard_sample_tick = interval(self.config.dashboard_sample_interval);
        dashboard_sample_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut normal_sample_tick = interval(self.config.normal_sample_interval);
        normal_sample_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut batch_flush_tick = interval(self.config.batch_flush_interval);
        batch_flush_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

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
                    self.collect_local_sample().await;
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
        let Some((access_token, hw_id, _hw_secret)) = self.credentials() else {
            return;
        };

        match self.api.realtime_status(&access_token, hw_id).await {
            Ok(status) if status.active => {
                let _ = self.mode_tx.send(TelemetryMode::Realtime);
            }
            Ok(_) => {
                let _ = self.mode_tx.send(TelemetryMode::Normal);
            }
            Err(error) => {
                if self.clear_local_session_if_device_inactive(&error) {
                    return;
                }
                eprintln!("Falha ao consultar modo realtime: {error}");
            }
        }
    }

    async fn push_realtime_sample(&mut self) {
        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            return;
        };

        let sample = self.collector.collect();
        self.publish_sample(&sample).await;
        self.batch
            .push(sample.clone().into_decision("realtime_observation"));

        let payload = RealtimeTelemetryPayload {
            hw_id,
            app_version: self.config.app_version.clone(),
            event_timestamp: sample.event_timestamp,
            cpu_usage: sample.cpu_usage,
            gpu_usage: sample.gpu_usage,
            gpu_name: sample.gpu_name,
            vram_gb: sample.vram_gb,
            ram_usage_mb: sample.ram_usage_mb,
            context_state: sample.context_state,
            details: sample.details,
            timestamp: chrono::Utc::now().timestamp(),
            nonce: nonce(),
        };

        match self
            .api
            .push_realtime(&access_token, &hw_secret, &payload)
            .await
        {
            Ok(status) if !status.active => {
                let _ = self.mode_tx.send(TelemetryMode::Normal);
            }
            Ok(_) => {}
            Err(error) => {
                if self.clear_local_session_if_device_inactive(&error) {
                    self.batch.clear();
                    return;
                }
                eprintln!("Falha ao enviar telemetria realtime: {error}");
            }
        }
    }

    async fn flush_batch(&mut self) {
        if self.batch.is_empty() {
            return;
        }

        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            return;
        };

        let batch = TelemetryBatch {
            hw_id,
            app_version: self.config.app_version.clone(),
            decisions: std::mem::take(&mut self.batch),
            timestamp: chrono::Utc::now().timestamp(),
            nonce: nonce(),
        };

        if let Err(error) = self.api.post_batch(&access_token, &hw_secret, &batch).await {
            if self.clear_local_session_if_device_inactive(&error) {
                return;
            }
            eprintln!("Falha ao enviar lote de telemetria: {error}");
            self.batch = batch.decisions;
        }
    }

    async fn refresh_agent_policy(&mut self) {
        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            self.policy = None;
            return;
        };

        match self.api.agent_policy(&access_token, hw_id, &hw_secret).await {
            Ok(policy) => {
                self.policy = Some(policy);
            }
            Err(error) => {
                if self.clear_local_session_if_device_inactive(&error) {
                    return;
                }
                eprintln!("Falha ao atualizar policy bundle local: {error}");
            }
        }
    }

    async fn collect_local_sample(&mut self) {
        if let Some(previous) = self.last_sample.as_ref() {
            let now = chrono::Utc::now().timestamp();
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
                eprintln!("Falha ao buscar comandos: {error}");
                return;
            }
        };

        if commands.is_empty() {
            self.run_local_policy_fallback(&access_token, &hw_id, &hw_secret)
                .await;
            return;
        }

        for command in commands {
            let before = self.latest_or_collect().await;
            let execution = optimizations::execute_command(
                &command.action_name,
                command.action_payload.clone(),
            )
            .await;
            sleep(self.config.post_optimization_measurement_delay).await;
            let after = self.collector.collect();
            self.publish_sample(&after).await;

            let details = json!({
                "agent": "analystblaze-desktop",
                "message": execution.message,
                "data": execution.details,
            });

            let event_payload = Self::agent_optimization_event_payload(
                hw_id,
                self.config.app_version.clone(),
                &command.action_name,
                Some(command.id),
                &before,
                &after,
                execution.success,
                details.clone(),
            );

            if let Err(error) = self
                .api
                .post_agent_event(&access_token, &hw_secret, &event_payload)
                .await
            {
                eprintln!("Falha ao registrar evento de otimizacao: {error}");
            }

            if let Err(error) = self
                .api
                .acknowledge_command(&access_token, command.id, execution.success, details)
                .await
            {
                eprintln!("Falha ao confirmar comando {}: {error}", command.id);
            }
        }
    }

    async fn run_local_policy_fallback(
        &mut self,
        access_token: &str,
        hw_id: &Uuid,
        hw_secret: &str,
    ) {
        let Some(policy) = self.policy.clone().filter(policy_is_usable) else {
            return;
        };

        if !policy.permissions.local_inference
            || !policy.permissions.automatic_agent_optimization
            || !policy.server_authority
        {
            return;
        }

        let now = chrono::Utc::now().timestamp();
        if let Some(last_action_at) = self.last_local_action_at {
            if now.saturating_sub(last_action_at) < policy.cooldowns.local_decision_seconds as i64 {
                return;
            }
        }

        let before = self.latest_or_collect().await;
        let Some(decision) = evaluate_local_policy(&policy, &before) else {
            return;
        };

        if decision.confidence < policy.thresholds.min_confidence {
            return;
        }

        let execution =
            optimizations::execute_command(&decision.action_name, Some(decision.action_payload.clone()))
                .await;
        sleep(self.config.post_optimization_measurement_delay).await;
        let after = self.collector.collect();
        self.publish_sample(&after).await;
        self.last_local_action_at = Some(now);

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

        let event_payload = Self::agent_optimization_event_payload(
            *hw_id,
            self.config.app_version.clone(),
            &decision.action_name,
            None,
            &before,
            &after,
            execution.success,
            details,
        );

        if let Err(error) = self
            .api
            .post_agent_event(access_token, hw_secret, &event_payload)
            .await
        {
            eprintln!("Falha ao registrar decisao local do agente: {error}");
        }
    }

    fn has_usable_policy(&self) -> bool {
        self.policy.as_ref().is_some_and(policy_is_usable)
    }

    fn agent_optimization_event_payload(
        hw_id: Uuid,
        app_version: String,
        action_name: &str,
        command_id: Option<Uuid>,
        before: &TelemetrySample,
        after: &TelemetrySample,
        success: bool,
        execution_details: Value,
    ) -> AgentOptimizationEventPayload {
        AgentOptimizationEventPayload {
            hw_id,
            app_version,
            action_name: action_name.to_string(),
            command_id,
            event_timestamp: after.event_timestamp,
            context_state: json!({
                "before": before.context_state,
                "after": after.context_state,
                "active_window_before": before.active_window,
                "active_window_after": after.active_window,
            }),
            before_metrics: OptimizationMetrics::from(before),
            after_metrics: OptimizationMetrics::from(after),
            delta_metrics: optimization_delta(before, after),
            execution_details,
            success,
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
}

fn policy_is_usable(policy: &AgentPolicyBundle) -> bool {
    chrono::DateTime::parse_from_rfc3339(&policy.expires_at)
        .map(|expires_at| expires_at.timestamp() > chrono::Utc::now().timestamp())
        .unwrap_or(false)
}

fn evaluate_local_policy(
    policy: &AgentPolicyBundle,
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

    let gaming_detected = activity == "gaming"
        || gaming_signal
        || (sample.gpu_usage >= policy.thresholds.high_gpu && sample.cpu_usage >= 25.0);
    if gaming_detected && action_allowed(policy, "APPLY_GAME_MODE") {
        let gpu_confidence = ratio(sample.gpu_usage, policy.thresholds.high_gpu);
        let confidence = (0.48
            + (policy.user_weights.gaming_priority * 0.34)
            + (gpu_confidence * 0.18))
            .clamp(0.0, 0.96);
        return Some(LocalPolicyDecision {
            action_name: "APPLY_GAME_MODE".to_string(),
            action_payload: json!({
                "source": "local_policy",
                "activity": activity,
                "signals": {
                    "gaming": gaming_signal,
                    "video": video_signal,
                    "music": music_signal,
                },
                "cpu_usage": sample.cpu_usage,
                "gpu_usage": sample.gpu_usage,
                "confidence": confidence,
            }),
            confidence,
            reason: "Jogo ou carga grafica detectada localmente sem ordem recente do servidor."
                .to_string(),
        });
    }

    let cleanup_candidate = sample.idle_seconds >= policy.thresholds.idle_seconds
        && sample.disk_usage_percent >= 88.0
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
                "min_age_hours": 24,
                "idle_seconds": sample.idle_seconds,
                "disk_usage_percent": sample.disk_usage_percent,
                "confidence": confidence,
            }),
            confidence,
            reason: "PC ocioso com uso de disco alto; limpeza temporaria segura permitida pela policy."
                .to_string(),
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

fn ratio(value: f64, baseline: f64) -> f64 {
    if baseline <= 0.0 {
        return 0.0;
    }
    (value / baseline).clamp(0.0, 1.0)
}
