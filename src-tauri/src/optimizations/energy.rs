use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Command;

use super::{
    local_ai_policy,
    os_version::WindowsGeneration,
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};
use crate::process_ext::{decode_console_bytes, CommandExt};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnergyDiagnostics {
    pub active_scheme_guid: Option<String>,
    pub active_scheme_name: Option<String>,
    pub active_scheme_alias: Option<String>,
    /// Windows 11's Settings > Power "Power mode" slider, layered on top
    /// of the classic scheme above. `None` on Windows 10 or when the
    /// overlay concept doesn't apply on this machine - see os_version.rs.
    pub active_overlay_scheme_alias: Option<String>,
    pub power_source: Option<String>,
    pub battery_percent: Option<f64>,
    pub battery_status: Option<String>,
    pub battery_saver_on: Option<bool>,
    pub cpu_current_clock_mhz: Option<f64>,
    pub cpu_max_clock_mhz: Option<f64>,
    pub recommended_plan: String,
    pub recommendations: Vec<String>,
    pub refreshed_at: i64,
}

#[derive(Debug, Clone, Copy)]
struct PowerPlanTarget {
    action_name: &'static str,
    alias: &'static str,
    label: &'static str,
    success_message: &'static str,
    failure_message: &'static str,
}

const HIGH_PERFORMANCE: PowerPlanTarget = PowerPlanTarget {
    action_name: "SET_POWER_PLAN_HIGH_PERFORMANCE",
    alias: "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c",
    label: "high_performance",
    success_message: "Plano de energia de alto desempenho ativado.",
    failure_message: "Nao foi possivel ativar o plano de alto desempenho.",
};

const BALANCED: PowerPlanTarget = PowerPlanTarget {
    action_name: "SET_POWER_PLAN_BALANCED",
    alias: "381b4222-f694-41f0-9685-ff5bb260df2e",
    label: "balanced",
    success_message: "Plano de energia equilibrado ativado.",
    failure_message: "Nao foi possivel ativar o plano equilibrado.",
};

const POWER_SAVER: PowerPlanTarget = PowerPlanTarget {
    action_name: "SET_POWER_PLAN_POWER_SAVER",
    alias: "a1841308-3541-4fab-bc81-f71556f20b4a",
    label: "power_saver",
    success_message: "Plano de economia de energia ativado.",
    failure_message: "Nao foi possivel ativar o plano de economia de energia.",
};

// Windows 11's overlay scheme GUIDs (the "Power mode" slider in
// Settings > Power). Public/documented values from powercfg; unlike the
// classic scheme GUIDs above these can't be exercised on this machine, so
// set_active_overlay_scheme() is always non-fatal on failure - powercfg
// rejects an unrecognized GUID with an error rather than silently applying
// something else, so a wrong value here degrades safely instead of
// mis-setting the mode.
const OVERLAY_BEST_PERFORMANCE_GUID: &str = "ded574b5-45a0-4f42-8737-46345c09c238";
const OVERLAY_BEST_POWER_EFFICIENCY_GUID: &str = "961cc777-2547-4f9d-8174-7d86181b8a7a";

fn overlay_guid_for_label(label: &str) -> Option<&'static str> {
    match label {
        "high_performance" => Some(OVERLAY_BEST_PERFORMANCE_GUID),
        "balanced" => Some(snapshot::OVERLAY_SCHEME_BALANCED_GUID),
        "power_saver" => Some(OVERLAY_BEST_POWER_EFFICIENCY_GUID),
        _ => None,
    }
}

fn overlay_scheme_alias(guid: &str) -> Option<String> {
    let normalized = guid.trim().to_ascii_lowercase();
    if normalized == OVERLAY_BEST_PERFORMANCE_GUID {
        Some("high_performance".to_string())
    } else if normalized == snapshot::OVERLAY_SCHEME_BALANCED_GUID {
        Some("balanced".to_string())
    } else if normalized == OVERLAY_BEST_POWER_EFFICIENCY_GUID {
        Some("power_saver".to_string())
    } else {
        None
    }
}

pub fn collect_energy_diagnostics() -> EnergyDiagnostics {
    // Same threshold the user already tunes in Settings for the automatic
    // power-saver trigger (engine.rs) - these diagnostics/recommendations
    // used to have their own disconnected 25%/20% literals that ignored it.
    let battery_low_threshold_percent =
        local_ai_policy::load_local_ai_policy().battery_saver_threshold_percent;
    let active_plan = snapshot::active_power_plan().ok();
    let battery = battery_info();
    let power_source = power_source();
    let battery_saver_on = battery_saver_on();
    let (cpu_current_clock_mhz, cpu_max_clock_mhz) = cpu_clock_info();
    let active_scheme_alias = active_plan
        .as_ref()
        .and_then(|plan| scheme_alias(&plan.scheme_guid, plan.scheme_name.as_deref()));
    let active_overlay_scheme_alias =
        if super::os_version::detected().generation == WindowsGeneration::Windows11 {
            snapshot::active_overlay_scheme()
                .and_then(|overlay| overlay_scheme_alias(&overlay.scheme_guid))
        } else {
            None
        };
    let recommended_plan = recommended_plan(
        power_source.as_deref(),
        battery.percent,
        battery_saver_on,
        active_scheme_alias.as_deref(),
        battery_low_threshold_percent,
    );
    let mut diagnostics = EnergyDiagnostics {
        active_scheme_guid: active_plan.as_ref().map(|plan| plan.scheme_guid.clone()),
        active_scheme_name: active_plan.and_then(|plan| plan.scheme_name),
        active_scheme_alias,
        active_overlay_scheme_alias,
        power_source,
        battery_percent: battery.percent,
        battery_status: battery.status,
        battery_saver_on,
        cpu_current_clock_mhz,
        cpu_max_clock_mhz,
        recommended_plan,
        recommendations: Vec::new(),
        refreshed_at: chrono::Utc::now().timestamp(),
    };
    diagnostics.recommendations = energy_recommendations(&diagnostics, battery_low_threshold_percent);
    diagnostics
}

pub async fn set_high_performance(payload: Option<Value>) -> ExecutionResult {
    set_power_plan(HIGH_PERFORMANCE, payload).await
}

pub async fn set_balanced(payload: Option<Value>) -> ExecutionResult {
    set_power_plan(BALANCED, payload).await
}

pub async fn set_power_saver(payload: Option<Value>) -> ExecutionResult {
    set_power_plan(POWER_SAVER, payload).await
}

async fn set_power_plan(target: PowerPlanTarget, payload: Option<Value>) -> ExecutionResult {
    let fallback_payload = payload.clone();
    let result =
        tokio::task::spawn_blocking(move || set_power_plan_with_snapshot(target, payload)).await;

    match result {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao executar powercfg: {error}"),
            details: json!({
                "payload": fallback_payload,
                "implemented": true,
                "target_plan": target.alias,
                "target_label": target.label,
            }),
        },
    }
}

fn set_power_plan_with_snapshot(
    target: PowerPlanTarget,
    payload: Option<Value>,
) -> ExecutionResult {
    let previous_plan = snapshot::active_power_plan();
    let is_windows_11 = super::os_version::detected().generation == WindowsGeneration::Windows11;
    let previous_overlay = if is_windows_11 {
        snapshot::active_overlay_scheme()
    } else {
        None
    };

    if let Err(error) = snapshot::set_active_power_plan(target.alias) {
        return ExecutionResult {
            success: false,
            message: target.failure_message.to_string(),
            details: json!({
                "payload": payload,
                "implemented": true,
                "target_plan": target.alias,
                "target_label": target.label,
                "snapshot_available": previous_plan.is_ok(),
                "error": error,
            }),
        };
    }
    let active_after = snapshot::active_power_plan();
    let verified = active_after
        .as_ref()
        .ok()
        .and_then(|plan| scheme_alias(&plan.scheme_guid, plan.scheme_name.as_deref()))
        .as_deref()
        == Some(target.label);
    if !verified {
        return ExecutionResult {
            success: false,
            message: target.failure_message.to_string(),
            details: json!({
                "payload": payload,
                "implemented": true,
                "target_plan": target.alias,
                "target_label": target.label,
                "snapshot_available": previous_plan.is_ok(),
                "active_after": active_after.ok().map(|plan| json!({
                    "scheme_guid": plan.scheme_guid,
                    "scheme_name": plan.scheme_name,
                })),
                "error": "power_plan_verification_failed",
            }),
        };
    }

    let mut details = json!({
        "payload": payload,
        "implemented": true,
        "target_plan": target.alias,
        "target_label": target.label,
        "verified": true,
        "requires_admin": false,
        "snapshot": null,
    });

    // Windows 11's Settings > Power "Power mode" slider is a separate
    // overlay on top of the classic scheme just verified above - see
    // os_version.rs. Best-effort and non-fatal: if this fails, the classic
    // scheme change already applied and verified above still stands.
    let overlay_target_guid = if is_windows_11 {
        overlay_guid_for_label(target.label)
    } else {
        None
    };
    let mut overlay_applied = false;
    let overlay_outcome = match overlay_target_guid {
        Some(overlay_guid) => match snapshot::set_active_overlay_scheme(overlay_guid) {
            Ok(()) => {
                overlay_applied = true;
                json!({ "applied": true, "target_overlay_scheme": overlay_guid })
            }
            Err(error) => json!({ "applied": false, "error": error }),
        },
        None => Value::Null,
    };
    details["power_mode_overlay"] = overlay_outcome.clone();

    let mut entries = Vec::new();
    if let Ok(previous_plan) = &previous_plan {
        entries.push(SnapshotEntry::PowerPlan {
            previous_scheme_guid: previous_plan.scheme_guid.clone(),
            previous_scheme_name: previous_plan.scheme_name.clone(),
            target_scheme: target.alias.to_string(),
        });
    }
    if overlay_applied {
        entries.push(SnapshotEntry::PowerOverlayScheme {
            previous_overlay_scheme: previous_overlay.map(|plan| plan.scheme_guid),
            target_overlay_scheme: overlay_target_guid
                .expect("overlay_applied implies overlay_target_guid is Some")
                .to_string(),
        });
    }

    if !entries.is_empty() {
        let snapshot = OptimizationSnapshot::new(
            target.action_name,
            entries,
            json!({
                "previous_scheme_guid": previous_plan.as_ref().ok().map(|plan| plan.scheme_guid.clone()),
                "previous_scheme_name": previous_plan.ok().and_then(|plan| plan.scheme_name),
                "target_scheme": target.alias,
                "target_label": target.label,
                "power_mode_overlay": overlay_outcome,
            }),
        );

        match snapshot::save_snapshot(&snapshot) {
            Ok(()) => {
                details["snapshot"] = json!({
                    "id": snapshot.id,
                    "entries": snapshot.entries.len(),
                    "reversible": true,
                });
            }
            Err(error) => {
                let rollback = snapshot::restore_snapshot_entries(&snapshot);
                return ExecutionResult {
                    success: false,
                    message: "Plano alterado, mas o snapshot falhou; reversao imediata solicitada."
                        .to_string(),
                    details: json!({
                        "implemented": true,
                        "target_plan": target.alias,
                        "target_label": target.label,
                        "snapshot_error": error,
                        "rollback": {
                            "restored_entries": rollback.restored_entries,
                            "failed_entries": rollback.failed_entries,
                            "messages": rollback.messages,
                        },
                    }),
                };
            }
        }
    }

    ExecutionResult::ok(target.success_message, details)
}

#[derive(Debug, Clone, Default)]
struct BatteryInfo {
    percent: Option<f64>,
    status: Option<String>,
}

fn battery_info() -> BatteryInfo {
    let Some(value) = powershell_json(
        "Get-CimInstance Win32_Battery | Select-Object -First 1 EstimatedChargeRemaining,BatteryStatus | ConvertTo-Json -Compress",
    ) else {
        return BatteryInfo::default();
    };

    BatteryInfo {
        percent: value
            .get("EstimatedChargeRemaining")
            .and_then(Value::as_f64),
        status: value
            .get("BatteryStatus")
            .and_then(Value::as_i64)
            .map(battery_status_label),
    }
}

fn power_source() -> Option<String> {
    let value = powershell_json(
        "Add-Type -AssemblyName System.Windows.Forms; [pscustomobject]@{PowerLineStatus=[string][System.Windows.Forms.SystemInformation]::PowerStatus.PowerLineStatus} | ConvertTo-Json -Compress",
    )?;
    value
        .get("PowerLineStatus")
        .and_then(Value::as_str)
        .map(|status| match status.to_ascii_lowercase().as_str() {
            "online" => "ac".to_string(),
            "offline" => "battery".to_string(),
            _ => "unknown".to_string(),
        })
}

fn battery_saver_on() -> Option<bool> {
    let value = powershell_json(
        "$path='HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling'; $value=(Get-ItemProperty -Path $path -Name PowerThrottlingOff -ErrorAction SilentlyContinue).PowerThrottlingOff; [pscustomobject]@{BatterySaverOn=($value -ne 1)} | ConvertTo-Json -Compress",
    )?;
    value.get("BatterySaverOn").and_then(Value::as_bool)
}

fn cpu_clock_info() -> (Option<f64>, Option<f64>) {
    let Some(value) = powershell_json(
        "Get-CimInstance Win32_Processor | Select-Object -First 1 CurrentClockSpeed,MaxClockSpeed | ConvertTo-Json -Compress",
    ) else {
        return (None, None);
    };

    (
        value.get("CurrentClockSpeed").and_then(Value::as_f64),
        value.get("MaxClockSpeed").and_then(Value::as_f64),
    )
}

fn recommended_plan(
    power_source: Option<&str>,
    battery_percent: Option<f64>,
    battery_saver_on: Option<bool>,
    active_scheme_alias: Option<&str>,
    battery_low_threshold_percent: f64,
) -> String {
    if power_source == Some("battery") && battery_percent.unwrap_or(100.0) <= battery_low_threshold_percent {
        return "power_saver".to_string();
    }
    if battery_saver_on == Some(true) {
        return "power_saver".to_string();
    }
    if power_source == Some("ac") && active_scheme_alias == Some("power_saver") {
        return "balanced".to_string();
    }
    "balanced".to_string()
}

fn energy_recommendations(
    diagnostics: &EnergyDiagnostics,
    battery_low_threshold_percent: f64,
) -> Vec<String> {
    let mut recommendations = Vec::new();

    if diagnostics.power_source.as_deref() == Some("battery")
        && diagnostics.battery_percent.unwrap_or(100.0) <= battery_low_threshold_percent
    {
        recommendations.push("battery_low_use_power_saver".to_string());
    }
    if diagnostics.power_source.as_deref() == Some("battery")
        && diagnostics.active_scheme_alias.as_deref() == Some("high_performance")
    {
        recommendations.push("high_performance_on_battery".to_string());
    }
    if diagnostics.power_source.as_deref() == Some("ac")
        && diagnostics.active_scheme_alias.as_deref() == Some("power_saver")
    {
        recommendations.push("power_saver_while_plugged".to_string());
    }
    if diagnostics.battery_saver_on == Some(true) {
        recommendations.push("battery_saver_enabled".to_string());
    }
    if recommendations.is_empty() {
        recommendations.push("energy_profile_ok".to_string());
    }

    recommendations
}

fn scheme_alias(guid: &str, name: Option<&str>) -> Option<String> {
    let normalized_guid = guid.trim().to_ascii_lowercase();
    let normalized_name = name.unwrap_or_default().to_ascii_lowercase();

    if normalized_guid == "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"
        || normalized_name.contains("high performance")
        || normalized_name.contains("alto desempenho")
    {
        Some("high_performance".to_string())
    } else if normalized_guid == "381b4222-f694-41f0-9685-ff5bb260df2e"
        || normalized_name.contains("balanced")
        || normalized_name.contains("equilibr")
    {
        Some("balanced".to_string())
    } else if normalized_guid == "a1841308-3541-4fab-bc81-f71556f20b4a"
        || normalized_name.contains("power saver")
        || normalized_name.contains("economia")
    {
        Some("power_saver".to_string())
    } else {
        None
    }
}

fn powershell_json(script: &str) -> Option<Value> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = decode_console_bytes(&output.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(&text).ok()
}

fn battery_status_label(status: i64) -> String {
    match status {
        1 => "discharging",
        2 => "ac",
        3 => "fully_charged",
        4 => "low",
        5 => "critical",
        6 => "charging",
        7 => "charging_high",
        8 => "charging_low",
        9 => "charging_critical",
        10 => "undefined",
        11 => "partially_charged",
        _ => "unknown",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{overlay_guid_for_label, overlay_scheme_alias, scheme_alias};

    #[test]
    fn maps_windows_power_plan_aliases() {
        assert_eq!(
            scheme_alias("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c", None).as_deref(),
            Some("high_performance")
        );
        assert_eq!(
            scheme_alias("381b4222-f694-41f0-9685-ff5bb260df2e", None).as_deref(),
            Some("balanced")
        );
        assert_eq!(
            scheme_alias("a1841308-3541-4fab-bc81-f71556f20b4a", None).as_deref(),
            Some("power_saver")
        );
    }

    #[test]
    fn overlay_guid_and_alias_round_trip_for_every_known_label() {
        for label in ["high_performance", "balanced", "power_saver"] {
            let guid = overlay_guid_for_label(label).expect("label should map to an overlay guid");
            assert_eq!(overlay_scheme_alias(guid).as_deref(), Some(label));
        }
    }

    #[test]
    fn unknown_label_has_no_overlay_guid() {
        assert_eq!(overlay_guid_for_label("turbo"), None);
    }
}
