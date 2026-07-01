use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;

use super::{detection, processes, snapshot, ExecutionResult};
use crate::audit;

const DEFAULT_LATENCY_SESSION_TTL_SECONDS: i64 = 45 * 60;
const LATENCY_SESSION_FILE: &str = "latency-session.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencySession {
    pub id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub target_pid: Option<u32>,
    pub target_process_name: Option<String>,
    pub snapshot_ids: Vec<String>,
    pub status: String,
    pub restore_reason: Option<String>,
    pub restored_at: Option<i64>,
    pub before: Value,
    pub after: Option<Value>,
    pub confidence: f64,
}

pub async fn apply_foreground_burst_mode(payload: Option<Value>) -> ExecutionResult {
    let detection = detection::detect_game_process_with_payload(payload.as_ref());
    let before = latency_observation("before", &detection);
    let foreground = processes::apply_foreground_burst_priority(payload.clone(), &detection).await;
    let quiet_background = if payload_bool(payload.as_ref(), "quiet_background", true) {
        processes::apply_background_quiet_mode(payload.clone()).await
    } else {
        ExecutionResult::ok(
            "Background Quiet ignorado pela policy do Foreground Burst.",
            json!({ "implemented": true, "skipped_by_policy": true }),
        )
    };
    let after = latency_observation("after", &detection);
    let snapshot_ids = collect_snapshot_ids([&foreground.details, &quiet_background.details]);
    let confidence = burst_confidence(&detection, &before, &after);
    let ttl_seconds = payload
        .as_ref()
        .and_then(|payload| {
            payload
                .get("ttl_seconds")
                .or_else(|| payload.get("ttlSeconds"))
                .and_then(Value::as_i64)
        })
        .unwrap_or(DEFAULT_LATENCY_SESSION_TTL_SECONDS)
        .clamp(60, 3 * 60 * 60);

    let session = if !snapshot_ids.is_empty() {
        save_latency_session(
            detection
                .pid
                .as_deref()
                .and_then(|pid| pid.parse::<u32>().ok()),
            detection.process_name.clone(),
            snapshot_ids.clone(),
            before.clone(),
            Some(after.clone()),
            confidence,
            ttl_seconds,
        )
        .ok()
    } else {
        None
    };

    let success = foreground.success || quiet_background.success;
    let details = json!({
        "implemented": true,
        "profile": "foreground_burst",
        "reversible": !snapshot_ids.is_empty(),
        "detection": detection,
        "before": before,
        "after": after,
        "confidence": confidence,
        "snapshotIds": snapshot_ids,
        "restoreSession": session,
        "steps": {
            "foreground": {
                "success": foreground.success,
                "message": foreground.message,
                "details": foreground.details,
            },
            "backgroundQuiet": {
                "success": quiet_background.success,
                "message": quiet_background.message,
                "details": quiet_background.details,
            },
        },
        "powerPlan": {
            "changed": false,
            "reason": "wave1_uses_documented_user_mode_process_controls_only"
        },
        "adminOnly": {
            "qosPolicy": "not_automated_without_helper",
            "wifiInterfaceMutation": "not_automated_without_documented_wlan_wrapper",
            "cpuSets": "not_automated_without_topology_validation"
        }
    });

    let _ = audit::record_event(
        if success { "info" } else { "warn" },
        "latency.foreground_burst",
        "Foreground Burst processado com controles reversiveis.",
        details.clone(),
    );

    ExecutionResult {
        success,
        message: if success {
            "Foreground Burst aplicado com ajustes reversiveis de processo.".to_string()
        } else {
            "Foreground Burst nao encontrou ajustes aplicaveis nesta sessao.".to_string()
        },
        details,
    }
}

pub async fn apply_background_quiet_mode(payload: Option<Value>) -> ExecutionResult {
    processes::apply_background_quiet_mode(payload).await
}

pub async fn apply_uplink_pressure_relief_stage1(payload: Option<Value>) -> ExecutionResult {
    let quiet = processes::apply_background_quiet_mode(payload.clone()).await;
    let details = json!({
        "implemented": true,
        "stage": 1,
        "noisyPidAttribution": "not_enabled_in_wave1",
        "hostOnlyControls": ["below_normal_priority", "low_memory_priority", "ecoqos_execution_speed"],
        "manualSuggestions": [
            "pause_cloud_sync_if_uploading",
            "pause_game_launcher_downloads",
            "close_inactive_browser_tabs_with_video_uploads"
        ],
        "internetRttGuarantee": false,
        "quiet": {
            "success": quiet.success,
            "message": quiet.message,
            "details": quiet.details,
        },
        "adminOnlyStage2": {
            "localQosPolicy": "requires_helper_admin_opt_in_and_ttl_rollback"
        }
    });

    ExecutionResult {
        success: quiet.success,
        message: if quiet.success {
            "Uplink Pressure Relief Stage 1 aplicado a apps de fundo elegiveis.".to_string()
        } else {
            "Uplink Pressure Relief Stage 1 nao encontrou apps elegiveis para reduzir pressao local.".to_string()
        },
        details,
    }
}

pub async fn apply_latency_tweaks(payload: Option<Value>) -> ExecutionResult {
    let profile = payload
        .as_ref()
        .and_then(|payload| payload.get("profile").and_then(Value::as_str))
        .unwrap_or("foreground_burst");
    match profile {
        "foreground_burst" | "burst" => apply_foreground_burst_mode(payload).await,
        "background_quiet" | "quiet" => apply_background_quiet_mode(payload).await,
        "uplink_pressure_relief" | "uplink_stage1" => {
            apply_uplink_pressure_relief_stage1(payload).await
        }
        _ => ExecutionResult::ok(
            "Perfil de latencia desconhecido; nenhuma alteracao aplicada.",
            json!({
                "implemented": true,
                "skipped": true,
                "reason": "unknown_latency_profile",
                "profile": profile,
                "allowedProfiles": ["foreground_burst", "background_quiet", "uplink_stage1"]
            }),
        ),
    }
}

pub fn restore_latency_session(reason: Option<String>) -> snapshot::RestoreReport {
    let Some(mut session) = active_latency_session() else {
        return snapshot::RestoreReport {
            restored_snapshots: 0,
            failed_snapshots: 0,
            restored_entries: 0,
            failed_entries: 0,
            skipped_conflicts: 0,
            messages: vec!["Nenhuma sessao de latencia ativa encontrada.".to_string()],
        };
    };

    let report = snapshot::restore_snapshots_by_ids(&session.snapshot_ids);
    session.status = "restored".to_string();
    session.restored_at = Some(chrono::Utc::now().timestamp());
    session.restore_reason = reason.or_else(|| Some("manual_restore".to_string()));
    let _ = write_latency_session(&session);
    let _ = audit::record_event(
        "info",
        "latency.session_restored",
        "Sessao de latencia restaurada por snapshots locais.",
        serde_json::to_value(&report).unwrap_or(Value::Null),
    );
    report
}

pub fn active_latency_session() -> Option<LatencySession> {
    let raw = fs::read_to_string(latency_session_path()).ok()?;
    let mut session = serde_json::from_str::<LatencySession>(&raw).ok()?;
    if session.restored_at.is_some() || session.status == "restored" {
        return None;
    }
    if session.expires_at <= chrono::Utc::now().timestamp() {
        session.status = "expired".to_string();
        let _ = write_latency_session(&session);
        return None;
    }
    Some(session)
}

fn save_latency_session(
    target_pid: Option<u32>,
    target_process_name: Option<String>,
    snapshot_ids: Vec<String>,
    before: Value,
    after: Option<Value>,
    confidence: f64,
    ttl_seconds: i64,
) -> Result<LatencySession, String> {
    let now = chrono::Utc::now().timestamp();
    let session = LatencySession {
        id: uuid::Uuid::new_v4().simple().to_string(),
        created_at: now,
        expires_at: now + ttl_seconds,
        target_pid,
        target_process_name,
        snapshot_ids,
        status: "active".to_string(),
        restore_reason: None,
        restored_at: None,
        before,
        after,
        confidence,
    };
    write_latency_session(&session)?;
    Ok(session)
}

fn write_latency_session(session: &LatencySession) -> Result<(), String> {
    let path = latency_session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn latency_session_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join(LATENCY_SESSION_FILE)
}

fn latency_observation(stage: &str, detection: &detection::GameDetection) -> Value {
    let network = crate::telemetry::network::collect_network_sample();
    json!({
        "stage": stage,
        "timestamp": chrono::Utc::now().timestamp(),
        "targetPid": detection.pid.clone(),
        "targetProcess": detection.process_name.clone(),
        "targetReason": detection.reason.clone(),
        "network": {
            "connected": network.connected,
            "latencyMs": crate::telemetry::network::best_latency_ms(&network),
            "jitterMs": network.jitter_ms,
            "packetLossPercent": network.packet_loss_percent,
            "recommendations": network.recommendations,
        }
    })
}

fn burst_confidence(detection: &detection::GameDetection, before: &Value, after: &Value) -> f64 {
    let detection_score = detection.confidence.clamp(0.0, 1.0) * 0.65;
    let before_latency = before
        .pointer("/network/latencyMs")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let after_latency = after
        .pointer("/network/latencyMs")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let network_score =
        if before_latency > 0.0 && after_latency > 0.0 && after_latency <= before_latency {
            0.2
        } else {
            0.08
        };
    (detection_score + network_score + 0.15).clamp(0.0, 0.98)
}

fn payload_bool(payload: Option<&Value>, key: &str, default: bool) -> bool {
    payload
        .and_then(|payload| payload.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn collect_snapshot_ids<'a>(details: impl IntoIterator<Item = &'a Value>) -> Vec<String> {
    details
        .into_iter()
        .filter_map(|details| details.pointer("/snapshot/id").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{burst_confidence, detection};

    #[test]
    fn burst_confidence_combines_detection_and_observation() {
        let detection = detection::GameDetection {
            detected: true,
            process_name: Some("game.exe".to_string()),
            pid: Some("123".to_string()),
            confidence: 0.9,
            reason: "test".to_string(),
        };
        let before = json!({ "network": { "latencyMs": 80.0 } });
        let after = json!({ "network": { "latencyMs": 60.0 } });

        assert!(burst_confidence(&detection, &before, &after) >= 0.9);
    }
}
