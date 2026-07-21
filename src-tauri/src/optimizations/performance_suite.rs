use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};
use sysinfo::System;
use uuid::Uuid;

use super::{
    cleanup, detection,
    local_ai_policy::{self, LocalAiPolicy},
    processes,
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    visual_effects, windows_actions, windows_inventory, ExecutionResult,
};
use crate::audit;
use crate::telemetry::collector::{TelemetryCollector, TelemetrySample};

const REPORT_HISTORY_LIMIT: usize = 40;
const CATEGORY_SCAN_LIMIT: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceReport {
    pub id: String,
    pub device_id: Option<String>,
    pub generated_at: i64,
    pub mode: String,
    pub overall_score: f64,
    pub previous_score: Option<f64>,
    pub measured_gain_percent: Option<f64>,
    pub score_delta_percent: Option<f64>,
    pub score_delta_points: Option<f64>,
    #[serde(default = "unknown_performance_change")]
    pub performance_change: String,
    pub score_breakdown: ScoreBreakdown,
    pub metrics: PerformanceMetrics,
    pub deltas: Vec<PerformanceDelta>,
    pub actions: Vec<PerformanceActionSummary>,
    pub bottlenecks: Vec<PerformanceBottleneck>,
    pub restore_session: Option<RestoreSessionSummary>,
    pub source: String,
    pub metrics_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBreakdown {
    pub boot_startup: f64,
    pub background: f64,
    pub memory: f64,
    pub disk: f64,
    pub network: f64,
    pub energy: f64,
    pub thermal: f64,
    pub gaming: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceMetrics {
    pub cpu_usage_percent: f64,
    pub gpu_usage_percent: f64,
    pub ram_usage_percent: f64,
    pub disk_usage_percent: f64,
    pub latency_ms: f64,
    pub jitter_ms: Option<f64>,
    pub packet_loss_percent: Option<f64>,
    pub active_processes: usize,
    pub cleanup_reclaimable_bytes: u64,
    pub startup_apps: usize,
    pub high_impact_startup_apps: usize,
    pub pending_snapshots: usize,
    pub power_plan: Option<String>,
    pub cpu_temperature_c: Option<f64>,
    pub gpu_temperature_c: Option<f64>,
    pub game_detected: bool,
    pub game_process: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceDelta {
    pub key: String,
    pub before: Option<f64>,
    pub after: Option<f64>,
    pub unit: String,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceActionSummary {
    pub action_name: String,
    pub status: String,
    pub message: String,
    pub snapshot_id: Option<String>,
    pub reversible: bool,
    pub impact_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceBottleneck {
    pub id: String,
    pub label: String,
    pub severity: String,
    pub score: f64,
    pub metric: Option<String>,
    pub recommended_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSessionSummary {
    pub id: String,
    pub snapshot_ids: Vec<String>,
    pub status: String,
    pub created_at: i64,
    pub restored_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupCategory {
    pub id: String,
    pub label: String,
    pub reclaimable_bytes: u64,
    pub scanned_paths: Vec<String>,
    pub risk: String,
    pub requires_helper: bool,
    pub reversible: bool,
    pub available_actions: Vec<String>,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupImpact {
    pub name: String,
    pub location: String,
    pub publisher: Option<String>,
    pub command_preview: String,
    pub impact_score: f64,
    pub risk: String,
    pub recommendation: String,
    pub available_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceSession {
    pub id: String,
    pub baseline_report_id: Option<String>,
    pub after_report_id: Option<String>,
    pub snapshot_ids: Vec<String>,
    pub status: String,
    pub created_at: i64,
    pub restored_at: Option<i64>,
    pub actions: Vec<PerformanceActionSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PcCleanFastOptions {
    #[serde(default = "default_true")]
    pub include_startup: bool,
    #[serde(default = "default_true")]
    pub include_cleanup: bool,
    #[serde(default = "default_true")]
    pub include_background: bool,
    #[serde(default)]
    pub include_network: bool,
    #[serde(default = "default_true")]
    pub include_gaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DelayedStartupRecord {
    name: String,
    location: String,
    command: String,
    delay_seconds: u64,
    created_at: i64,
    disabled_snapshot_id: Option<String>,
    launch_supported: bool,
    last_launched_boot_id: Option<i64>,
}

#[derive(Debug, Default)]
struct CategoryScanSummary {
    scanned_files: usize,
    scanned_dirs: usize,
    reclaimable_bytes: u64,
    skipped_recent: usize,
    failed_entries: usize,
    capped: bool,
}

#[derive(Debug, Default)]
struct CategoryApplySummary {
    scanned_files: usize,
    scanned_dirs: usize,
    quarantined_files: usize,
    quarantined_bytes: u64,
    removed_empty_dirs: usize,
    skipped_recent: usize,
    skipped_special: usize,
    failed_entries: usize,
    entries: Vec<SnapshotEntry>,
}

pub async fn run_performance_scan(
    mode: String,
    device_id: Option<String>,
) -> Result<PerformanceReport, String> {
    tokio::task::spawn_blocking(move || run_performance_scan_blocking(&mode, device_id))
        .await
        .map_err(|error| error.to_string())?
}

pub async fn scan_cleanup_categories() -> Result<Vec<CleanupCategory>, String> {
    tokio::task::spawn_blocking(scan_cleanup_categories_blocking)
        .await
        .map_err(|error| error.to_string())
}

pub async fn apply_cleanup_category(category: String, mode: Option<String>) -> ExecutionResult {
    let category = category.trim().to_ascii_lowercase();
    let mode = mode.unwrap_or_else(|| "safe".to_string());

    if category == "user_temp" {
        return cleanup::empty_temp(Some(json!({
            "mode": if mode == "deep_confirmed" { "deep_confirmed" } else { "safe" },
            "min_age_minutes": if mode == "deep_confirmed" { 5 } else { 60 },
            "include_windows_temp": false,
        })))
        .await;
    }
    if category == "windows_temp" {
        return cleanup::empty_temp(Some(json!({
            "mode": if mode == "deep_confirmed" { "deep_confirmed" } else { "safe" },
            "min_age_minutes": if mode == "deep_confirmed" { 5 } else { 60 },
            "include_windows_temp": true,
        })))
        .await;
    }
    if category == "cleanup_quarantine" {
        if mode != "purge" {
            return ExecutionResult {
                success: false,
                message: "Purge da quarentena exige confirmacao explicita do usuario.".to_string(),
                details: json!({
                    "implemented": true,
                    "category": category,
                    "required_mode": "purge",
                    "confirmed": false,
                }),
            };
        }
        return cleanup::purge_cleanup_quarantine(Some(json!({
            "user_confirmed_purge": true,
            "confirmation": "purge_cleanup_quarantine",
        })))
        .await;
    }

    match tokio::task::spawn_blocking(move || apply_cleanup_category_blocking(&category, &mode))
        .await
    {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao aplicar categoria de limpeza: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn scan_startup_impact() -> Result<Vec<StartupImpact>, String> {
    tokio::task::spawn_blocking(scan_startup_impact_blocking)
        .await
        .map_err(|error| error.to_string())
}

pub async fn delay_startup_app(
    name: String,
    location: Option<String>,
    delay_seconds: Option<u64>,
) -> ExecutionResult {
    let delay_seconds = delay_seconds.unwrap_or(120).clamp(30, 900);
    let target_name = name.trim().to_string();
    if target_name.is_empty() {
        return ExecutionResult {
            success: false,
            message: "Informe o app de inicializacao para atrasar.".to_string(),
            details: json!({ "implemented": true }),
        };
    }

    let inventory = windows_inventory::collect_windows_inventory();
    let Some(app) = inventory.startup_apps.into_iter().find(|app| {
        app.name.eq_ignore_ascii_case(&target_name)
            && location
                .as_deref()
                .map(|location| app.location.eq_ignore_ascii_case(location))
                .unwrap_or(true)
    }) else {
        return ExecutionResult {
            success: false,
            message: "App de inicializacao nao encontrado no inventario local.".to_string(),
            details: json!({
                "implemented": true,
                "name": target_name,
                "location": location,
            }),
        };
    };

    if app.risk != "safe" {
        return ExecutionResult {
            success: false,
            message: "Somente apps seguros podem ser movidos para inicializacao atrasada."
                .to_string(),
            details: json!({
                "implemented": true,
                "name": app.name,
                "risk": app.risk,
            }),
        };
    }

    let disabled = windows_actions::disable_startup_app(Some(json!({
        "name": app.name,
        "location": app.location,
    })))
    .await;
    if !disabled.success {
        return disabled;
    }

    let snapshot_id = disabled
        .details
        .pointer("/snapshot/id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let record = DelayedStartupRecord {
        name: app.name.clone(),
        location: app.location.clone(),
        command: app.command.clone(),
        delay_seconds,
        created_at: chrono::Utc::now().timestamp(),
        disabled_snapshot_id: snapshot_id.clone(),
        launch_supported: parse_launch_command(&app.command).is_some(),
        last_launched_boot_id: None,
    };

    match upsert_delayed_startup_record(record.clone()) {
        Ok(()) => ExecutionResult::ok(
            "App movido para fila local de inicializacao atrasada com snapshot reversivel.",
            json!({
                "implemented": true,
                "record": record,
                "snapshot": {
                    "id": snapshot_id,
                    "reversible": true,
                },
            }),
        ),
        Err(error) => {
            let _ = snapshot::restore_startup_app_snapshots(Some(&app.name));
            ExecutionResult {
                success: false,
                message: "Atraso revertido porque nao foi possivel salvar a fila local."
                    .to_string(),
                details: json!({
                    "implemented": true,
                    "error": error,
                    "rollback": "restore_startup_snapshot_attempted",
                }),
            }
        }
    }
}

pub async fn restore_delayed_startup_app(name: Option<String>) -> ExecutionResult {
    match tokio::task::spawn_blocking(move || restore_delayed_startup_app_blocking(name)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar inicializacao atrasada: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn apply_pc_clean_fast_profile(options: PcCleanFastOptions) -> ExecutionResult {
    let baseline = match run_performance_scan("baseline".to_string(), None).await {
        Ok(report) => report,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: format!("Falha ao coletar baseline do Performance Scan: {error}"),
                details: json!({ "implemented": true }),
            }
        }
    };

    let mut actions = Vec::new();
    let mut snapshot_ids = Vec::new();

    if options.include_cleanup {
        let cleanup =
            apply_cleanup_category("user_temp".to_string(), Some("safe".to_string())).await;
        append_action_result(
            &mut actions,
            &mut snapshot_ids,
            "APPLY_CLEANUP_CATEGORY",
            &cleanup,
        );
    }

    if options.include_background {
        let visual = visual_effects::apply_visual_performance_mode(None).await;
        append_action_result(
            &mut actions,
            &mut snapshot_ids,
            "APPLY_VISUAL_PERFORMANCE_MODE",
            &visual,
        );

        let background = processes::optimize_background_process_priorities(Some(json!({
            "backgroundPriority": "below_normal",
            "maxBackgroundProcesses": 20,
        })))
        .await;
        append_action_result(
            &mut actions,
            &mut snapshot_ids,
            "APPLY_PC_CLEAN_FAST_BACKGROUND_PRIORITIES",
            &background,
        );
    }

    if options.include_gaming {
        let detected_game = detection::detect_game_process_with_payload(None);
        if detected_game.detected {
            let game_mode = super::apply_game_mode(Some(json!({
                    "safe_temp_cleanup": false,
                    "enter_focus_mode": true,
                    "optimize_visual_effects": false,
                    "optimize_process_priorities": true,
                    "auto_restore": true,
            })))
            .await;
            append_action_result(
                &mut actions,
                &mut snapshot_ids,
                "APPLY_GAME_MODE",
                &game_mode,
            );
        }
    }

    if options.include_network {
        let network = crate::telemetry::network::collect_network_diagnostics();
        actions.push(PerformanceActionSummary {
            action_name: "NETWORK_DIAGNOSTICS".to_string(),
            status: "observed".to_string(),
            message: network.recommendations.join(" / "),
            snapshot_id: None,
            reversible: false,
            impact_score: 0.0,
        });
    }

    if options.include_startup {
        let startup = scan_startup_impact_blocking();
        let candidates = startup
            .iter()
            .filter(|item| item.recommendation == "delay" && item.risk == "safe")
            .take(2)
            .cloned()
            .collect::<Vec<_>>();
        for app in candidates {
            let delayed =
                delay_startup_app(app.name.clone(), Some(app.location.clone()), Some(120)).await;
            append_action_result(
                &mut actions,
                &mut snapshot_ids,
                "DELAY_STARTUP_APP",
                &delayed,
            );
        }
    }

    let mut after = match run_performance_scan("after".to_string(), None).await {
        Ok(report) => report,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: format!("Falha ao coletar scan final: {error}"),
                details: json!({
                    "implemented": true,
                    "baselineReportId": baseline.id,
                    "actions": actions,
                    "snapshotIds": snapshot_ids,
                }),
            }
        }
    };
    after.actions = actions.clone();
    after.restore_session = Some(RestoreSessionSummary {
        id: Uuid::new_v4().simple().to_string(),
        snapshot_ids: snapshot_ids.clone(),
        status: if snapshot_ids.is_empty() {
            "none".to_string()
        } else {
            "available".to_string()
        },
        created_at: chrono::Utc::now().timestamp(),
        restored_at: None,
    });
    let _ = save_report(after.clone());

    let session = PerformanceSession {
        id: after
            .restore_session
            .as_ref()
            .map(|session| session.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string()),
        baseline_report_id: Some(baseline.id.clone()),
        after_report_id: Some(after.id.clone()),
        snapshot_ids,
        status: "applied".to_string(),
        created_at: chrono::Utc::now().timestamp(),
        restored_at: None,
        actions,
    };
    let _ = write_json_file(&performance_session_path(), &session);

    ExecutionResult::ok(
        "Perfil PC limpo/rapido aplicado com ganhos medidos localmente.",
        json!({
            "implemented": true,
            "session": session,
            "baselineReport": baseline,
            "afterReport": after,
        }),
    )
}

pub fn restore_performance_session(session_id: Option<String>) -> ExecutionResult {
    let path = performance_session_path();
    let Ok(mut session) = read_json_file::<PerformanceSession>(&path) else {
        return ExecutionResult {
            success: false,
            message: "Nenhuma sessao de Performance Suite encontrada para restaurar.".to_string(),
            details: json!({ "implemented": true }),
        };
    };

    if let Some(expected) = session_id.as_deref() {
        if session.id != expected {
            return ExecutionResult {
                success: false,
                message: "Sessao solicitada nao corresponde a sessao ativa local.".to_string(),
                details: json!({
                    "implemented": true,
                    "requestedSessionId": expected,
                    "activeSessionId": session.id,
                }),
            };
        }
    }

    let report = snapshot::restore_snapshots_by_ids(&session.snapshot_ids);
    session.status = if report.failed_snapshots == 0 && report.failed_entries == 0 {
        "restored".to_string()
    } else {
        "restore_failed".to_string()
    };
    session.restored_at = Some(chrono::Utc::now().timestamp());
    let _ = write_json_file(&path, &session);

    let success = report.failed_snapshots == 0 && report.failed_entries == 0;
    ExecutionResult {
        success,
        message: if success {
            "Sessao de performance restaurada por snapshots locais.".to_string()
        } else {
            "Restauracao da sessao de performance concluiu com falhas.".to_string()
        },
        details: json!({
            "implemented": true,
            "session": session,
            "restore": report,
        }),
    }
}

pub fn spawn_delayed_startup_runner() {
    thread::spawn(|| {
        let records = match read_delayed_startup_records() {
            Ok(records) => records,
            Err(_) => return,
        };
        if records.is_empty() {
            return;
        }
        let boot_id = current_boot_id();
        let mut updated = records.clone();

        for record in records {
            if !record.launch_supported || record.last_launched_boot_id == Some(boot_id) {
                continue;
            }
            let Some((program, args)) = parse_launch_command(&record.command) else {
                continue;
            };
            thread::sleep(Duration::from_secs(record.delay_seconds));
            if Command::new(&program).args(args).spawn().is_ok() {
                if let Some(stored) = updated.iter_mut().find(|item| {
                    item.name.eq_ignore_ascii_case(&record.name)
                        && item.location.eq_ignore_ascii_case(&record.location)
                }) {
                    stored.last_launched_boot_id = Some(boot_id);
                }
                let _ = audit::record_event(
                    "info",
                    "performance.delayed_startup_launched",
                    "App de inicializacao atrasada executado pelo agente local.",
                    json!({
                        "name": record.name,
                        "delay_seconds": record.delay_seconds,
                    }),
                );
            }
        }

        let _ = write_delayed_startup_records(&updated);
    });
}

pub fn performance_summary_payload(report: &PerformanceReport) -> Value {
    json!({
        "reportId": report.id,
        "deviceId": report.device_id,
        "generatedAt": report.generated_at,
        "overallScore": report.overall_score,
        "previousScore": report.previous_score,
        "measuredGainPercent": report.measured_gain_percent,
        "scoreDeltaPercent": report.score_delta_percent,
        "scoreDeltaPoints": report.score_delta_points,
        "performanceChange": report.performance_change,
        "scoreBreakdown": report.score_breakdown,
        "deltas": report.deltas,
        "actions": report.actions.iter().take(12).collect::<Vec<_>>(),
        "bottlenecks": report.bottlenecks.iter().take(8).collect::<Vec<_>>(),
        "restoreSession": report.restore_session,
        "source": report.source,
        "metricsVersion": report.metrics_version,
    })
}

fn run_performance_scan_blocking(
    mode: &str,
    device_id: Option<String>,
) -> Result<PerformanceReport, String> {
    let normalized_mode = normalize_scan_mode(mode);
    let mut collector = TelemetryCollector::new();
    let sample = collector.collect();
    let cleanup_categories = scan_cleanup_categories_blocking();
    let startup_impact = scan_startup_impact_blocking();
    let detected_game = detection::detect_game_process_with_payload(None);
    let power_plan = snapshot::active_power_plan().ok();
    let pending_snapshots = snapshot::list_snapshots(250)
        .unwrap_or_default()
        .into_iter()
        .filter(|snapshot| snapshot.restored_at.is_none())
        .count();
    let cleanup_reclaimable_bytes = cleanup_categories
        .iter()
        .filter(|category| category.id != "cleanup_quarantine")
        .map(|category| category.reclaimable_bytes)
        .fold(0_u64, u64::saturating_add);
    let high_impact_startup_apps = startup_impact
        .iter()
        .filter(|item| item.impact_score >= 65.0)
        .count();
    let metrics = PerformanceMetrics {
        cpu_usage_percent: round1(sample.cpu_usage),
        gpu_usage_percent: round1(sample.gpu_usage),
        ram_usage_percent: round1(sample.ram_usage_percent),
        disk_usage_percent: round1(sample.disk_usage_percent),
        latency_ms: round1(sample.latency_ms),
        jitter_ms: sample.network.jitter_ms.map(round1),
        packet_loss_percent: sample.network.packet_loss_percent.map(round1),
        active_processes: sample.active_processes,
        cleanup_reclaimable_bytes,
        startup_apps: startup_impact.len(),
        high_impact_startup_apps,
        pending_snapshots,
        power_plan: power_plan
            .as_ref()
            .and_then(|plan| plan.scheme_name.clone())
            .or_else(|| power_plan.as_ref().map(|plan| plan.scheme_guid.clone())),
        cpu_temperature_c: sample
            .cpu_temperature_available
            .then_some(round1(sample.cpu_temperature)),
        gpu_temperature_c: sample
            .gpu_temperature_available
            .then_some(round1(sample.gpu_temperature)),
        game_detected: detected_game.detected,
        game_process: detected_game.process_name.clone(),
    };
    let score = score_report(
        &sample,
        &metrics,
        power_plan
            .as_ref()
            .and_then(|plan| plan.scheme_name.as_deref()),
    );
    let previous = previous_report_for_delta(&normalized_mode);
    let score_delta_points = previous
        .as_ref()
        .map(|previous| score_delta_points(previous.overall_score, score.overall_score));
    let score_delta_percent = previous
        .as_ref()
        .map(|previous| score_change_percent(previous.overall_score, score.overall_score));
    let measured_gain_percent = score_delta_percent
        .map(|value| measured_gain_percent_from_delta(score_delta_points, value));
    let deltas = previous
        .as_ref()
        .map(|previous| report_deltas(previous, &metrics, score.overall_score))
        .unwrap_or_default();
    let report = PerformanceReport {
        id: Uuid::new_v4().simple().to_string(),
        device_id,
        generated_at: chrono::Utc::now().timestamp(),
        mode: normalized_mode.clone(),
        overall_score: score.overall_score,
        previous_score: previous.as_ref().map(|report| report.overall_score),
        measured_gain_percent,
        score_delta_percent,
        score_delta_points,
        performance_change: performance_change_label(score_delta_points),
        score_breakdown: score.breakdown,
        metrics,
        deltas,
        actions: Vec::new(),
        bottlenecks: score.bottlenecks,
        restore_session: None,
        source: "local_performance_scan".to_string(),
        metrics_version: "performance-suite-v1".to_string(),
    };

    save_report(report.clone())?;
    if normalized_mode == "baseline" {
        let session = PerformanceSession {
            id: Uuid::new_v4().simple().to_string(),
            baseline_report_id: Some(report.id.clone()),
            after_report_id: None,
            snapshot_ids: Vec::new(),
            status: "baseline".to_string(),
            created_at: report.generated_at,
            restored_at: None,
            actions: Vec::new(),
        };
        let _ = write_json_file(&performance_session_path(), &session);
    } else if normalized_mode == "after" {
        let _ = update_after_report_id(&report.id);
    }

    let _ = audit::record_event(
        "info",
        "performance.scan_completed",
        "Performance Scan local concluido com score medido.",
        json!({
            "report_id": report.id,
            "mode": report.mode,
            "overall_score": report.overall_score,
            "measured_gain_percent": report.measured_gain_percent,
        }),
    );
    Ok(report)
}

fn score_report(
    sample: &TelemetrySample,
    metrics: &PerformanceMetrics,
    power_plan_name: Option<&str>,
) -> ScoreComputation {
    let boot_startup = clamp_score(
        100.0
            - (metrics.startup_apps as f64 * 2.0)
            - (metrics.high_impact_startup_apps as f64 * 7.0)
            - (metrics.pending_snapshots as f64 * 1.5),
    );
    let background = clamp_score(
        100.0
            - (sample.active_processes.saturating_sub(80) as f64 * 0.22)
            - (sample.cpu_usage * 0.28),
    );
    let memory = clamp_score(105.0 - (sample.ram_usage_percent * 0.95));
    let disk = clamp_score(
        105.0
            - (sample.disk_usage_percent * 0.85)
            - bytes_to_gb(metrics.cleanup_reclaimable_bytes) * 1.2,
    );
    let latency_penalty = if sample.latency_ms > 0.0 {
        (sample.latency_ms - 30.0).max(0.0) * 0.45
    } else {
        10.0
    };
    let jitter_penalty = sample.network.jitter_ms.unwrap_or_default() * 0.75;
    let loss_penalty = sample.network.packet_loss_percent.unwrap_or_default() * 8.0;
    let network = clamp_score(100.0 - latency_penalty - jitter_penalty - loss_penalty);
    let plan = power_plan_name.unwrap_or_default().to_ascii_lowercase();
    let energy = if plan.contains("alto") || plan.contains("high") || plan.contains("ultimate") {
        94.0
    } else if plan.contains("econom") || plan.contains("saver") {
        62.0
    } else if plan.is_empty() {
        76.0
    } else {
        82.0
    };
    let cpu_temp_penalty = if sample.cpu_temperature_available {
        (sample.cpu_temperature - 78.0).max(0.0) * 2.0
    } else {
        4.0
    };
    let gpu_temp_penalty = if sample.gpu_temperature_available {
        (sample.gpu_temperature - 82.0).max(0.0) * 1.8
    } else {
        3.0
    };
    let thermal = clamp_score(100.0 - cpu_temp_penalty - gpu_temp_penalty);
    let gaming = metrics.game_detected.then(|| {
        clamp_score(
            100.0
                - (sample.latency_ms - 35.0).max(0.0) * 0.35
                - (sample.ram_usage_percent - 82.0).max(0.0) * 1.1
                - (sample.cpu_usage - 88.0).max(0.0) * 0.7,
        )
    });

    let mut weighted_total = boot_startup * 0.12
        + background * 0.18
        + memory * 0.17
        + disk * 0.15
        + network * 0.12
        + energy * 0.11
        + thermal * 0.15;
    let mut weight = 1.0;
    if let Some(gaming) = gaming {
        weighted_total = weighted_total * 0.82 + gaming * 0.18;
        weight = 1.0;
    }
    let breakdown = ScoreBreakdown {
        boot_startup: round1(boot_startup),
        background: round1(background),
        memory: round1(memory),
        disk: round1(disk),
        network: round1(network),
        energy: round1(energy),
        thermal: round1(thermal),
        gaming: gaming.map(round1),
    };
    let bottlenecks = bottlenecks_from_scores(&breakdown, metrics);

    ScoreComputation {
        overall_score: round1(clamp_score(weighted_total / weight)),
        breakdown,
        bottlenecks,
    }
}

#[derive(Debug)]
struct ScoreComputation {
    overall_score: f64,
    breakdown: ScoreBreakdown,
    bottlenecks: Vec<PerformanceBottleneck>,
}

fn bottlenecks_from_scores(
    breakdown: &ScoreBreakdown,
    metrics: &PerformanceMetrics,
) -> Vec<PerformanceBottleneck> {
    let mut items = Vec::new();
    maybe_bottleneck(
        &mut items,
        "startup",
        "Inicializacao pesada",
        breakdown.boot_startup,
        Some(format!("{} apps", metrics.startup_apps)),
        Some("DELAY_STARTUP_APP"),
    );
    maybe_bottleneck(
        &mut items,
        "background",
        "Apps de fundo disputando CPU",
        breakdown.background,
        Some(format!("{} processos", metrics.active_processes)),
        Some("APPLY_PC_CLEAN_FAST_BACKGROUND_PRIORITIES"),
    );
    maybe_bottleneck(
        &mut items,
        "memory",
        "Memoria pressionada",
        breakdown.memory,
        Some(format!("{}% RAM", metrics.ram_usage_percent)),
        Some("APPLY_PC_CLEAN_FAST_PROFILE"),
    );
    maybe_bottleneck(
        &mut items,
        "disk",
        "Disco cheio ou caches acumulados",
        breakdown.disk,
        Some(format!(
            "{} GB elegiveis",
            round1(bytes_to_gb(metrics.cleanup_reclaimable_bytes))
        )),
        Some("APPLY_CLEANUP_CATEGORY"),
    );
    maybe_bottleneck(
        &mut items,
        "network",
        "Rede instavel",
        breakdown.network,
        Some(format!("{} ms", metrics.latency_ms)),
        Some("NETWORK_DIAGNOSTICS"),
    );
    maybe_bottleneck(
        &mut items,
        "energy",
        "Plano de energia nao ideal",
        breakdown.energy,
        metrics.power_plan.clone(),
        Some("SET_POWER_PLAN_HIGH_PERFORMANCE"),
    );
    maybe_bottleneck(
        &mut items,
        "thermal",
        "Temperatura alta ou sensor ausente",
        breakdown.thermal,
        metrics
            .cpu_temperature_c
            .or(metrics.gpu_temperature_c)
            .map(|value| format!("{value} C")),
        None,
    );
    if let Some(gaming) = breakdown.gaming {
        maybe_bottleneck(
            &mut items,
            "gaming",
            "Jogo ativo com margem para priorizacao",
            gaming,
            metrics.game_process.clone(),
            Some("APPLY_GAME_MODE"),
        );
    }
    items.sort_by(|left, right| left.score.total_cmp(&right.score));
    items.truncate(6);
    items
}

fn maybe_bottleneck(
    items: &mut Vec<PerformanceBottleneck>,
    id: &str,
    label: &str,
    score: f64,
    metric: Option<String>,
    recommended_action: Option<&str>,
) {
    if score >= 72.0 {
        return;
    }
    items.push(PerformanceBottleneck {
        id: id.to_string(),
        label: label.to_string(),
        severity: if score < 45.0 {
            "high".to_string()
        } else if score < 62.0 {
            "medium".to_string()
        } else {
            "low".to_string()
        },
        score,
        metric,
        recommended_action: recommended_action.map(ToString::to_string),
    });
}

fn scan_cleanup_categories_blocking() -> Vec<CleanupCategory> {
    let policy = local_ai_policy::load_local_ai_policy();
    let mut categories = Vec::new();
    for spec in cleanup_category_specs(&policy) {
        let mut summary = CategoryScanSummary::default();
        let mut paths = Vec::new();
        for path in spec.targets.iter() {
            if !path.exists() {
                continue;
            }
            paths.push(path.display().to_string());
            scan_reclaimable_dir(path, path, spec.min_age, spec.id, &mut summary);
        }
        let skipped_reason = if paths.is_empty() {
            Some("path_not_found".to_string())
        } else if summary.capped {
            Some("scan_limit_reached".to_string())
        } else {
            None
        };
        categories.push(CleanupCategory {
            id: spec.id.to_string(),
            label: spec.label.to_string(),
            reclaimable_bytes: summary.reclaimable_bytes,
            scanned_paths: paths,
            risk: spec.risk.to_string(),
            requires_helper: spec.requires_helper,
            reversible: spec.reversible,
            available_actions: spec
                .actions
                .iter()
                .map(|action| action.to_string())
                .collect(),
            skipped_reason,
        });
    }

    categories
}

#[derive(Debug)]
struct CleanupCategorySpec {
    id: &'static str,
    label: &'static str,
    targets: Vec<PathBuf>,
    min_age: Duration,
    risk: &'static str,
    requires_helper: bool,
    reversible: bool,
    actions: &'static [&'static str],
}

/// Grouped by risk tier rather than one setting per category (9 individual
/// sliders would be noise for most users) - each tier's minutes come from
/// LocalAiPolicy, user-tunable in Settings, falling back to the same
/// defaults this used to hardcode per-category.
fn cleanup_category_specs(policy: &LocalAiPolicy) -> Vec<CleanupCategorySpec> {
    let temp_min_age = Duration::from_secs(policy.cleanup_temp_min_age_minutes * 60);
    let cache_min_age = Duration::from_secs(policy.cleanup_cache_min_age_minutes * 60);
    let system_min_age = Duration::from_secs(policy.cleanup_system_min_age_minutes * 60);

    vec![
        CleanupCategorySpec {
            id: "user_temp",
            label: "%TEMP% do usuario",
            targets: user_temp_targets(),
            min_age: temp_min_age,
            risk: "safe",
            requires_helper: false,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "windows_temp",
            label: "%WINDIR%\\Temp",
            targets: windows_temp_targets(),
            min_age: temp_min_age,
            risk: "sensitive",
            requires_helper: true,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "directx_shader_cache",
            label: "DirectX/GPU shader cache",
            targets: shader_cache_targets(),
            min_age: cache_min_age,
            risk: "safe",
            requires_helper: false,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "thumbnail_cache",
            label: "Cache de miniaturas do Explorer",
            targets: thumbnail_cache_targets(),
            min_age: cache_min_age,
            risk: "safe",
            requires_helper: false,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "crash_dumps",
            label: "Crash dumps locais antigos",
            targets: crash_dump_targets(),
            min_age: system_min_age,
            risk: "safe",
            requires_helper: false,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "browser_cache",
            label: "Cache de navegadores",
            targets: browser_cache_targets(),
            min_age: cache_min_age,
            risk: "safe",
            requires_helper: false,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "windows_update_cache",
            label: "Cache de downloads do Windows Update",
            targets: windows_update_cache_targets(),
            min_age: system_min_age,
            risk: "sensitive",
            requires_helper: true,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "delivery_optimization_cache",
            label: "Cache de Delivery Optimization",
            targets: delivery_optimization_cache_targets(),
            min_age: system_min_age,
            risk: "sensitive",
            requires_helper: true,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "memory_dumps",
            label: "Dumps de memoria do Windows",
            targets: memory_dump_targets(),
            min_age: system_min_age,
            risk: "sensitive",
            requires_helper: true,
            reversible: true,
            actions: &["scan", "apply", "restore"],
        },
        CleanupCategorySpec {
            id: "cleanup_quarantine",
            label: "Quarentena AnalystBlaze",
            targets: vec![snapshot::cleanup_quarantine_root()],
            min_age: Duration::from_secs(0),
            risk: "sensitive",
            requires_helper: false,
            reversible: false,
            actions: &["scan", "purge"],
        },
    ]
}

fn apply_cleanup_category_blocking(category: &str, mode: &str) -> ExecutionResult {
    let policy = local_ai_policy::load_local_ai_policy();
    let Some(spec) = cleanup_category_specs(&policy)
        .into_iter()
        .find(|spec| spec.id == category)
    else {
        return ExecutionResult {
            success: false,
            message: "Categoria de limpeza desconhecida.".to_string(),
            details: json!({
                "implemented": true,
                "category": category,
                "allowed": cleanup_category_specs(&policy)
                    .iter()
                    .map(|spec| spec.id)
                    .collect::<Vec<_>>(),
            }),
        };
    };

    let min_age = if mode == "deep_confirmed" {
        Duration::from_secs(5 * 60)
    } else {
        spec.min_age
    };
    let snapshot_id = Uuid::new_v4().simple().to_string();
    let quarantine_root = snapshot::cleanup_quarantine_dir(&snapshot_id).join(spec.id);
    let mut summary = CategoryApplySummary::default();
    let mut targets = Vec::new();

    for (index, target) in spec.targets.iter().enumerate() {
        if !target.exists() {
            continue;
        }
        targets.push(target.display().to_string());
        quarantine_category_dir(
            target,
            target,
            &quarantine_root.join(format!("target-{index}")),
            min_age,
            spec.id,
            &mut summary,
        );
    }

    if summary.entries.is_empty() {
        return ExecutionResult::ok(
            "Nenhum arquivo elegivel encontrado para esta categoria.",
            json!({
                "implemented": true,
                "category": spec.id,
                "mode": mode,
                "targets": targets,
                "scannedFiles": summary.scanned_files,
                "scannedDirs": summary.scanned_dirs,
                "skippedRecent": summary.skipped_recent,
                "failedEntries": summary.failed_entries,
                "snapshot": null,
            }),
        );
    }

    let snapshot = OptimizationSnapshot {
        id: snapshot_id,
        action_name: "APPLY_CLEANUP_CATEGORY".to_string(),
        created_at: chrono::Utc::now().timestamp(),
        restored_at: None,
        entries: summary.entries.clone(),
        details: json!({
            "category": spec.id,
            "mode": mode,
            "targets": targets,
            "scannedFiles": summary.scanned_files,
            "scannedDirs": summary.scanned_dirs,
            "quarantinedFiles": summary.quarantined_files,
            "quarantinedBytes": summary.quarantined_bytes,
            "removedEmptyDirs": summary.removed_empty_dirs,
            "skippedRecent": summary.skipped_recent,
            "skippedSpecial": summary.skipped_special,
            "failedEntries": summary.failed_entries,
        }),
    };

    match snapshot::save_snapshot(&snapshot) {
        Ok(()) => ExecutionResult::ok(
            "Categoria de limpeza movida para quarentena reversivel.",
            json!({
                "implemented": true,
                "category": spec.id,
                "mode": mode,
                "targets": targets,
                "quarantinedFiles": summary.quarantined_files,
                "quarantinedBytes": summary.quarantined_bytes,
                "failedEntries": summary.failed_entries,
                "snapshot": {
                    "id": snapshot.id,
                    "entries": snapshot.entries.len(),
                    "reversible": true,
                    "spaceReclaimPending": true,
                },
            }),
        ),
        Err(error) => {
            let rollback = snapshot::restore_snapshot_entries(&snapshot);
            ExecutionResult {
                success: false,
                message: "Limpeza revertida porque o snapshot nao pode ser salvo.".to_string(),
                details: json!({
                    "implemented": true,
                    "category": spec.id,
                    "snapshotError": error,
                    "rollback": {
                        "restoredEntries": rollback.restored_entries,
                        "failedEntries": rollback.failed_entries,
                        "skippedConflicts": rollback.skipped_conflicts,
                        "messages": rollback.messages,
                    },
                }),
            }
        }
    }
}

fn scan_reclaimable_dir(
    root: &Path,
    dir: &Path,
    min_age: Duration,
    category: &str,
    summary: &mut CategoryScanSummary,
) {
    if summary.scanned_files + summary.scanned_dirs >= CATEGORY_SCAN_LIMIT {
        summary.capped = true;
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        summary.failed_entries += 1;
        return;
    };

    for entry in entries.flatten() {
        if summary.scanned_files + summary.scanned_dirs >= CATEGORY_SCAN_LIMIT {
            summary.capped = true;
            return;
        }
        let path = entry.path();
        if !path_stays_inside(root, &path) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            summary.failed_entries += 1;
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            summary.scanned_dirs += 1;
            scan_reclaimable_dir(root, &path, min_age, category, summary);
        } else if metadata.is_file() && category_file_allowed(category, &path) {
            summary.scanned_files += 1;
            if is_old_enough(&metadata, min_age) {
                summary.reclaimable_bytes =
                    summary.reclaimable_bytes.saturating_add(metadata.len());
            } else {
                summary.skipped_recent += 1;
            }
        } else if metadata.is_file() {
            summary.scanned_files += 1;
        }
    }
}

fn quarantine_category_dir(
    root: &Path,
    dir: &Path,
    quarantine_root: &Path,
    min_age: Duration,
    category: &str,
    summary: &mut CategoryApplySummary,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        summary.failed_entries += 1;
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path_stays_inside(root, &path) || path_stays_inside(quarantine_root, &path) {
            summary.skipped_special += 1;
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            summary.failed_entries += 1;
            continue;
        };
        if metadata.file_type().is_symlink() {
            summary.skipped_special += 1;
            continue;
        }
        if metadata.is_dir() {
            summary.scanned_dirs += 1;
            quarantine_category_dir(root, &path, quarantine_root, min_age, category, summary);
            if is_old_enough(&metadata, min_age) && fs::remove_dir(&path).is_ok() {
                summary.removed_empty_dirs += 1;
                summary.entries.push(SnapshotEntry::RemovedEmptyDir {
                    original_path: path,
                });
            }
            continue;
        }
        if !metadata.is_file() || !category_file_allowed(category, &path) {
            summary.skipped_special += 1;
            continue;
        }
        summary.scanned_files += 1;
        if !is_old_enough(&metadata, min_age) {
            summary.skipped_recent += 1;
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            summary.failed_entries += 1;
            continue;
        };
        let quarantine_path = quarantine_root.join(relative);
        if let Some(parent) = quarantine_path.parent() {
            if fs::create_dir_all(parent).is_err() {
                summary.failed_entries += 1;
                continue;
            }
        }
        let len = metadata.len();
        match snapshot::move_file_across_volumes(&path, &quarantine_path) {
            Ok(()) => {
                summary.quarantined_files += 1;
                summary.quarantined_bytes = summary.quarantined_bytes.saturating_add(len);
                summary.entries.push(SnapshotEntry::QuarantinedPath {
                    original_path: path,
                    quarantine_path,
                    bytes: len,
                });
            }
            Err(_) => summary.failed_entries += 1,
        }
    }
}

fn category_file_allowed(category: &str, path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match category {
        "thumbnail_cache" => {
            name.starts_with("thumbcache") || name.starts_with("iconcache") || name.ends_with(".db")
        }
        "crash_dumps" => {
            name.ends_with(".dmp")
                || name.ends_with(".mdmp")
                || name.ends_with(".hdmp")
                || name.ends_with(".wer")
        }
        "memory_dumps" => {
            name.ends_with(".dmp")
                || name.ends_with(".mdmp")
                || name.ends_with(".hdmp")
                || name.eq_ignore_ascii_case("memory.dmp")
        }
        _ => true,
    }
}

fn scan_startup_impact_blocking() -> Vec<StartupImpact> {
    let inventory = windows_inventory::collect_windows_inventory();
    let mut impacts = inventory
        .startup_apps
        .into_iter()
        .map(startup_impact_from_app)
        .collect::<Vec<_>>();
    impacts.sort_by(|left, right| right.impact_score.total_cmp(&left.impact_score));
    impacts
}

fn startup_impact_from_app(app: windows_inventory::StartupApp) -> StartupImpact {
    let normalized = format!(
        "{} {}",
        app.name.to_ascii_lowercase(),
        app.command.to_ascii_lowercase()
    );
    let mut score: f64 = 22.0;
    for (needle, weight) in [
        ("discord", 18.0),
        ("steam", 16.0),
        ("epic", 14.0),
        ("spotify", 12.0),
        ("teams", 16.0),
        ("slack", 14.0),
        ("onedrive", 14.0),
        ("dropbox", 14.0),
        ("google drive", 14.0),
        ("adobe", 14.0),
        ("launcher", 10.0),
        ("updater", 8.0),
    ] {
        if normalized.contains(needle) {
            score += weight;
        }
    }
    if app.location.starts_with("HKLM") {
        score += 10.0;
    }
    if app.command.len() > 160 {
        score += 6.0;
    }
    if app.risk != "safe" {
        score = score.min(38.0);
    }
    let recommendation = if app.risk != "safe" {
        "keep"
    } else if score >= 40.0 {
        "delay"
    } else if score >= 32.0 {
        "observe"
    } else {
        "keep"
    };
    let mut available_actions = vec!["keep".to_string()];
    if app.risk == "safe" {
        available_actions.push("delay".to_string());
        available_actions.push("disable".to_string());
    }

    StartupImpact {
        name: app.name,
        location: app.location,
        publisher: None,
        command_preview: app.command.chars().take(220).collect(),
        impact_score: round1(clamp_score(score)),
        risk: app.risk,
        recommendation: recommendation.to_string(),
        available_actions,
    }
}

fn append_action_result(
    actions: &mut Vec<PerformanceActionSummary>,
    snapshot_ids: &mut Vec<String>,
    action_name: &str,
    result: &ExecutionResult,
) {
    let snapshot_id = result
        .details
        .pointer("/snapshot/id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    if let Some(snapshot_id) = snapshot_id.clone() {
        snapshot_ids.push(snapshot_id);
    }
    actions.push(PerformanceActionSummary {
        action_name: action_name.to_string(),
        status: if result.success { "applied" } else { "failed" }.to_string(),
        message: result.message.clone(),
        reversible: snapshot_id.is_some(),
        snapshot_id,
        impact_score: if result.success { 1.0 } else { 0.0 },
    });
}

fn report_deltas(
    previous: &PerformanceReport,
    metrics: &PerformanceMetrics,
    score: f64,
) -> Vec<PerformanceDelta> {
    vec![
        PerformanceDelta {
            key: "overallScore".to_string(),
            before: Some(previous.overall_score),
            after: Some(score),
            unit: "score".to_string(),
            direction: "higher_is_better".to_string(),
        },
        PerformanceDelta {
            key: "ramUsagePercent".to_string(),
            before: Some(previous.metrics.ram_usage_percent),
            after: Some(metrics.ram_usage_percent),
            unit: "percent".to_string(),
            direction: "lower_is_better".to_string(),
        },
        PerformanceDelta {
            key: "diskUsagePercent".to_string(),
            before: Some(previous.metrics.disk_usage_percent),
            after: Some(metrics.disk_usage_percent),
            unit: "percent".to_string(),
            direction: "lower_is_better".to_string(),
        },
        PerformanceDelta {
            key: "latencyMs".to_string(),
            before: Some(previous.metrics.latency_ms),
            after: Some(metrics.latency_ms),
            unit: "ms".to_string(),
            direction: "lower_is_better".to_string(),
        },
        PerformanceDelta {
            key: "activeProcesses".to_string(),
            before: Some(previous.metrics.active_processes as f64),
            after: Some(metrics.active_processes as f64),
            unit: "count".to_string(),
            direction: "lower_is_better".to_string(),
        },
    ]
}

fn previous_report_for_delta(mode: &str) -> Option<PerformanceReport> {
    if mode == "after" {
        let session = read_json_file::<PerformanceSession>(&performance_session_path()).ok()?;
        let baseline_id = session.baseline_report_id?;
        return reports_history()
            .ok()?
            .into_iter()
            .find(|report| report.id == baseline_id);
    }
    None
}

fn save_report(report: PerformanceReport) -> Result<(), String> {
    let mut reports = reports_history().unwrap_or_default();
    reports.retain(|existing| existing.id != report.id);
    reports.push(report);
    reports.sort_by_key(|report| std::cmp::Reverse(report.generated_at));
    reports.truncate(REPORT_HISTORY_LIMIT);
    write_json_file(&performance_reports_path(), &reports)
}

fn reports_history() -> Result<Vec<PerformanceReport>, String> {
    read_json_file(&performance_reports_path()).or_else(|error| {
        if performance_reports_path().exists() {
            Err(error)
        } else {
            Ok(Vec::new())
        }
    })
}

fn update_after_report_id(report_id: &str) -> Result<(), String> {
    let path = performance_session_path();
    let mut session = read_json_file::<PerformanceSession>(&path)?;
    session.after_report_id = Some(report_id.to_string());
    session.status = "after".to_string();
    write_json_file(&path, &session)
}

fn restore_delayed_startup_app_blocking(name: Option<String>) -> ExecutionResult {
    let target = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut records = read_delayed_startup_records().unwrap_or_default();
    let matching = records
        .iter()
        .filter(|record| {
            target
                .map(|target| record.name.eq_ignore_ascii_case(target))
                .unwrap_or(true)
        })
        .map(|record| record.name.clone())
        .collect::<Vec<_>>();
    let restore = snapshot::restore_startup_app_snapshots(target);
    match restore {
        Ok(report) => {
            records.retain(|record| {
                target
                    .map(|target| !record.name.eq_ignore_ascii_case(target))
                    .unwrap_or(false)
            });
            let _ = write_delayed_startup_records(&records);
            let success = report.failed_snapshots == 0 && report.failed_entries == 0;
            ExecutionResult {
                success,
                message: if success {
                    "Inicializacao atrasada restaurada para o Registro do Windows.".to_string()
                } else {
                    "Restauracao da inicializacao atrasada concluiu com falhas.".to_string()
                },
                details: json!({
                    "implemented": true,
                    "target": target,
                    "matched": matching,
                    "restore": report,
                }),
            }
        }
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar inicializacao atrasada: {error}"),
            details: json!({
                "implemented": true,
                "target": target,
                "matched": matching,
            }),
        },
    }
}

fn upsert_delayed_startup_record(record: DelayedStartupRecord) -> Result<(), String> {
    let mut records = read_delayed_startup_records().unwrap_or_default();
    records.retain(|item| {
        !(item.name.eq_ignore_ascii_case(&record.name)
            && item.location.eq_ignore_ascii_case(&record.location))
    });
    records.push(record);
    write_delayed_startup_records(&records)
}

fn read_delayed_startup_records() -> Result<Vec<DelayedStartupRecord>, String> {
    read_json_file(&delayed_startup_path()).or_else(|error| {
        if delayed_startup_path().exists() {
            Err(error)
        } else {
            Ok(Vec::new())
        }
    })
}

fn write_delayed_startup_records(records: &[DelayedStartupRecord]) -> Result<(), String> {
    write_json_file(&delayed_startup_path(), records)
}

fn parse_launch_command(command: &str) -> Option<(PathBuf, Vec<String>)> {
    let expanded = expand_env_vars(command.trim());
    let parts = split_command_line(&expanded);
    let program = PathBuf::from(parts.first()?.trim_matches('"'));
    if !program.is_file() {
        return None;
    }
    let executable = program
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("exe"))
        .unwrap_or(false);
    if !executable {
        return None;
    }
    Some((program, parts.into_iter().skip(1).collect()))
}

fn split_command_line(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in value.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            continue;
        }
        if ch.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                parts.push(current.clone());
                current.clear();
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn expand_env_vars(value: &str) -> String {
    let mut output = value.to_string();
    for (key, val) in std::env::vars() {
        let pattern = format!("%{key}%");
        if output.contains(&pattern) {
            output = output.replace(&pattern, &val);
        }
    }
    output
}

fn user_temp_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    push_unique_dir(&mut targets, std::env::temp_dir());
    for key in ["TEMP", "TMP"] {
        if let Some(path) = std::env::var_os(key).map(PathBuf::from) {
            push_unique_dir(&mut targets, path);
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        push_unique_dir(&mut targets, local_app_data.join("Temp"));
    }
    targets
}

fn windows_temp_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(path) = std::env::var_os(key).map(PathBuf::from) {
            push_unique_dir(&mut targets, path.join("Temp"));
        }
    }
    targets
}

fn shader_cache_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        for relative in [
            "D3DSCache",
            "NVIDIA\\DXCache",
            "NVIDIA\\GLCache",
            "AMD\\DxCache",
            "AMD\\GLCache",
            "Intel\\ShaderCache",
        ] {
            push_unique_dir(&mut targets, local_app_data.join(relative));
        }
    }
    targets
}

fn thumbnail_cache_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        push_unique_dir(
            &mut targets,
            local_app_data.join("Microsoft\\Windows\\Explorer"),
        );
    }
    targets
}

fn crash_dump_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        push_unique_dir(&mut targets, local_app_data.join("CrashDumps"));
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA").map(PathBuf::from) {
        push_unique_dir(
            &mut targets,
            program_data.join("Microsoft\\Windows\\WER\\ReportArchive"),
        );
        push_unique_dir(
            &mut targets,
            program_data.join("Microsoft\\Windows\\WER\\ReportQueue"),
        );
    }
    targets
}

fn browser_cache_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        for user_data in [
            local_app_data.join("Google\\Chrome\\User Data"),
            local_app_data.join("Microsoft\\Edge\\User Data"),
            local_app_data.join("BraveSoftware\\Brave-Browser\\User Data"),
            local_app_data.join("Vivaldi\\User Data"),
        ] {
            push_chromium_cache_profile_dirs(&mut targets, &user_data);
        }
        push_unique_dir(
            &mut targets,
            local_app_data.join("Opera Software\\Opera Stable\\Cache"),
        );
        push_unique_dir(
            &mut targets,
            local_app_data.join("Opera Software\\Opera Stable\\Code Cache"),
        );
        push_unique_dir(
            &mut targets,
            local_app_data.join("Opera Software\\Opera GX Stable\\Cache"),
        );
        push_unique_dir(
            &mut targets,
            local_app_data.join("Opera Software\\Opera GX Stable\\Code Cache"),
        );
    }
    if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
        push_firefox_cache_profile_dirs(&mut targets, &app_data.join("Mozilla\\Firefox\\Profiles"));
    }
    targets
}

fn push_chromium_cache_profile_dirs(targets: &mut Vec<PathBuf>, user_data: &Path) {
    let Ok(entries) = fs::read_dir(user_data) else {
        return;
    };
    for entry in entries.flatten() {
        let profile = entry.path();
        if !profile.is_dir() || !is_browser_profile_dir(&profile) {
            continue;
        }
        for relative in ["Cache", "Code Cache", "GPUCache", "GrShaderCache"] {
            push_unique_dir(targets, profile.join(relative));
        }
    }
}

fn push_firefox_cache_profile_dirs(targets: &mut Vec<PathBuf>, profiles_root: &Path) {
    let Ok(entries) = fs::read_dir(profiles_root) else {
        return;
    };
    for entry in entries.flatten() {
        let profile = entry.path();
        if !profile.is_dir() {
            continue;
        }
        push_unique_dir(targets, profile.join("cache2"));
        push_unique_dir(targets, profile.join("startupCache"));
    }
}

fn is_browser_profile_dir(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name == "default"
        || name == "guest profile"
        || name.starts_with("profile ")
        || name.starts_with("system profile")
}

fn windows_update_cache_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(windows_dir) = std::env::var_os(key).map(PathBuf::from) {
            push_unique_dir(
                &mut targets,
                windows_dir.join("SoftwareDistribution\\Download"),
            );
        }
    }
    targets
}

fn delivery_optimization_cache_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    if let Some(program_data) = std::env::var_os("PROGRAMDATA").map(PathBuf::from) {
        push_unique_dir(
            &mut targets,
            program_data.join("Microsoft\\Windows\\DeliveryOptimization\\Cache"),
        );
    }
    targets
}

fn memory_dump_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(windows_dir) = std::env::var_os(key).map(PathBuf::from) {
            push_unique_dir(&mut targets, windows_dir.join("Minidump"));
            push_unique_dir(&mut targets, windows_dir.join("LiveKernelReports"));
        }
    }
    targets
}

fn push_unique_dir(targets: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.is_dir() {
        return;
    }
    let canonical = fs::canonicalize(&path).unwrap_or(path.clone());
    if !targets.iter().any(|existing| {
        fs::canonicalize(existing)
            .unwrap_or_else(|_| existing.clone())
            .eq(&canonical)
    }) {
        targets.push(path);
    }
}

fn path_stays_inside(root: &Path, path: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else {
        return false;
    };
    let candidate = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    candidate.starts_with(root)
}

fn is_old_enough(metadata: &fs::Metadata, min_age: Duration) -> bool {
    if min_age.is_zero() {
        return true;
    }
    let modified = metadata
        .modified()
        .or_else(|_| metadata.created())
        .unwrap_or(SystemTime::now());
    modified
        .elapsed()
        .map(|age| age >= min_age)
        .unwrap_or(false)
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn write_json_file<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn performance_reports_path() -> PathBuf {
    snapshot::app_data_dir().join("performance-reports.json")
}

fn performance_session_path() -> PathBuf {
    snapshot::app_data_dir().join("performance-session.json")
}

fn delayed_startup_path() -> PathBuf {
    snapshot::app_data_dir().join("delayed-startup.json")
}

fn normalize_scan_mode(mode: &str) -> String {
    match mode.trim().to_ascii_lowercase().as_str() {
        "baseline" => "baseline".to_string(),
        "after" => "after".to_string(),
        _ => "quick".to_string(),
    }
}

fn score_change_percent(before: f64, after: f64) -> f64 {
    if before <= 0.0 {
        return 0.0;
    }
    round1(((after - before) / before) * 100.0)
}

fn score_delta_points(before: f64, after: f64) -> f64 {
    round1(after - before)
}

fn measured_gain_percent_from_delta(delta_points: Option<f64>, delta_percent: f64) -> f64 {
    if delta_points.unwrap_or_default() < 1.0 || delta_percent <= 0.0 {
        return 0.0;
    }
    delta_percent
}

fn performance_change_label(delta_points: Option<f64>) -> String {
    match delta_points {
        Some(value) if value >= 1.0 => "improved",
        Some(value) if value <= -1.0 => "regressed",
        Some(_) => "stable",
        None => "unknown",
    }
    .to_string()
}

fn clamp_score(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn default_true() -> bool {
    true
}

fn current_boot_id() -> i64 {
    chrono::Utc::now().timestamp() - System::uptime() as i64
}

fn unknown_performance_change() -> String {
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        measured_gain_percent_from_delta, performance_change_label, score_change_percent,
        score_delta_points, split_command_line, startup_impact_from_app,
        windows_inventory::StartupApp,
    };

    #[test]
    fn computes_measured_gain_from_score_delta() {
        assert_eq!(score_change_percent(70.0, 77.0), 10.0);
        assert_eq!(score_change_percent(77.0, 70.0), -9.1);
        assert_eq!(score_change_percent(0.0, 77.0), 0.0);
        assert_eq!(score_delta_points(70.0, 77.0), 7.0);
        assert_eq!(measured_gain_percent_from_delta(Some(7.0), 10.0), 10.0);
        assert_eq!(measured_gain_percent_from_delta(Some(-7.0), -9.1), 0.0);
        assert_eq!(measured_gain_percent_from_delta(Some(0.6), 0.8), 0.0);
        assert_eq!(performance_change_label(Some(7.0)), "improved");
        assert_eq!(performance_change_label(Some(-7.0)), "regressed");
        assert_eq!(performance_change_label(Some(0.6)), "stable");
        assert_eq!(performance_change_label(None), "unknown");
    }

    #[test]
    fn parses_simple_quoted_startup_command() {
        let parts = split_command_line("\"C:\\Program Files\\App\\app.exe\" --silent");
        assert_eq!(parts[0], "C:\\Program Files\\App\\app.exe");
        assert_eq!(parts[1], "--silent");
    }

    #[test]
    fn recommends_delay_for_safe_heavy_startup_app() {
        let impact = startup_impact_from_app(StartupApp {
            name: "Discord".to_string(),
            command:
                "C:\\Users\\me\\AppData\\Local\\Discord\\Update.exe --processStart Discord.exe"
                    .to_string(),
            location: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".to_string(),
            risk: "safe".to_string(),
        });

        assert_eq!(impact.recommendation, "delay");
        assert!(impact.available_actions.contains(&"delay".to_string()));
    }
}
