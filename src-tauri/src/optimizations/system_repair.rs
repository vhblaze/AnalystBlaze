//! Runs `sfc /scannow` and, if it finds corruption it can't fix on its own,
//! `DISM /Online /Cleanup-Image /RestoreHealth` - the real Windows tools for
//! checking/repairing corrupted system files, both of which require
//! elevation and can legitimately run for many minutes. Only ever invoked
//! through the privileged helper (see `optimizations::privileged_helper`),
//! same reasoning as `frame_capture_control.rs`.
//!
//! START_SYSTEM_FILE_CHECK / SYSTEM_FILE_CHECK_STATUS (and the DISM pair
//! below) share the same start-then-poll shape as
//! START_FRAME_CAPTURE/STOP_FRAME_CAPTURE: the helper's one-shot
//! request/response protocol can't stream a live result, so START spawns
//! the tool in a background thread and returns a `scanId` immediately, and
//! the caller polls STATUS until it reports `done`.
//!
//! No live percentage is reported. Both sfc and DISM draw their in-console
//! progress with carriage-return redraws meant for an interactive terminal,
//! not discrete lines a redirected pipe can parse into a percentage -
//! reporting a fake/smoothed number here would be worse than being honest
//! that this is "running, elapsed Ns" until it finishes.
//!
//! sfc's own process exit code is not reliable for telling "found nothing"
//! apart from "found and repaired" (both commonly return 0) - the actual
//! signal is the final summary line it prints, so outcomes are classified
//! by matching that line in both English and Portuguese, the same
//! locale-agnostic-anchor technique `telemetry::network`'s ping/traceroute
//! parsers use (anchor on a language-independent marker - here, a short
//! list of known EN/PT-BR key phrases - rather than the whole sentence).
//! If neither known phrase matches, the outcome is `Unknown` and the raw
//! tail of the tool's own output is returned so the UI can show it
//! verbatim instead of asserting a result we're not sure of.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::ExecutionResult;

/// sfc/DISM can legitimately run ~15-20 minutes on a slow disk or a large
/// component store repair. A scan nobody ever polled to completion (crashed
/// caller, helper restarted) is reaped after this long instead of leaking
/// a background thread/child process forever - same policy as
/// frame_capture_control.rs's MAX_CAPTURE_SECONDS, just longer.
const MAX_SCAN_SECONDS: i64 = 60 * 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanKind {
    Sfc,
    DismRestoreHealth,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ScanOutcome {
    /// No integrity violations found - nothing to fix.
    Clean,
    /// Violations were found and sfc/DISM fixed them.
    Repaired,
    /// sfc found violations it could not fix on its own - the normal
    /// trigger to offer DISM RestoreHealth as the next step.
    NeedsDismRepair,
    /// Windows Resource Protection could not even perform the scan
    /// (usually needs a reboot into safe mode or another scan already
    /// running) - not the same as "clean".
    CouldNotPerform,
    /// The tool's own exit code reported failure.
    Failed,
    /// Finished, but the summary line didn't match any known EN/PT-BR
    /// phrase - the raw output is returned so the UI can show it as-is
    /// rather than asserting a result we're not confident about.
    Unknown,
}

struct ScanResult {
    success: bool,
    outcome: ScanOutcome,
    message: String,
    raw_tail: String,
}

struct ActiveScan {
    kind: ScanKind,
    started_at: i64,
    result: Arc<Mutex<Option<ScanResult>>>,
}

static ACTIVE_SCANS: OnceLock<Mutex<HashMap<String, ActiveScan>>> = OnceLock::new();

fn active_scans() -> &'static Mutex<HashMap<String, ActiveScan>> {
    ACTIVE_SCANS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drops any scan entry older than MAX_SCAN_SECONDS, running or not -
/// mirrors frame_capture_control.rs's reap_stale_captures. A finished scan
/// the caller never polled past this window is assumed abandoned; one still
/// running that long is almost certainly stuck (or the caller crashed) and
/// its thread will simply finish and drop its result into a Mutex nobody
/// reads anymore, which is harmless.
fn reap_stale_scans(scans: &mut HashMap<String, ActiveScan>) {
    let now = chrono::Utc::now().timestamp();
    scans.retain(|_, scan| now.saturating_sub(scan.started_at) <= MAX_SCAN_SECONDS);
}

// The exact typed-confirmation phrase each action requires
// ("RUN_SYSTEM_FILE_CHECK" / "RUN_DISM_RESTORE_HEALTH") is enforced by
// safety.rs::validate_action_payload before either function below is ever
// reached - same gate RESET_WINSOCK_CATALOG uses, see typed_confirmation_matches
// there. Nothing to re-check here.

pub async fn start_system_file_check(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(|| start_scan_sync(ScanKind::Sfc)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao iniciar a verificacao de arquivos de sistema: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn start_dism_restore_health(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(|| start_scan_sync(ScanKind::DismRestoreHealth)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao iniciar o reparo DISM: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn system_file_check_status(payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(move || scan_status_sync(payload)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao consultar o status da verificacao: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

fn scan_status_sync(payload: Option<Value>) -> ExecutionResult {
    let scan_id = payload
        .as_ref()
        .and_then(|value| value.get("scanId").or_else(|| value.get("scan_id")))
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(scan_id) = scan_id else {
        return ExecutionResult {
            success: false,
            message: "Informe o scanId retornado ao iniciar a verificacao.".to_string(),
            details: json!({ "implemented": true }),
        };
    };

    let Ok(mut scans) = active_scans().lock() else {
        return ExecutionResult {
            success: false,
            message: "Estado interno da verificacao indisponivel.".to_string(),
            details: json!({ "implemented": true }),
        };
    };
    reap_stale_scans(&mut scans);
    let Some(scan) = scans.get(&scan_id) else {
        return ExecutionResult {
            success: false,
            message: "Nenhuma verificacao ativa com esse scanId.".to_string(),
            details: json!({ "implemented": true, "scanId": scan_id }),
        };
    };

    let elapsed_seconds = chrono::Utc::now().timestamp().saturating_sub(scan.started_at);
    let result = scan.result.lock().ok().and_then(|guard| {
        guard.as_ref().map(|result| {
            json!({
                "success": result.success,
                "outcome": result.outcome,
                "message": result.message,
                "rawTail": result.raw_tail,
            })
        })
    });
    let done = result.is_some();
    let kind_label = match scan.kind {
        ScanKind::Sfc => "sfc",
        ScanKind::DismRestoreHealth => "dism_restore_health",
    };
    drop(scans);

    if done {
        // Free the slot once the caller has actually seen the result -
        // status calls are cheap to repeat, so there's no reason to make
        // the caller send a separate "dismiss" action.
        if let Ok(mut scans) = active_scans().lock() {
            scans.remove(&scan_id);
        }
    }

    ExecutionResult::ok(
        if done {
            "Verificacao concluida."
        } else {
            "Verificacao em andamento."
        },
        json!({
            "implemented": true,
            "scanId": scan_id,
            "kind": kind_label,
            "done": done,
            "elapsedSeconds": elapsed_seconds,
            "result": result,
        }),
    )
}

#[cfg(windows)]
fn system_root() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string())
}

#[cfg(windows)]
fn start_scan_sync(kind: ScanKind) -> ExecutionResult {
    use crate::process_ext::CommandExt;
    use std::process::{Command, Stdio};
    use std::thread;

    let (tool_path, args): (std::path::PathBuf, Vec<&str>) = match kind {
        ScanKind::Sfc => (
            std::path::Path::new(&system_root()).join("System32\\sfc.exe"),
            vec!["/scannow"],
        ),
        ScanKind::DismRestoreHealth => (
            std::path::Path::new(&system_root()).join("System32\\dism.exe"),
            vec!["/Online", "/Cleanup-Image", "/RestoreHealth"],
        ),
    };

    if !tool_path.is_file() {
        return ExecutionResult {
            success: false,
            message: format!("Ferramenta nao encontrada em {}", tool_path.display()),
            details: json!({ "implemented": true }),
        };
    }

    let mut command = Command::new(&tool_path);
    command
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .no_window();

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel iniciar a ferramenta.".to_string(),
                details: json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    };

    let result: Arc<Mutex<Option<ScanResult>>> = Arc::new(Mutex::new(None));
    let thread_result = Arc::clone(&result);
    thread::spawn(move || {
        let output = child.wait_with_output();
        let scan_result = match output {
            Ok(output) => classify_output(kind, output.status.success(), &output.stdout, &output.stderr),
            Err(error) => ScanResult {
                success: false,
                outcome: ScanOutcome::Failed,
                message: format!("A ferramenta encerrou com erro: {error}"),
                raw_tail: String::new(),
            },
        };
        if let Ok(mut guard) = thread_result.lock() {
            *guard = Some(scan_result);
        }
    });

    let scan_id = uuid::Uuid::new_v4().simple().to_string();
    let started_at = chrono::Utc::now().timestamp();
    let Ok(mut scans) = active_scans().lock() else {
        return ExecutionResult {
            success: false,
            message: "Estado interno da verificacao indisponivel.".to_string(),
            details: json!({ "implemented": true }),
        };
    };
    reap_stale_scans(&mut scans);
    scans.insert(
        scan_id.clone(),
        ActiveScan {
            kind,
            started_at,
            result,
        },
    );

    ExecutionResult::ok(
        "Verificacao iniciada. Isso pode levar varios minutos - consulte o status periodicamente.",
        json!({
            "implemented": true,
            "scanId": scan_id,
            "startedAt": started_at,
        }),
    )
}

/// Classifies sfc/DISM's own textual summary. Exit code alone isn't enough
/// - sfc commonly returns 0 whether it found nothing or found and repaired
/// something - so this anchors on known EN/PT-BR phrases in the captured
/// output, same technique as telemetry::network's timeout detection
/// (a short OR-list of the locales we actually ship, not a full sentence
/// match, not a translation table).
#[cfg(windows)]
fn classify_output(kind: ScanKind, exit_success: bool, stdout: &[u8], stderr: &[u8]) -> ScanResult {
    use crate::process_ext::decode_console_bytes;

    let stdout_text = decode_console_bytes(stdout);
    let stderr_text = decode_console_bytes(stderr);
    let combined = format!("{stdout_text}\n{stderr_text}");
    let lower = combined.to_lowercase();
    let raw_tail = tail_lines(&combined, 12);

    let outcome = match kind {
        ScanKind::Sfc => {
            if contains_any(&lower, &[
                "did not find any integrity violations",
                "não encontrou nenhuma violação de integridade",
                "nao encontrou nenhuma violacao de integridade",
            ]) {
                ScanOutcome::Clean
            } else if contains_any(&lower, &[
                "found corrupt files and successfully repaired",
                "encontrou arquivos corrompidos e os reparou",
            ]) {
                ScanOutcome::Repaired
            } else if contains_any(&lower, &[
                "found corrupt files but was unable to fix",
                "found corrupt files but is unable to fix",
                "encontrou arquivos corrompidos, mas não foi possível corrigir",
                "encontrou arquivos corrompidos, mas nao foi possivel corrigir",
            ]) {
                ScanOutcome::NeedsDismRepair
            } else if contains_any(&lower, &[
                "could not perform the requested operation",
                "não pôde executar a operação solicitada",
                "nao pode executar a operacao solicitada",
            ]) {
                ScanOutcome::CouldNotPerform
            } else if !exit_success {
                ScanOutcome::Failed
            } else {
                ScanOutcome::Unknown
            }
        }
        ScanKind::DismRestoreHealth => {
            if contains_any(&lower, &[
                "restore operation completed successfully",
                "no component store corruption detected",
                "operação de restauração foi concluída com êxito",
                "operacao de restauracao foi concluida com exito",
                "nenhuma corrupção do repositório de componentes foi detectada",
            ]) {
                ScanOutcome::Repaired
            } else if !exit_success {
                ScanOutcome::Failed
            } else if contains_any(&lower, &["completed successfully", "concluída com êxito", "concluida com exito"]) {
                ScanOutcome::Repaired
            } else {
                ScanOutcome::Unknown
            }
        }
    };

    let message = match &outcome {
        ScanOutcome::Clean => "Nenhum arquivo de sistema corrompido encontrado.".to_string(),
        ScanOutcome::Repaired => "Arquivos corrompidos encontrados e reparados com sucesso.".to_string(),
        ScanOutcome::NeedsDismRepair => {
            "Arquivos corrompidos encontrados, mas o sfc nao conseguiu corrigir sozinho - use o reparo DISM."
                .to_string()
        }
        ScanOutcome::CouldNotPerform => {
            "O Windows nao conseguiu concluir a verificacao (pode exigir reiniciar em modo de seguranca)."
                .to_string()
        }
        ScanOutcome::Failed => "A ferramenta encerrou com erro.".to_string(),
        ScanOutcome::Unknown => {
            "Verificacao concluida, mas nao foi possivel classificar o resultado automaticamente - veja o texto original abaixo."
                .to_string()
        }
    };

    ScanResult {
        success: matches!(outcome, ScanOutcome::Clean | ScanOutcome::Repaired),
        outcome,
        message,
        raw_tail,
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

/// Keeps only the last few non-empty lines - sfc/DISM's summary is always
/// near the end, and the full transcript (sfc especially, which repeats a
/// progress line many times) is not useful to store or show.
fn tail_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let start = lines.len().saturating_sub(count);
    lines[start..].join("\n")
}

#[cfg(not(windows))]
fn start_scan_sync(_kind: ScanKind) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Verificacao de arquivos de sistema disponivel apenas no Windows.".to_string(),
        details: json!({ "implemented": true }),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn stdout(text: &str) -> Vec<u8> {
        text.as_bytes().to_vec()
    }

    #[test]
    fn sfc_english_clean_result_is_classified_correctly() {
        let output = stdout(
            "Beginning system scan.\nVerification 100% complete.\nWindows Resource Protection did not find any integrity violations.",
        );
        let result = classify_output(ScanKind::Sfc, true, &output, &[]);
        assert!(result.success);
        assert_eq!(result.outcome, ScanOutcome::Clean);
    }

    #[test]
    fn sfc_portuguese_clean_result_is_classified_correctly() {
        let output = stdout(
            "Verificacao 100% concluida.\nA Protecao de Recursos do Windows nao encontrou nenhuma violacao de integridade.",
        );
        let result = classify_output(ScanKind::Sfc, true, &output, &[]);
        assert!(result.success);
        assert_eq!(result.outcome, ScanOutcome::Clean);
    }

    #[test]
    fn sfc_english_repaired_result_is_classified_correctly() {
        let output = stdout(
            "Windows Resource Protection found corrupt files and successfully repaired them.",
        );
        let result = classify_output(ScanKind::Sfc, true, &output, &[]);
        assert!(result.success);
        assert_eq!(result.outcome, ScanOutcome::Repaired);
    }

    #[test]
    fn sfc_portuguese_needs_dism_result_is_classified_correctly() {
        let output = stdout(
            "A Protecao de Recursos do Windows encontrou arquivos corrompidos, mas nao foi possivel corrigir alguns deles.",
        );
        let result = classify_output(ScanKind::Sfc, true, &output, &[]);
        assert!(!result.success);
        assert_eq!(result.outcome, ScanOutcome::NeedsDismRepair);
    }

    #[test]
    fn sfc_could_not_perform_is_not_confused_with_clean() {
        let output = stdout(
            "Windows Resource Protection could not perform the requested operation.",
        );
        let result = classify_output(ScanKind::Sfc, true, &output, &[]);
        assert!(!result.success);
        assert_eq!(result.outcome, ScanOutcome::CouldNotPerform);
    }

    #[test]
    fn sfc_unrecognized_output_is_unknown_not_falsely_clean() {
        // A locale/wording this parser doesn't know about must never be
        // silently reported as "Clean" - Unknown plus the raw text is the
        // only honest outcome when the summary line doesn't match anything.
        let output = stdout("Some future Windows build's wording nobody wrote a matcher for yet.");
        let result = classify_output(ScanKind::Sfc, true, &output, &[]);
        assert!(!result.success);
        assert_eq!(result.outcome, ScanOutcome::Unknown);
        assert!(!result.raw_tail.is_empty());
    }

    #[test]
    fn sfc_exit_failure_without_a_known_phrase_is_failed_not_unknown() {
        let output = stdout("some unexpected crash output");
        let result = classify_output(ScanKind::Sfc, false, &output, &[]);
        assert!(!result.success);
        assert_eq!(result.outcome, ScanOutcome::Failed);
    }

    #[test]
    fn dism_english_success_result_is_classified_correctly() {
        let output = stdout("The restore operation completed successfully.");
        let result = classify_output(ScanKind::DismRestoreHealth, true, &output, &[]);
        assert!(result.success);
        assert_eq!(result.outcome, ScanOutcome::Repaired);
    }

    #[test]
    fn dism_portuguese_success_result_is_classified_correctly() {
        let output = stdout("A operacao de restauracao foi concluida com exito.");
        let result = classify_output(ScanKind::DismRestoreHealth, true, &output, &[]);
        assert!(result.success);
        assert_eq!(result.outcome, ScanOutcome::Repaired);
    }

    #[test]
    fn dism_stderr_is_considered_alongside_stdout() {
        // DISM sometimes writes its summary to stderr - the classifier must
        // check both streams, not just stdout.
        let result = classify_output(
            ScanKind::DismRestoreHealth,
            true,
            &[],
            &stdout("The restore operation completed successfully."),
        );
        assert!(result.success);
        assert_eq!(result.outcome, ScanOutcome::Repaired);
    }

    #[test]
    fn tail_lines_keeps_only_the_last_non_empty_lines() {
        let text = "line1\n\nline2\nline3\nline4\n";
        assert_eq!(tail_lines(text, 2), "line3\nline4");
        assert_eq!(tail_lines(text, 10), "line1\nline2\nline3\nline4");
    }
}
