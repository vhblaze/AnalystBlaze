use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::watch;
use tokio::time::{interval, MissedTickBehavior};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::auth::SecureStore;
use crate::config::AgentConfig;
use crate::optimizations;

use super::collector::{TelemetryCollector, TelemetrySample};

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

#[derive(Clone)]
pub struct TelemetryEngineHandle {
    mode_tx: watch::Sender<TelemetryMode>,
}

impl TelemetryEngineHandle {
    pub fn spawn(config: AgentConfig, api: ApiClient, store: SecureStore) -> Self {
        let (mode_tx, mode_rx) = watch::channel(TelemetryMode::Normal);
        let engine = TelemetryEngine {
            config,
            api,
            store,
            mode_rx,
            mode_tx: mode_tx.clone(),
            batch: Vec::with_capacity(128),
            collector: TelemetryCollector::new(),
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
    mode_rx: watch::Receiver<TelemetryMode>,
    mode_tx: watch::Sender<TelemetryMode>,
    batch: Vec<DecisionRecord>,
    collector: TelemetryCollector,
}

impl TelemetryEngine {
    async fn run(mut self) {
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

        loop {
            tokio::select! {
                _ = normal_sample_tick.tick() => {
                    if *self.mode_rx.borrow() == TelemetryMode::Normal {
                        let sample = self.collector.collect();
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
                changed = self.mode_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    }

    async fn refresh_realtime_mode(&self) {
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
                eprintln!("Falha ao consultar modo realtime: {error}");
            }
        }
    }

    async fn push_realtime_sample(&mut self) {
        let Some((access_token, hw_id, hw_secret)) = self.credentials() else {
            return;
        };

        let sample = self.collector.collect();
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
            Err(error) => eprintln!("Falha ao enviar telemetria realtime: {error}"),
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
            eprintln!("Falha ao enviar lote de telemetria: {error}");
            self.batch = batch.decisions;
        }
    }

    async fn poll_commands(&self) {
        let Some((access_token, hw_id, _hw_secret)) = self.credentials() else {
            return;
        };

        let commands = match self.api.next_commands(&access_token, hw_id).await {
            Ok(commands) => commands,
            Err(error) => {
                eprintln!("Falha ao buscar comandos: {error}");
                return;
            }
        };

        for command in commands {
            let execution = optimizations::execute_command(
                &command.action_name,
                command.action_payload.clone(),
            )
            .await;
            let details = json!({
                "agent": "analystblaze-desktop",
                "message": execution.message,
                "data": execution.details,
            });

            if let Err(error) = self
                .api
                .acknowledge_command(&access_token, command.id, execution.success, details)
                .await
            {
                eprintln!("Falha ao confirmar comando {}: {error}", command.id);
            }
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
