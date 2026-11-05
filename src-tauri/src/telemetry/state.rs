use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::optimizations;

use super::collector::TelemetrySample;

pub const TELEMETRY_UPDATE_EVENT: &str = "telemetry-update";
pub const AGENT_SESSION_INVALIDATED_EVENT: &str = "agent-session-invalidated";
pub const WEEKLY_AI_USAGE_EVENT: &str = "weekly-ai-usage";
pub const ANNOUNCEMENTS_EVENT: &str = "announcements-updated";
pub const DNS_OPTIMIZATION_SUGGESTED_EVENT: &str = "dns-optimization-suggested";

/// Payload for DNS_OPTIMIZATION_SUGGESTED_EVENT - a background benchmark
/// found a measurably faster DNS server than the one currently configured.
/// Local policy never applies this on its own (the privileged helper
/// rejects admin actions sourced from automatic policy without a human in
/// the loop, by design) - this is surfaced to the user to apply manually.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsOptimizationSuggestion {
    pub adapter_name: String,
    pub dns_server: String,
    pub dns_label: String,
    pub previous_latency_ms: Option<f64>,
    pub candidate_latency_ms: f64,
}

pub type SharedTelemetryState = Arc<RwLock<Option<TelemetryDashboardSnapshot>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryHealth {
    pub score: u8,
    pub level: String,
    pub reasons: Vec<String>,
    /// L3 (redesign D3): a conta da nota, um item por regra que tirou pontos.
    /// `score` = 100 - soma dos `points` (limitado a 0..100).
    pub breakdown: Vec<HealthDeduction>,
}

/// Uma regra da Saúde que tirou pontos nesta leitura.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthDeduction {
    /// Mesmo código de `reasons` ("ram_pressure", "cpu_critical", ...).
    pub factor: String,
    pub points: u8,
    /// O valor medido que disparou a regra, quando é um número.
    pub measured: Option<f64>,
    /// O limite da regra, na mesma unidade de `measured`.
    pub threshold: Option<f64>,
}

/// Só o que a Saúde usa de uma amostra, para a conta ser testável sozinha.
#[derive(Debug, Clone, Default)]
pub struct HealthInputs {
    pub cpu_usage: f64,
    pub ram_usage_percent: f64,
    pub gpu_temperature: Option<f64>,
    pub cpu_temperature: Option<f64>,
    pub disk_usage_percent: f64,
    pub active_processes: usize,
    pub disk_predict_failure: bool,
    pub defender_unavailable: bool,
    pub event_log_critical_errors_24h: u32,
    pub packet_loss_percent: f64,
    pub jitter_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryDashboardSnapshot {
    pub event_timestamp: i64,
    pub cpu_usage: f64,
    pub cpu_name: String,
    pub cpu_frequency_mhz: Option<f64>,
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
    /// telemetry::disk_activity::DiskActivity - null until the PDH reader
    /// has two samples to compute rates from.
    pub disk_activity: serde_json::Value,
    pub active_processes: usize,
    pub system_uptime_seconds: u64,
    pub active_window: Option<String>,
    pub idle_seconds: u64,
    pub advanced: serde_json::Value,
    pub network: serde_json::Value,
    pub health_score: u8,
    pub health_level: String,
    pub health_reasons: Vec<String>,
    /// L3: a conta da Saúde ponto a ponto (ver TelemetryHealth::breakdown).
    #[serde(default)]
    pub health_breakdown: Vec<HealthDeduction>,
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
            cpu_name: sample.cpu_name.clone(),
            cpu_frequency_mhz: sample.cpu_frequency_mhz,
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
            disk_activity: serde_json::to_value(&sample.disk_activity).unwrap_or(serde_json::Value::Null),
            active_processes: sample.active_processes,
            system_uptime_seconds: sample.system_uptime_seconds,
            active_window: sample.active_window.clone(),
            idle_seconds: sample.idle_seconds,
            advanced: serde_json::to_value(&sample.advanced).unwrap_or(serde_json::Value::Null),
            network: serde_json::to_value(&sample.network).unwrap_or(serde_json::Value::Null),
            health_score: health.score,
            health_level: health.level,
            health_reasons: health.reasons,
            health_breakdown: health.breakdown,
            optimization_status: optimization_status(sample, device_online),
            active_profile: active_profile(sample, telemetry_mode),
            telemetry_mode: telemetry_mode.to_string(),
            device_online,
        }
    }
}

impl TelemetryHealth {
    pub fn from_sample(sample: &TelemetrySample) -> Self {
        Self::from_inputs(&HealthInputs {
            cpu_usage: sample.cpu_usage,
            ram_usage_percent: sample.ram_usage_percent,
            gpu_temperature: sample.gpu_temperature_available.then_some(sample.gpu_temperature),
            cpu_temperature: sample.cpu_temperature_available.then_some(sample.cpu_temperature),
            disk_usage_percent: sample.disk_usage_percent,
            active_processes: sample.active_processes,
            disk_predict_failure: sample.advanced.disk_predict_failure == Some(true),
            defender_unavailable: sample.advanced.defender_status.as_deref() == Some("disabled_or_unavailable"),
            event_log_critical_errors_24h: sample.advanced.event_log_critical_errors_24h.unwrap_or(0),
            packet_loss_percent: sample.network.packet_loss_percent.unwrap_or_default(),
            jitter_ms: sample.network.jitter_ms.unwrap_or_default(),
        })
    }

    /// Regras fixas: cada uma tira um número exato de pontos de 100. Nada é
    /// estimado. São as mesmas regras e pesos de antes; agora cada uma fica
    /// registrada em `breakdown` para a tela mostrar a conta.
    pub fn from_inputs(inputs: &HealthInputs) -> Self {
        let mut breakdown: Vec<HealthDeduction> = Vec::new();
        let mut deduct = |factor: &str, points: u8, measured: Option<f64>, threshold: Option<f64>| {
            breakdown.push(HealthDeduction { factor: factor.to_string(), points, measured, threshold });
        };

        if inputs.cpu_usage >= 90.0 {
            deduct("cpu_critical", 22, Some(inputs.cpu_usage), Some(90.0));
        } else if inputs.cpu_usage >= 75.0 {
            deduct("cpu_sustained_load", 10, Some(inputs.cpu_usage), Some(75.0));
        }

        if inputs.ram_usage_percent >= 92.0 {
            deduct("ram_pressure_critical", 24, Some(inputs.ram_usage_percent), Some(92.0));
        } else if inputs.ram_usage_percent >= 82.0 {
            deduct("ram_pressure", 12, Some(inputs.ram_usage_percent), Some(82.0));
        }

        if let Some(gpu) = inputs.gpu_temperature {
            if gpu >= 87.0 {
                deduct("gpu_thermal_critical", 20, Some(gpu), Some(87.0));
            } else if gpu >= 78.0 {
                deduct("gpu_thermal_watch", 10, Some(gpu), Some(78.0));
            }
        }

        if let Some(cpu) = inputs.cpu_temperature {
            if cpu >= 92.0 {
                deduct("cpu_thermal_critical", 20, Some(cpu), Some(92.0));
            } else if cpu >= 82.0 {
                deduct("cpu_thermal_watch", 10, Some(cpu), Some(82.0));
            }
        }

        if inputs.disk_usage_percent >= 95.0 {
            deduct("disk_space_low", 10, Some(inputs.disk_usage_percent), Some(95.0));
        }

        if inputs.active_processes >= 280 {
            deduct("background_process_load", 6, Some(inputs.active_processes as f64), Some(280.0));
        }

        if inputs.disk_predict_failure {
            deduct("disk_smart_predict_failure", 30, None, None);
        }

        if inputs.defender_unavailable {
            deduct("defender_attention", 8, None, None);
        }

        if inputs.event_log_critical_errors_24h >= 10 {
            deduct("system_event_errors", 8, Some(inputs.event_log_critical_errors_24h as f64), Some(10.0));
        }

        if inputs.packet_loss_percent >= 2.0 {
            deduct("network_packet_loss", 10, Some(inputs.packet_loss_percent), Some(2.0));
        } else if inputs.jitter_ms >= 20.0 {
            deduct("network_jitter", 6, Some(inputs.jitter_ms), Some(20.0));
        }

        let lost: i32 = breakdown.iter().map(|item| item.points as i32).sum();
        let score = (100 - lost).clamp(0, 100) as u8;
        let level = if score >= 90 {
            "excellent"
        } else if score >= 75 {
            "good"
        } else if score >= 55 {
            "watch"
        } else {
            "critical"
        };
        let mut reasons: Vec<String> = breakdown.iter().map(|item| item.factor.clone()).collect();
        if reasons.is_empty() {
            reasons.push("stable".to_string());
        }

        Self {
            score,
            level: level.to_string(),
            reasons,
            breakdown,
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

#[cfg(test)]
mod health_tests {
    use super::*;

    #[test]
    fn calm_pc_scores_100_with_no_deductions() {
        let health = TelemetryHealth::from_inputs(&HealthInputs { cpu_usage: 20.0, ram_usage_percent: 40.0, ..Default::default() });
        assert_eq!(health.score, 100);
        assert_eq!(health.level, "excellent");
        assert!(health.breakdown.is_empty());
        assert_eq!(health.reasons, vec!["stable".to_string()]);
    }

    #[test]
    fn breakdown_explains_every_point_lost() {
        let health = TelemetryHealth::from_inputs(&HealthInputs {
            cpu_usage: 80.0,
            ram_usage_percent: 87.0,
            gpu_temperature: Some(80.0),
            jitter_ms: 25.0,
            ..Default::default()
        });
        let lost: u32 = health.breakdown.iter().map(|item| item.points as u32).sum();
        assert_eq!(health.score as u32, 100 - lost);
        // 100 - 10 (cpu) - 12 (memória) - 10 (gpu) - 6 (jitter)
        assert_eq!(health.score, 62);
        assert_eq!(
            health.breakdown[1],
            HealthDeduction { factor: "ram_pressure".to_string(), points: 12, measured: Some(87.0), threshold: Some(82.0) }
        );
        assert_eq!(health.reasons, vec!["cpu_sustained_load", "ram_pressure", "gpu_thermal_watch", "network_jitter"]);
    }

    #[test]
    fn missing_sensors_never_cost_points() {
        let health = TelemetryHealth::from_inputs(&HealthInputs { gpu_temperature: None, cpu_temperature: None, ..Default::default() });
        assert_eq!(health.score, 100);
    }

    #[test]
    fn score_floors_at_zero() {
        let health = TelemetryHealth::from_inputs(&HealthInputs {
            cpu_usage: 99.0,
            ram_usage_percent: 99.0,
            gpu_temperature: Some(95.0),
            cpu_temperature: Some(99.0),
            disk_usage_percent: 99.0,
            active_processes: 400,
            disk_predict_failure: true,
            defender_unavailable: true,
            event_log_critical_errors_24h: 50,
            packet_loss_percent: 10.0,
            ..Default::default()
        });
        assert_eq!(health.score, 0);
        assert_eq!(health.level, "critical");
    }
}
