//! Owns the actual PresentMon.exe child process for a ground-truth frame
//! capture - spawning it, reading its stdout incrementally, and stopping
//! it. See `telemetry::frame_capture` for the pure data/statistics half;
//! this module is the half that needs elevation (starting an ETW trace
//! session requires it), which is why it's only ever invoked through the
//! privileged helper (see `optimizations::privileged_helper`), never from
//! the main app process directly.
//!
//! `START_FRAME_CAPTURE` / `STOP_FRAME_CAPTURE` are a pair sharing state
//! across two separate one-shot helper requests, because the helper's
//! request/response protocol isn't built for a long-running streaming call
//! (see privileged_helper.rs's `HelperCommandRequest`/`HelperCommandResponse`).
//! START spawns the child process plus a background reader thread, stashes
//! both behind a generated `captureId` in `ACTIVE_CAPTURES`, and returns
//! immediately. STOP looks the capture up by that id, kills the child, and
//! reduces whatever samples the reader thread collected into a
//! `FrameTimeStats`. Both the child handle and its samples live only in the
//! helper process's own memory - if the helper service restarts mid-capture
//! the capture is lost rather than recovered, the same risk
//! STOP_SERVICE/RESTORE_SERVICE already accept for state that only exists
//! in memory between two calls.
//!
//! The exact PresentMon CLI flags below (`--process_id`, `--output_stdout`,
//! ...) match the v2.x GNU-style syntax documented at
//! https://github.com/GameTechDev/PresentMon - verify against whichever
//! release actually gets bundled (`tauri.conf.json`'s `bundle.resources`;
//! not wired up yet - see this module's own TODO) before shipping, since
//! PresentMon's flag syntax changed between its 1.x and 2.x lines.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::ExecutionResult;
use crate::telemetry::frame_capture::FrameSample;

/// A capture nobody ever stopped (crashed caller, helper killed mid-game)
/// is dropped after this long instead of leaking a PresentMon process - and
/// its ETW session - forever. Long enough to comfortably outlast any real
/// play session; short enough that an abandoned one doesn't linger all day.
const MAX_CAPTURE_SECONDS: i64 = 60 * 60 * 3;

struct ActiveCapture {
    child: std::process::Child,
    samples: Arc<Mutex<Vec<FrameSample>>>,
    started_at: i64,
}

static ACTIVE_CAPTURES: OnceLock<Mutex<HashMap<String, ActiveCapture>>> = OnceLock::new();

fn active_captures() -> &'static Mutex<HashMap<String, ActiveCapture>> {
    ACTIVE_CAPTURES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Removes any capture that's been running past MAX_CAPTURE_SECONDS.
/// Called opportunistically on every start/stop instead of running its own
/// timer thread, matching this file's otherwise fully request-driven shape.
fn reap_stale_captures(captures: &mut HashMap<String, ActiveCapture>) {
    let now = chrono::Utc::now().timestamp();
    let stale_ids: Vec<String> = captures
        .iter()
        .filter(|(_, capture)| now.saturating_sub(capture.started_at) > MAX_CAPTURE_SECONDS)
        .map(|(id, _)| id.clone())
        .collect();
    for id in stale_ids {
        if let Some(mut capture) = captures.remove(&id) {
            let _ = capture.child.kill();
            let _ = capture.child.wait();
        }
    }
}

pub async fn start_frame_capture(payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(move || start_frame_capture_sync(payload)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao iniciar captura de frames: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn stop_frame_capture(payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(move || stop_frame_capture_sync(payload)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao parar captura de frames: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

#[cfg(windows)]
fn presentmon_path() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let dir = exe
        .parent()
        .ok_or_else(|| "Nao foi possivel resolver o diretorio de instalacao.".to_string())?;
    let candidate = dir.join("PresentMon.exe");
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(format!(
            "PresentMon.exe nao encontrado em {}",
            candidate.display()
        ))
    }
}

#[cfg(windows)]
fn start_frame_capture_sync(payload: Option<Value>) -> ExecutionResult {
    use crate::process_ext::CommandExt;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::thread;

    let target_pid = payload
        .as_ref()
        .and_then(|value| value.get("targetPid").or_else(|| value.get("target_pid")))
        .and_then(Value::as_u64);
    let Some(target_pid) = target_pid else {
        return ExecutionResult {
            success: false,
            message: "Informe o PID do processo a capturar.".to_string(),
            details: json!({ "implemented": true }),
        };
    };

    let presentmon = match presentmon_path() {
        Ok(path) => path,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "PresentMon nao esta instalado nesta maquina.".to_string(),
                details: json!({ "implemented": true, "error": error }),
            };
        }
    };

    let mut command = Command::new(&presentmon);
    command
        .args([
            "--process_id",
            &target_pid.to_string(),
            "--output_stdout",
            "--stop_existing_session",
            "--terminate_on_proc_exit",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .no_window();

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel iniciar o PresentMon.".to_string(),
                details: json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    };

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        return ExecutionResult {
            success: false,
            message: "PresentMon iniciou sem stdout capturavel.".to_string(),
            details: json!({ "implemented": true }),
        };
    };

    let samples: Arc<Mutex<Vec<FrameSample>>> = Arc::new(Mutex::new(Vec::new()));
    let reader_samples = Arc::clone(&samples);
    thread::spawn(move || {
        let mut parser = crate::telemetry::frame_capture::PresentMonLineParser::new();
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(sample) = parser.feed_line(&line) {
                if let Ok(mut samples) = reader_samples.lock() {
                    samples.push(sample);
                }
            }
        }
    });

    let capture_id = uuid::Uuid::new_v4().simple().to_string();
    let started_at = chrono::Utc::now().timestamp();
    let Ok(mut captures) = active_captures().lock() else {
        let _ = child.kill();
        return ExecutionResult {
            success: false,
            message: "Estado interno de captura de frames indisponivel.".to_string(),
            details: json!({ "implemented": true }),
        };
    };
    reap_stale_captures(&mut captures);
    captures.insert(
        capture_id.clone(),
        ActiveCapture {
            child,
            samples,
            started_at,
        },
    );

    ExecutionResult::ok(
        "Captura de frames iniciada via PresentMon.",
        json!({
            "implemented": true,
            "captureId": capture_id,
            "targetPid": target_pid,
            "startedAt": started_at,
        }),
    )
}

#[cfg(windows)]
fn stop_frame_capture_sync(payload: Option<Value>) -> ExecutionResult {
    use std::thread;
    use std::time::Duration;

    let capture_id = payload
        .as_ref()
        .and_then(|value| value.get("captureId").or_else(|| value.get("capture_id")))
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(capture_id) = capture_id else {
        return ExecutionResult {
            success: false,
            message: "Informe o captureId retornado por START_FRAME_CAPTURE.".to_string(),
            details: json!({ "implemented": true }),
        };
    };

    let Ok(mut captures) = active_captures().lock() else {
        return ExecutionResult {
            success: false,
            message: "Estado interno de captura de frames indisponivel.".to_string(),
            details: json!({ "implemented": true }),
        };
    };
    reap_stale_captures(&mut captures);
    let Some(mut capture) = captures.remove(&capture_id) else {
        return ExecutionResult {
            success: false,
            message: "Nenhuma captura de frames ativa com esse captureId.".to_string(),
            details: json!({ "implemented": true, "captureId": capture_id }),
        };
    };
    drop(captures);

    let _ = capture.child.kill();
    let _ = capture.child.wait();

    // The reader thread may still be mid-line right after kill() - a short
    // grace period lets it drain whatever PresentMon already flushed to the
    // pipe before it sees EOF, instead of racing it for the last few frames.
    thread::sleep(Duration::from_millis(200));

    let samples = capture
        .samples
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let stats = crate::telemetry::frame_capture::compute_frame_time_stats(&samples);
    let ended_at = chrono::Utc::now().timestamp();

    ExecutionResult::ok(
        "Captura de frames finalizada.",
        json!({
            "implemented": true,
            "captureId": capture_id,
            "startedAt": capture.started_at,
            "endedAt": ended_at,
            "sampleCount": stats.sample_count,
            "avgFps": stats.avg_fps,
            "avgFrameTimeMs": stats.avg_frame_time_ms,
            "low1PctFps": stats.low_1pct_fps,
            "low0_1PctFps": stats.low_0_1pct_fps,
            "droppedFrameCount": stats.dropped_frame_count,
            "stutterCount": stats.stutter_count,
        }),
    )
}

/// How many finished captures can wait for upload at once. Each entry is a
/// handful of numbers (no per-frame data - just the reduced
/// `FrameTimeStats`), so this is a generous ceiling meant only to stop
/// unbounded growth if the backend is unreachable for a long stretch, not a
/// realistic day-to-day limit.
const MAX_QUEUED_UPLOADS: usize = 50;

fn pending_uploads_path() -> std::path::PathBuf {
    super::snapshot::app_data_dir().join("frame-capture-pending-uploads.json")
}

/// Appends a finished capture's stats (already shaped like the server's
/// `FrameCaptureSessionCreate` schema, minus `deviceId` - the engine adds
/// that at POST time, same as `sync_performance_report_summary` does for
/// performance summaries) to the local pending-upload queue. This module
/// has no backend credentials and no network client of its own, so queuing
/// to disk and letting the telemetry engine's next tick pick it up (see
/// `queued_frame_capture_uploads` / `discard_queued_frame_capture_upload`)
/// is how a capture's result actually reaches the server.
pub fn queue_frame_capture_upload(mut entry: Value) -> Result<(), String> {
    if let Some(object) = entry.as_object_mut() {
        object.insert(
            "localQueueId".to_string(),
            json!(uuid::Uuid::new_v4().simple().to_string()),
        );
    }

    let mut queued = queued_frame_capture_uploads();
    queued.push(entry);
    if queued.len() > MAX_QUEUED_UPLOADS {
        let overflow = queued.len() - MAX_QUEUED_UPLOADS;
        queued.drain(0..overflow);
    }
    write_pending_uploads(&queued)
}

/// Reads the queue without clearing it - the caller (the telemetry engine)
/// removes entries one at a time as each upload actually succeeds, via
/// `discard_queued_frame_capture_upload`, so a mid-batch network failure
/// doesn't lose the captures that came before it in the same tick.
pub fn queued_frame_capture_uploads() -> Vec<Value> {
    let Ok(raw) = std::fs::read_to_string(pending_uploads_path()) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn discard_queued_frame_capture_upload(local_queue_id: &str) {
    let mut queued = queued_frame_capture_uploads();
    let before = queued.len();
    queued.retain(|entry| {
        entry.get("localQueueId").and_then(Value::as_str) != Some(local_queue_id)
    });
    if queued.len() != before {
        let _ = write_pending_uploads(&queued);
    }
}

fn write_pending_uploads(queued: &[Value]) -> Result<(), String> {
    let path = pending_uploads_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(queued).map_err(|error| error.to_string())?;
    std::fs::write(path, raw).map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn start_frame_capture_sync(_payload: Option<Value>) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Captura de frames via PresentMon disponivel apenas no Windows.".to_string(),
        details: json!({ "implemented": true }),
    }
}

#[cfg(not(windows))]
fn stop_frame_capture_sync(_payload: Option<Value>) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Captura de frames via PresentMon disponivel apenas no Windows.".to_string(),
        details: json!({ "implemented": true }),
    }
}
