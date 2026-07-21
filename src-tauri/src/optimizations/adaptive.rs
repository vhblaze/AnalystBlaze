use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sysinfo::{ProcessesToUpdate, System};

use super::{detection, energy, latency, processes, ExecutionResult};
use crate::audit;

const DEFAULT_IDLE_ECO_THRESHOLD_SECONDS: u64 = 10 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowsSupportSummary {
    os_label: Option<String>,
    supported: bool,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdaptiveObservation {
    stage: String,
    timestamp: i64,
    cpu_usage_percent: f64,
    ram_usage_percent: f64,
    active_processes: usize,
    idle_seconds: u64,
    latency_ms: Option<f64>,
    jitter_ms: Option<f64>,
    packet_loss_percent: Option<f64>,
    power_plan: Option<String>,
    power_source: Option<String>,
    app_impact: Value,
}

pub async fn apply_adaptive_optimization(payload: Option<Value>) -> ExecutionResult {
    let payload = payload.unwrap_or_else(|| json!({}));
    let support = windows_support_summary();
    if !support.supported {
        return ExecutionResult::ok(
            "Adaptive Optimization ignorado porque este host nao parece ser Windows 10/11.",
            json!({
                "implemented": true,
                "skipped": true,
                "reason": support.reason,
                "windows": support,
            }),
        );
    }

    let before = collect_observation_blocking("before");
    let detection = detection::detect_game_process_with_payload(Some(&payload));

    let mut steps = Vec::new();
    let foreground = latency::apply_foreground_burst_mode(Some(payload.clone())).await;
    steps.push(json!({
        "id": "foreground_burst",
        "success": foreground.success,
        "message": foreground.message,
        "details": foreground.details,
    }));

    let affinity = processes::apply_game_affinity(Some(payload.clone()), &detection).await;
    steps.push(json!({
        "id": "game_affinity",
        "success": affinity.success,
        "message": affinity.message,
        "details": affinity.details,
    }));

    if should_apply_eco_mode(&payload, before.idle_seconds) {
        let execution_state = release_thread_execution_state_for_eco();
        let eco = energy::set_power_saver(Some(json!({
            "source": "adaptive_optimization",
            "reason": "idle_eco_mode",
        })))
        .await;
        steps.push(json!({
            "id": "idle_eco_mode",
            "success": eco.success,
            "message": eco.message,
            "details": {
                "powerPlan": eco.details,
                "threadExecutionState": execution_state,
                "screenBrightness": {
                    "changed": false,
                    "reason": "requires_device_specific_brightness_api_validation"
                }
            },
        }));
    } else {
        steps.push(json!({
            "id": "idle_eco_mode",
            "success": true,
            "skipped": true,
            "reason": "idle_threshold_not_met_or_disabled",
            "idleSeconds": before.idle_seconds,
        }));
    }

    let network_plan = network_admin_plan(&payload, is_elevated());
    steps.push(json!({
        "id": "network_admin_plan",
        "success": true,
        "message": "Plano de rede preparado sem aplicar alteracoes disruptivas.",
        "details": network_plan,
    }));

    let after = collect_observation_blocking("after");
    let snapshot_ids = collect_snapshot_ids(&steps);
    let success = steps
        .iter()
        .any(|step| step.get("success").and_then(Value::as_bool) == Some(true));
    let details = json!({
        "implemented": true,
        "service": "adaptive_optimization_manager",
        "windows": support,
        "before": before,
        "after": after,
        "steps": steps,
        "snapshotIds": snapshot_ids,
        "historyReport": {
            "latencySummary": {
                "profile": "adaptive_optimization",
                "before": {
                    "latencyMs": before.latency_ms,
                    "jitterMs": before.jitter_ms,
                    "packetLossPercent": before.packet_loss_percent,
                    "appImpact": before.app_impact,
                },
                "after": {
                    "latencyMs": after.latency_ms,
                    "jitterMs": after.jitter_ms,
                    "packetLossPercent": after.packet_loss_percent,
                    "appImpact": after.app_impact,
                },
                "rollback": {
                    "snapshotIds": snapshot_ids,
                    "reversible": !snapshot_ids.is_empty(),
                },
                "reasonCode": "adaptive_optimization_wave2",
            }
        },
        "hmacCoverage": "new ping/jitter/appImpact fields live inside existing telemetry details/history payloads and are covered by the existing canonical HMAC signer when transmitted",
        "notAutomated": [
            "nic_advanced_properties_are_vendor_specific_and_require_admin_helper",
            "screen_brightness_requires_device_specific_api_validation"
        ],
    });

    let _ = audit::record_event(
        if success { "info" } else { "warn" },
        "adaptive_optimization.completed",
        "Adaptive Optimization Manager executado com controles reversiveis.",
        details.clone(),
    );

    ExecutionResult {
        success,
        message: "Adaptive Optimization Manager executado.".to_string(),
        details,
    }
}

fn collect_observation_blocking(stage: &str) -> AdaptiveObservation {
    let mut system = System::new_all();
    system.refresh_cpu_usage();
    system.refresh_memory();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let ram_total = system.total_memory() as f64;
    let ram_used = system.used_memory() as f64;
    let ram_usage_percent = if ram_total > 0.0 {
        ((ram_used / ram_total) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let network = crate::telemetry::network::collect_network_sample();
    let energy = energy::collect_energy_diagnostics();
    let high_cpu_processes = system
        .processes()
        .values()
        .filter(|process| process.cpu_usage() >= 10.0)
        .count();

    AdaptiveObservation {
        stage: stage.to_string(),
        timestamp: chrono::Utc::now().timestamp(),
        cpu_usage_percent: system.global_cpu_usage() as f64,
        ram_usage_percent,
        active_processes: system.processes().len(),
        idle_seconds: idle_seconds(),
        latency_ms: Some(crate::telemetry::network::best_latency_ms(&network)),
        jitter_ms: network.jitter_ms,
        packet_loss_percent: network.packet_loss_percent,
        power_plan: energy.active_scheme_alias.or(energy.active_scheme_name),
        power_source: energy.power_source,
        app_impact: json!({
            "highCpuProcessCount": high_cpu_processes,
            "backgroundPressureScore": background_pressure_score(
                system.global_cpu_usage() as f64,
                ram_usage_percent,
                high_cpu_processes,
            ),
        }),
    }
}

fn should_apply_eco_mode(payload: &Value, idle_seconds: u64) -> bool {
    if !payload_bool(payload, "ecoMode", true) {
        return false;
    }
    let threshold = payload
        .get("idleEcoThresholdSeconds")
        .or_else(|| payload.get("idle_eco_threshold_seconds"))
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_IDLE_ECO_THRESHOLD_SECONDS)
        .clamp(60, 3 * 60 * 60);
    idle_seconds >= threshold
}

fn background_pressure_score(cpu_usage: f64, ram_usage: f64, high_cpu_processes: usize) -> f64 {
    ((cpu_usage * 0.45) + (ram_usage * 0.35) + ((high_cpu_processes as f64) * 6.0))
        .clamp(0.0, 100.0)
}

fn network_admin_plan(payload: &Value, is_admin: bool) -> Value {
    let requested = payload_bool(payload, "includeNetworkAdminTweaks", false);
    let confirmed = payload_bool(payload, "networkChangesConfirmed", false);
    let adapter_name = payload
        .get("adapterName")
        .or_else(|| payload.get("adapter_name"))
        .and_then(Value::as_str)
        .unwrap_or("<adapter>");
    let dns_servers = payload
        .get("dnsServers")
        .or_else(|| payload.get("dns_servers"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| is_safe_dns_literal(item))
                .take(2)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let status = if !requested {
        "not_requested"
    } else if !confirmed {
        "blocked_user_consent_required"
    } else if !is_admin {
        "blocked_elevation_required"
    } else {
        "ready_for_privileged_helper"
    };

    // NIC advanced-property names vary by vendor/driver. The manager emits an explicit plan
    // instead of mutating ambiguous properties from the default user-mode agent.
    json!({
        "requested": requested,
        "confirmed": confirmed,
        "isAdmin": is_admin,
        "status": status,
        "requiresPrivilegedHelper": requested,
        "operations": [
            {
                "id": "flush_dns",
                "description": "Flush DNS resolver cache.",
                "commandPreview": ["ipconfig", "/flushdns"],
                "requiresAdmin": false,
                "requiresReboot": false,
                "reversible": false,
                "executionStatus": "automated_via_flush_dns_cache_action",
            },
            {
                "id": "winsock_reset",
                "description": "Reset Winsock catalog; disruptive and normally requires reboot.",
                "commandPreview": ["netsh", "winsock", "reset"],
                "requiresAdmin": true,
                "requiresReboot": true,
                "reversible": false,
                "executionStatus": "automated_via_reset_winsock_catalog_action",
            },
            {
                "id": "set_dns_servers",
                "description": "Change adapter DNS only after capturing current adapter DNS configuration.",
                "adapterName": adapter_name,
                "dnsServers": dns_servers,
                "commandPreview": if dns_servers.is_empty() {
                    json!(null)
                } else {
                    json!(["netsh", "interface", "ip", "set", "dns", &format!("name={adapter_name}"), "static", &dns_servers[0]])
                },
                "requiresAdmin": true,
                "requiresReboot": false,
                "reversible": true,
                "executionStatus": if dns_servers.is_empty() {
                    "requires_dns_servers_input"
                } else {
                    "automated_via_set_dns_servers_action"
                },
            },
            {
                "id": "nic_advanced_properties",
                "description": "Driver-specific candidates: Interrupt Moderation, Flow Control, Energy Efficient Ethernet, Jumbo Packet.",
                "candidateProperties": ["Interrupt Moderation", "Flow Control", "Energy Efficient Ethernet", "Jumbo Packet"],
                "requiresAdmin": true,
                "requiresReboot": false,
                "reversible": true,
                "executionStatus": "requires_vendor_property_discovery_snapshot_and_helper",
            }
        ],
    })
}

fn collect_snapshot_ids(steps: &[Value]) -> Vec<String> {
    let mut ids = Vec::new();
    for step in steps {
        collect_snapshot_ids_from_value(step, &mut ids);
    }
    ids.sort();
    ids.dedup();
    ids
}

fn collect_snapshot_ids_from_value(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(id) = map
                .get("snapshot")
                .and_then(|snapshot| snapshot.get("id"))
                .and_then(Value::as_str)
            {
                ids.push(id.to_string());
            }
            for child in map.values() {
                collect_snapshot_ids_from_value(child, ids);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_snapshot_ids_from_value(item, ids);
            }
        }
        _ => {}
    }
}

fn payload_bool(payload: &Value, key: &str, default: bool) -> bool {
    payload
        .get(key)
        .or_else(|| {
            let snake = key
                .chars()
                .flat_map(|ch| {
                    if ch.is_ascii_uppercase() {
                        vec!['_', ch.to_ascii_lowercase()]
                    } else {
                        vec![ch]
                    }
                })
                .collect::<String>();
            payload.get(snake)
        })
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

pub(crate) fn is_safe_dns_literal(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 45
        && value
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || matches!(ch, '.' | ':'))
}

#[cfg(windows)]
fn release_thread_execution_state_for_eco() -> Value {
    use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};

    let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    json!({
        "attempted": true,
        "api": "SetThreadExecutionState",
        "requestedState": "ES_CONTINUOUS",
        "previousState": previous.0,
        "note": "clears current-thread execution requirements; does not suspend or throttle foreign threads"
    })
}

#[cfg(not(windows))]
fn release_thread_execution_state_for_eco() -> Value {
    json!({
        "attempted": false,
        "reason": "windows_only"
    })
}

fn windows_support_summary() -> WindowsSupportSummary {
    let os_label = System::long_os_version().or_else(System::name);
    // The OS label string is unreliable for telling Windows 10 and 11 apart
    // (ProductName can still read "Windows 10" on some Win11 builds) - the
    // actual gate is the detected build number, see os_version.rs.
    let supported = !matches!(
        super::os_version::detected().generation,
        super::os_version::WindowsGeneration::Unknown
    );
    WindowsSupportSummary {
        os_label,
        supported,
        reason: if supported {
            None
        } else {
            Some("requires_windows_10_or_11".to_string())
        },
    }
}

#[cfg(windows)]
fn is_elevated() -> bool {
    unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
}

#[cfg(not(windows))]
fn is_elevated() -> bool {
    false
}

#[cfg(windows)]
fn idle_seconds() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut last_input = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if !unsafe { GetLastInputInfo(&mut last_input) }.as_bool() {
        return 0;
    }
    let now_ms = unsafe { GetTickCount64() };
    now_ms.saturating_sub(last_input.dwTime as u64) / 1_000
}

#[cfg(not(windows))]
fn idle_seconds() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        background_pressure_score, is_safe_dns_literal, network_admin_plan, should_apply_eco_mode,
    };

    #[test]
    fn gates_eco_mode_on_idle_threshold() {
        assert!(should_apply_eco_mode(
            &json!({ "idleEcoThresholdSeconds": 120 }),
            180
        ));
        assert!(!should_apply_eco_mode(
            &json!({ "idleEcoThresholdSeconds": 120 }),
            30
        ));
        assert!(!should_apply_eco_mode(&json!({ "ecoMode": false }), 9999));
    }

    #[test]
    fn network_admin_plan_requires_request_consent_and_elevation() {
        let not_requested = network_admin_plan(&json!({}), false);
        assert_eq!(not_requested["status"], "not_requested");

        let no_consent = network_admin_plan(&json!({ "includeNetworkAdminTweaks": true }), true);
        assert_eq!(no_consent["status"], "blocked_user_consent_required");

        let no_admin = network_admin_plan(
            &json!({
                "includeNetworkAdminTweaks": true,
                "networkChangesConfirmed": true,
                "dnsServers": ["1.1.1.1", "8.8.8.8"],
            }),
            false,
        );
        assert_eq!(no_admin["status"], "blocked_elevation_required");

        let ready = network_admin_plan(
            &json!({
                "includeNetworkAdminTweaks": true,
                "networkChangesConfirmed": true,
                "adapterName": "Ethernet",
                "dnsServers": ["1.1.1.1"],
            }),
            true,
        );
        assert_eq!(ready["status"], "ready_for_privileged_helper");
        assert_eq!(ready["operations"][1]["id"], "winsock_reset");
    }

    #[test]
    fn validates_dns_literals_and_pressure_score() {
        assert!(is_safe_dns_literal("1.1.1.1"));
        assert!(is_safe_dns_literal("2606:4700:4700::1111"));
        assert!(!is_safe_dns_literal("$(bad)"));
        assert!(background_pressure_score(90.0, 80.0, 5) > 90.0);
    }
}
