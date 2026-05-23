use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use tokio::time::{interval, sleep, MissedTickBehavior};
use uuid::Uuid;

use crate::api::{AgentPolicyBundle, ApiClient};
use crate::auth::SecureStore;
use crate::config::AgentConfig;
use crate::optimizations::{self, safety::CommandSource};

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
    cooldown_seconds: u64,
    requires_automatic_sensitive_consent: bool,
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

        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            return;
        };
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
            Ok(status) if !status.active && self.manual_mode_override_rx.borrow().is_none() => {
                self.record_backend_success();
                let _ = self.mode_tx.send(TelemetryMode::Normal);
            }
            Ok(_) => {
                self.record_backend_success();
            }
            Err(error) => {
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

        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            return;
        };

        let decisions = std::mem::take(&mut self.batch);
        let hourly_summary = hourly_summary_decision(&decisions);
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
            let execution = if command.hw_id != hw_id {
                optimizations::ExecutionResult::rejected(
                    &command.action_name,
                    "device_mismatch",
                    json!({
                        "command_hw_id": command.hw_id,
                        "local_hw_id": hw_id,
                    }),
                )
            } else {
                optimizations::execute_command_checked(
                    CommandSource::RemoteCommand,
                    &command.action_name,
                    command.action_payload.clone(),
                    allowed_actions.as_deref(),
                    false,
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
                self.record_backend_failure("registrar evento de otimizacao", &error);
            }

            if let Err(error) = self
                .api
                .acknowledge_command(&access_token, command.id, execution.success, details)
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
            self.record_backend_failure("registrar decisao local do agente", &error);
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

fn backend_backoff_seconds(failure_count: u32) -> i64 {
    let exponent = failure_count.saturating_sub(1).min(6);
    (15_i64 * 2_i64.pow(exponent)).min(300)
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
        && local_ai_policy.safe_temp_cleanup
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
            reason:
                "PC ocioso com uso de disco alto; limpeza temporaria segura permitida pela policy."
                    .to_string(),
            cooldown_seconds: policy.cooldowns.cleanup_seconds,
            requires_automatic_sensitive_consent: true,
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
