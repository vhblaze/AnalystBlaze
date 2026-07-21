use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

use super::snapshot;
use crate::audit;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAiPolicy {
    pub enabled: bool,
    pub agent_mode: String,
    pub auto_game_mode: bool,
    pub auto_pc_clean: bool,
    pub auto_restore_game_mode: bool,
    pub optimize_power_plan: bool,
    pub safe_temp_cleanup: bool,
    pub energy_estimation_enabled: bool,
    pub thermal_analysis_enabled: bool,
    pub manage_startup_apps: bool,
    pub manage_services: bool,
    pub reduce_background_processes: bool,
    pub allow_automatic_sensitive_actions: bool,
    pub require_confirmation_for_sensitive: bool,
    pub max_risk: String,
    pub confirmed_game_apps: Vec<String>,
    pub game_min_confidence: f64,
    pub game_cooldown_seconds: u64,
    pub pc_clean_cooldown_seconds: u64,
    pub cleanup_min_idle_seconds: u64,
    pub cleanup_disk_threshold_percent: f64,
    pub thermal_cpu_limit_c: f64,
    pub thermal_gpu_limit_c: f64,
    pub battery_saver_threshold_percent: f64,
    pub network_latency_threshold_ms: f64,
    /// Minimum age before shader/thumbnail/browser cache files are eligible
    /// for cleanup - low-risk, short-lived data, so this can default low.
    pub cleanup_cache_min_age_minutes: u64,
    /// Minimum age before %TEMP%/%WINDIR%\Temp files are eligible.
    pub cleanup_temp_min_age_minutes: u64,
    /// Minimum age before crash dumps, Windows Update cache, Delivery
    /// Optimization cache and memory dumps are eligible - these are the
    /// most consequential to delete too early, so the default stays high.
    pub cleanup_system_min_age_minutes: u64,
    /// Idle time before Adaptive Optimization enters eco/power-saver mode.
    /// Distinct from cleanup_min_idle_seconds (that one gates automatic
    /// cleanup, this one gates the energy-saving action).
    pub adaptive_idle_eco_threshold_seconds: u64,
}

impl Default for LocalAiPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            agent_mode: "manual".to_string(),
            auto_game_mode: true,
            auto_pc_clean: true,
            auto_restore_game_mode: true,
            optimize_power_plan: true,
            safe_temp_cleanup: true,
            energy_estimation_enabled: true,
            thermal_analysis_enabled: true,
            manage_startup_apps: false,
            manage_services: false,
            reduce_background_processes: false,
            allow_automatic_sensitive_actions: false,
            require_confirmation_for_sensitive: true,
            max_risk: "safe".to_string(),
            confirmed_game_apps: Vec::new(),
            game_min_confidence: 0.74,
            game_cooldown_seconds: 15 * 60,
            pc_clean_cooldown_seconds: 60 * 60,
            cleanup_min_idle_seconds: 15 * 60,
            cleanup_disk_threshold_percent: 90.0,
            thermal_cpu_limit_c: 88.0,
            thermal_gpu_limit_c: 84.0,
            battery_saver_threshold_percent: 20.0,
            network_latency_threshold_ms: 100.0,
            cleanup_cache_min_age_minutes: 6 * 60,
            cleanup_temp_min_age_minutes: 60,
            cleanup_system_min_age_minutes: 24 * 60,
            adaptive_idle_eco_threshold_seconds: 10 * 60,
        }
    }
}

pub fn load_local_ai_policy() -> LocalAiPolicy {
    let path = policy_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return LocalAiPolicy::default();
    };

    serde_json::from_str::<LocalAiPolicy>(&raw)
        .map(normalize_policy)
        .unwrap_or_default()
}

pub fn save_local_ai_policy(policy: LocalAiPolicy) -> Result<LocalAiPolicy, String> {
    let policy = normalize_policy(policy);
    let path = policy_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let raw = serde_json::to_string_pretty(&policy).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())?;
    let _ = audit::record_event(
        "info",
        "local_ai.policy_saved",
        "Preferencias locais do agente de IA atualizadas.",
        json!({
            "enabled": policy.enabled,
            "agent_mode": policy.agent_mode,
            "auto_game_mode": policy.auto_game_mode,
            "auto_pc_clean": policy.auto_pc_clean,
            "auto_restore_game_mode": policy.auto_restore_game_mode,
            "optimize_power_plan": policy.optimize_power_plan,
            "safe_temp_cleanup": policy.safe_temp_cleanup,
            "energy_estimation_enabled": policy.energy_estimation_enabled,
            "thermal_analysis_enabled": policy.thermal_analysis_enabled,
            "manage_startup_apps": policy.manage_startup_apps,
            "manage_services": policy.manage_services,
            "reduce_background_processes": policy.reduce_background_processes,
            "allow_automatic_sensitive_actions": policy.allow_automatic_sensitive_actions,
            "require_confirmation_for_sensitive": policy.require_confirmation_for_sensitive,
            "max_risk": policy.max_risk,
            "confirmed_game_apps": policy.confirmed_game_apps,
            "game_min_confidence": policy.game_min_confidence,
            "game_cooldown_seconds": policy.game_cooldown_seconds,
            "pc_clean_cooldown_seconds": policy.pc_clean_cooldown_seconds,
            "cleanup_min_idle_seconds": policy.cleanup_min_idle_seconds,
            "cleanup_disk_threshold_percent": policy.cleanup_disk_threshold_percent,
            "thermal_cpu_limit_c": policy.thermal_cpu_limit_c,
            "thermal_gpu_limit_c": policy.thermal_gpu_limit_c,
            "battery_saver_threshold_percent": policy.battery_saver_threshold_percent,
            "network_latency_threshold_ms": policy.network_latency_threshold_ms,
            "cleanup_cache_min_age_minutes": policy.cleanup_cache_min_age_minutes,
            "cleanup_temp_min_age_minutes": policy.cleanup_temp_min_age_minutes,
            "cleanup_system_min_age_minutes": policy.cleanup_system_min_age_minutes,
            "adaptive_idle_eco_threshold_seconds": policy.adaptive_idle_eco_threshold_seconds,
        }),
    );
    Ok(policy)
}

fn normalize_policy(mut policy: LocalAiPolicy) -> LocalAiPolicy {
    if !matches!(policy.max_risk.as_str(), "safe" | "sensitive") {
        policy.max_risk = "safe".to_string();
    }
    if !matches!(policy.agent_mode.as_str(), "off" | "manual" | "automatic") {
        policy.agent_mode = if policy.enabled {
            "automatic".to_string()
        } else {
            "manual".to_string()
        };
    }
    policy.enabled = policy.enabled && policy.agent_mode != "off";

    // Sem helper privilegiado e sem MFA local, servicos ficam apenas como recomendacao.
    if policy.max_risk == "safe" {
        policy.manage_services = false;
    }

    policy.require_confirmation_for_sensitive = true;
    policy.confirmed_game_apps = policy
        .confirmed_game_apps
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .take(64)
        .collect();
    policy.game_min_confidence = policy.game_min_confidence.clamp(0.5, 0.98);
    policy.game_cooldown_seconds = policy.game_cooldown_seconds.clamp(60, 60 * 60 * 6);
    policy.pc_clean_cooldown_seconds = policy
        .pc_clean_cooldown_seconds
        .clamp(10 * 60, 60 * 60 * 24);
    policy.cleanup_min_idle_seconds = policy.cleanup_min_idle_seconds.clamp(60, 60 * 60 * 12);
    policy.cleanup_disk_threshold_percent = policy.cleanup_disk_threshold_percent.clamp(70.0, 99.0);
    policy.thermal_cpu_limit_c = policy.thermal_cpu_limit_c.clamp(70.0, 105.0);
    policy.thermal_gpu_limit_c = policy.thermal_gpu_limit_c.clamp(70.0, 100.0);
    policy.battery_saver_threshold_percent =
        policy.battery_saver_threshold_percent.clamp(5.0, 50.0);
    policy.network_latency_threshold_ms = policy.network_latency_threshold_ms.clamp(40.0, 500.0);
    policy.cleanup_cache_min_age_minutes = policy.cleanup_cache_min_age_minutes.clamp(10, 7 * 24 * 60);
    policy.cleanup_temp_min_age_minutes = policy.cleanup_temp_min_age_minutes.clamp(5, 7 * 24 * 60);
    policy.cleanup_system_min_age_minutes =
        policy.cleanup_system_min_age_minutes.clamp(60, 30 * 24 * 60);
    policy.adaptive_idle_eco_threshold_seconds = policy
        .adaptive_idle_eco_threshold_seconds
        .clamp(60, 3 * 60 * 60);
    policy
}

fn policy_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join("local-ai-policy.json")
}
