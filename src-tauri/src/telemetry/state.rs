use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::optimizations;

use super::collector::TelemetrySample;

pub const TELEMETRY_UPDATE_EVENT: &str = "telemetry-update";
pub const AGENT_SESSION_INVALIDATED_EVENT: &str = "agent-session-invalidated";
pub const WEEKLY_AI_USAGE_EVENT: &str = "weekly-ai-usage";
pub const ANNOUNCEMENTS_EVENT: &str = "announcements-updated";

pub type SharedTelemetryState = Arc<RwLock<Option<TelemetryDashboardSnapshot>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryHealth {
    pub score: u8,
    pub level: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryDashboardSnapshot {
    pub event_timestamp: i64,
    pub cpu_usage: f64,
    pub cpu_temperature: f64,
    pub cpu_temperature_available: bool,
    pub cpu_temperature_source: Option<String>,
    pub cpu_temperature_methods: serde_json::Value,
    pub gpu_usage: f64,
    pub gpu_usage_available: bool,
    pub gpu_name: String,
    pub vram_gb: f64,
    pub vram_used_gb: Option<f64>,
    pub vram_usage_percent: Option<f64>,
    pub ram_usage_mb: f64,
    pub ram_total_mb: f64,
    pub ram_usage_percent: f64,
    pub gpu_temperature: f64,
    pub gpu_temperature_available: bool,
    pub gpu_temperature_source: Option<String>,
    pub gpu_temperature_methods: serde_json::Value,
    pub thermal_sensors: serde_json::Value,
    pub power_sensors: serde_json::Value,
    pub fan_sensors: serde_json::Value,
    pub thermal_state: String,
    pub thermal_trend: String,
    pub throttling_suspected: bool,
    pub watts: Option<f64>,
    pub cpu_watts: Option<f64>,
    pub gpu_watts: Option<f64>,
    pub estimated_kwh: Option<f64>,
    pub energy_confidence: f64,
    pub is_estimated: bool,
    pub energy_source: String,
    pub power_profile: String,
    pub latency_ms: f64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_usage_percent: f64,
    pub active_processes: usize,
    pub system_uptime_seconds: u64,
    pub active_window: Option<String>,
    pub idle_seconds: u64,
    pub advanced: serde_json::Value,
    pub network: serde_json::Value,
    pub health_score: u8,
    pub health_level: String,
    pub health_reasons: Vec<String>,
    pub optimization_status: String,
    pub active_profile: String,
    pub telemetry_mode: String,
    pub device_online: bool,
}

pub fn new_shared_telemetry_state() -> SharedTelemetryState {
    Arc::new(RwLock::new(None))
}

impl TelemetryDashboardSnapshot {
    pub fn from_sample(
        sample: &TelemetrySample,
        telemetry_mode: &str,
        device_online: bool,
    ) -> Self {
        let health = TelemetryHealth::from_sample(sample);

        Self {
            event_timestamp: sample.event_timestamp,
            cpu_usage: sample.cpu_usage,
            cpu_temperature: sample.cpu_temperature,
            cpu_temperature_available: sample.cpu_temperature_available,
            cpu_temperature_source: sample.cpu_temperature_source.clone(),
            cpu_temperature_methods: serde_json::to_value(&sample.cpu_temperature_methods)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            gpu_usage: sample.gpu_usage,
            gpu_usage_available: sample.gpu_usage_available,
            gpu_name: sample.gpu_name.clone(),
            vram_gb: sample.vram_gb,
            vram_used_gb: sample.vram_used_gb,
            vram_usage_percent: sample.vram_usage_percent,
            ram_usage_mb: sample.ram_usage_mb,
            ram_total_mb: sample.ram_total_mb,
            ram_usage_percent: sample.ram_usage_percent,
            gpu_temperature: sample.gpu_temperature,
            gpu_temperature_available: sample.gpu_temperature_available,
            gpu_temperature_source: sample.gpu_temperature_source.clone(),
            gpu_temperature_methods: serde_json::to_value(&sample.gpu_temperature_methods)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            thermal_sensors: serde_json::to_value(&sample.thermal_sensors)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            power_sensors: serde_json::to_value(&sample.power_sensors)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            fan_sensors: serde_json::to_value(&sample.fan_sensors)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            thermal_state: sample.thermal_state.clone(),
            thermal_trend: sample.thermal_trend.clone(),
            throttling_suspected: sample.throttling_suspected,
            watts: sample.watts,
            cpu_watts: sample.cpu_watts,
            gpu_watts: sample.gpu_watts,
            estimated_kwh: sample.watts.map(|watts| watts / 1000.0),
            energy_confidence: sample.energy_confidence,
            is_estimated: sample.energy_is_estimated,
            energy_source: sample.energy_source.clone(),
            power_profile: sample.power_profile.clone(),
            latency_ms: sample.latency_ms,
            disk_used_gb: sample.disk_used_gb,
            disk_total_gb: sample.disk_total_gb,
            disk_usage_percent: sample.disk_usage_percent,
            active_processes: sample.active_processes,
            system_uptime_seconds: sample.system_uptime_seconds,
            active_window: sample.active_window.clone(),
            idle_seconds: sample.idle_seconds,
            advanced: serde_json::to_value(&sample.advanced).unwrap_or(serde_json::Value::Null),
            network: serde_json::to_value(&sample.network).unwrap_or(serde_json::Value::Null),
            health_score: health.score,
            health_level: health.level,
            health_reasons: health.reasons,
            optimization_status: optimization_status(sample, device_online),
            active_profile: active_profile(sample, telemetry_mode),
            telemetry_mode: telemetry_mode.to_string(),
            device_online,
        }
    }
}

impl TelemetryHealth {
    pub fn from_sample(sample: &TelemetrySample) -> Self {
        let mut score: i32 = 100;
        let mut reasons = Vec::new();

        if sample.cpu_usage >= 90.0 {
            score -= 22;
            reasons.push("cpu_critical".to_string());
        } else if sample.cpu_usage >= 75.0 {
            score -= 10;
            reasons.push("cpu_sustained_load".to_string());
        }

        if sample.ram_usage_percent >= 92.0 {
            score -= 24;
            reasons.push("ram_pressure_critical".to_string());
        } else if sample.ram_usage_percent >= 82.0 {
            score -= 12;
            reasons.push("ram_pressure".to_string());
        }

        if sample.gpu_temperature_available && sample.gpu_temperature >= 87.0 {
            score -= 20;
            reasons.push("gpu_thermal_critical".to_string());
        } else if sample.gpu_temperature_available && sample.gpu_temperature >= 78.0 {
            score -= 10;
            reasons.push("gpu_thermal_watch".to_string());
        }

        if sample.cpu_temperature_available && sample.cpu_temperature >= 92.0 {
            score -= 20;
            reasons.push("cpu_thermal_critical".to_string());
        } else if sample.cpu_temperature_available && sample.cpu_temperature >= 82.0 {
            score -= 10;
            reasons.push("cpu_thermal_watch".to_string());
        }

        if sample.disk_usage_percent >= 95.0 {
            score -= 10;
            reasons.push("disk_space_low".to_string());
        }

        if sample.active_processes >= 280 {
            score -= 6;
            reasons.push("background_process_load".to_string());
        }

        if sample.advanced.disk_predict_failure == Some(true) {
            score -= 30;
            reasons.push("disk_smart_predict_failure".to_string());
        }

        if sample.advanced.defender_status.as_deref() == Some("disabled_or_unavailable") {
            score -= 8;
            reasons.push("defender_attention".to_string());
        }

        if sample.advanced.event_log_critical_errors_24h.unwrap_or(0) >= 10 {
            score -= 8;
            reasons.push("system_event_errors".to_string());
        }

        if sample.network.packet_loss_percent.unwrap_or_default() >= 2.0 {
            score -= 10;
            reasons.push("network_packet_loss".to_string());
        } else if sample.network.jitter_ms.unwrap_or_default() >= 20.0 {
            score -= 6;
            reasons.push("network_jitter".to_string());
        }

        if reasons.is_empty() {
            reasons.push("stable".to_string());
        }

        let score = score.clamp(0, 100) as u8;
        let level = if score >= 90 {
            "excellent"
        } else if score >= 75 {
            "good"
        } else if score >= 55 {
            "watch"
        } else {
            "critical"
        };

        Self {
            score,
            level: level.to_string(),
            reasons,
        }
    }
}

fn optimization_status(sample: &TelemetrySample, device_online: bool) -> String {
    if optimizations::focus::active_focus_session().is_some() {
        return "focus_mode".to_string();
    }
    if !device_online {
        return "local_only".to_string();
    }
    if sample.idle_seconds >= 300 {
        return "idle_monitoring".to_string();
    }
    if sample.cpu_usage >= 75.0 || sample.ram_usage_percent >= 82.0 {
        return "observing_pressure".to_string();
    }
    "monitoring".to_string()
}

fn active_profile(sample: &TelemetrySample, telemetry_mode: &str) -> String {
    if let Some(session) = optimizations::focus::active_focus_session() {
        return format!("focus_{}", session.profile);
    }
    if telemetry_mode == "realtime" {
        return "realtime".to_string();
    }
    if sample.idle_seconds >= 300 {
        return "idle".to_string();
    }
    if sample.cpu_usage >= 70.0 || sample.gpu_usage >= 70.0 {
        return "performance".to_string();
    }
    "balanced".to_string()
}
