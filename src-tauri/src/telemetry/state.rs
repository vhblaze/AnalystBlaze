use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::collector::TelemetrySample;

pub const TELEMETRY_UPDATE_EVENT: &str = "telemetry-update";
pub const AGENT_SESSION_INVALIDATED_EVENT: &str = "agent-session-invalidated";

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
    pub latency_ms: f64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_usage_percent: f64,
    pub active_processes: usize,
    pub system_uptime_seconds: u64,
    pub active_window: Option<String>,
    pub idle_seconds: u64,
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
    pub fn from_sample(sample: &TelemetrySample, telemetry_mode: &str, device_online: bool) -> Self {
        let health = TelemetryHealth::from_sample(sample);

        Self {
            event_timestamp: sample.event_timestamp,
            cpu_usage: sample.cpu_usage,
            cpu_temperature: sample.cpu_temperature,
            cpu_temperature_available: sample.cpu_temperature_available,
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
            latency_ms: sample.latency_ms,
            disk_used_gb: sample.disk_used_gb,
            disk_total_gb: sample.disk_total_gb,
            disk_usage_percent: sample.disk_usage_percent,
            active_processes: sample.active_processes,
            system_uptime_seconds: sample.system_uptime_seconds,
            active_window: sample.active_window.clone(),
            idle_seconds: sample.idle_seconds,
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
