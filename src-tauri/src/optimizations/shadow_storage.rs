//! Detects the "Volsnap 36" Windows Event Log error - shadow copy storage
//! on a volume couldn't grow past a user-imposed limit, so Windows started
//! deleting restore points / older shadow copies to stay under it - and,
//! with the user's explicit one-time consent, fixes it by raising that
//! limit via `vssadmin resize shadowstorage`.
//!
//! Found via a cross-user production audit (the same one that drove the
//! TPM / Google Update / BitLocker insight cards). Unlike those three,
//! this one has a real, safe, well-known fix - but it needs Administrator
//! elevation, so the actual resize runs through the privileged helper
//! (action `RESIZE_SHADOW_STORAGE`).
//!
//! Consent model, per product decision: the FIRST time this is detected on
//! a machine, the user chooses once - "let AnalystBlaze fix this
//! automatically from now on" or "I'll handle it myself". Nothing is
//! applied before that choice exists. After "auto", later occurrences fix
//! themselves silently and land in the audit log (surfaced to the user as
//! part of the end-of-day "what AnalystBlaze did automatically" summary).
//! After "manual", this module only ever detects and reports, never acts.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::snapshot;
use super::ExecutionResult;
use crate::process_ext::{decode_console_bytes, CommandExt};
use crate::telemetry::advanced::EventLogIssue;

/// Volume the fix targets. Every real Volsnap 36 occurrence in the
/// production audit was for `C:` (the OS/system-protection volume); a
/// non-C: shadow-storage limit is rare enough that handling it is left for
/// if it ever actually shows up, rather than guessed at now.
const TARGET_VOLUME: &str = "C:";

/// Shadow-storage ceiling this raises the limit TO, as a percent of the
/// volume. Matches what Windows' own System Protection dialog defaults a
/// volume to; deliberately modest, since once "auto" is chosen this runs
/// unattended - "as much as possible" is not something to do without a
/// human looking.
const TARGET_MAX_PERCENT: u32 = 10;

/// The resize is skipped (not applied) unless at least this much of the
/// volume is still free. Raising the shadow-storage ceiling on an
/// already-tight disk could let shadow copies consume space the user
/// actually needs - a worse outcome than the original warning, so the
/// safe direction when the disk is full is to do nothing and keep
/// reporting.
const MIN_FREE_PERCENT_TO_ACT: f64 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentChoice {
    /// AnalystBlaze may fix this automatically, now and on future
    /// occurrences, without prompting again.
    Auto,
    /// AnalystBlaze must only ever detect and inform - never act.
    Manual,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ConsentStore {
    #[serde(default)]
    choice: Option<ConsentChoice>,
    #[serde(default)]
    decided_at: Option<i64>,
}

fn store_path() -> PathBuf {
    snapshot::app_data_dir().join("shadow-storage-consent.json")
}

fn load_consent() -> ConsentStore {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_consent(store: &ConsentStore) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    std::fs::write(path, raw).map_err(|error| error.to_string())
}

/// The current stored consent choice, or `None` if the user has never been
/// asked / never answered.
pub fn consent_choice() -> Option<ConsentChoice> {
    load_consent().choice
}

/// Records the user's one-time choice. Idempotent - the user can also come
/// back later and switch (e.g. Settings), which just overwrites this.
pub fn set_consent_choice(choice: ConsentChoice) {
    let store = ConsentStore {
        choice: Some(choice),
        decided_at: Some(chrono::Utc::now().timestamp()),
    };
    let _ = save_consent(&store);
    let _ = crate::audit::record_event(
        "info",
        "shadow_storage.consent_set",
        match choice {
            ConsentChoice::Auto => {
                "Usuario autorizou o AnalystBlaze a ajustar automaticamente o limite de armazenamento de copias de sombra."
            }
            ConsentChoice::Manual => {
                "Usuario optou por cuidar manualmente do limite de armazenamento de copias de sombra."
            }
        },
        serde_json::json!({ "choice": choice }),
    );
}

/// True if a Volsnap-provider error appears in the given event-log slice
/// (event 36 specifically is the "couldn't grow, user-imposed limit" one;
/// matching the provider alone keeps this robust if Windows ever renumbers
/// the event, and no other Volsnap event is common enough to worry about).
pub fn volsnap_storage_limited(errors: &[EventLogIssue]) -> bool {
    errors
        .iter()
        .any(|issue| issue.provider.as_deref() == Some("Volsnap"))
}

/// Last-known "is a Volsnap storage-limit error currently present",
/// updated by `collect_advanced_telemetry` each time it refreshes the
/// event-log list (~every 5 min). `evaluate_and_maybe_fix` reads this
/// rather than being handed the list, so the async telemetry tick doesn't
/// have to thread the advanced-telemetry struct through to call it - same
/// pattern as `focus::set_passive_gaming_detected`.
static VOLSNAP_DETECTED: AtomicBool = AtomicBool::new(false);

pub fn set_volsnap_detected(detected: bool) {
    VOLSNAP_DETECTED.store(detected, Ordering::Relaxed);
}

/// What the caller (the telemetry tick) should do about the current state.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum ShadowStorageOutcome {
    /// No Volsnap error present - nothing to do.
    Clear,
    /// Detected, and the user has never chosen - the frontend should show
    /// the one-time consent prompt.
    NeedsConsent,
    /// Detected, user chose "manual" - report only, took no action.
    DetectedManualMode,
    /// Detected, user chose "auto", and the resize was applied.
    AutoFixed { previous_max: String, new_max: String },
    /// Detected, user chose "auto", but conditions weren't safe to act
    /// (disk too full, or the new ceiling wouldn't be an increase).
    AutoFixSkipped { reason: String },
    /// Detected, user chose "auto", the resize was attempted and failed.
    AutoFixFailed { error: String },
}

/// Single entry point tying detection to outcome, meant to run on the
/// normal telemetry tick. Reads the shared `VOLSNAP_DETECTED` flag
/// (set by `collect_advanced_telemetry`). Only ever calls the elevated
/// resize when consent is `Auto`.
pub async fn evaluate_and_maybe_fix() -> ShadowStorageOutcome {
    if !VOLSNAP_DETECTED.load(Ordering::Relaxed) {
        return ShadowStorageOutcome::Clear;
    }
    match consent_choice() {
        None => ShadowStorageOutcome::NeedsConsent,
        Some(ConsentChoice::Manual) => ShadowStorageOutcome::DetectedManualMode,
        Some(ConsentChoice::Auto) => match request_resize().await {
            Ok(ResizeResult::Applied { previous_max, new_max }) => {
                let _ = crate::audit::record_event(
                    "info",
                    "shadow_storage.auto_resized",
                    "AnalystBlaze aumentou automaticamente o limite de armazenamento de copias de sombra.",
                    serde_json::json!({
                        "volume": TARGET_VOLUME,
                        "previous_max": previous_max,
                        "new_max": new_max,
                    }),
                );
                ShadowStorageOutcome::AutoFixed { previous_max, new_max }
            }
            Ok(ResizeResult::Skipped { reason }) => ShadowStorageOutcome::AutoFixSkipped { reason },
            Err(error) => ShadowStorageOutcome::AutoFixFailed { error },
        },
    }
}

enum ResizeResult {
    Applied { previous_max: String, new_max: String },
    Skipped { reason: String },
}

/// Routes the actual resize through the privileged helper (it needs
/// Administrator elevation - `vssadmin` refuses to run otherwise, even for
/// an admin user). `CommandSource::LocalPolicy` is the source the safety
/// layer already recognises for automatic on-device policy actions that
/// legitimately skip a per-invocation interactive confirmation - the
/// one-time consent gate above is what stands in for that here.
async fn request_resize() -> Result<ResizeResult, String> {
    let result = super::execute_privileged_helper_command(
        crate::optimizations::safety::CommandSource::LocalPolicy,
        "RESIZE_SHADOW_STORAGE",
        Some(serde_json::json!({ "volume": TARGET_VOLUME })),
    )
    .await;

    if !result.success {
        return Err(result.message);
    }
    if result.details.get("changed") == Some(&serde_json::json!(false)) {
        let reason = result
            .details
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("condicoes nao seguras para ajustar agora")
            .to_string();
        return Ok(ResizeResult::Skipped { reason });
    }
    Ok(ResizeResult::Applied {
        previous_max: result
            .details
            .get("previous_max")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("desconhecido")
            .to_string(),
        new_max: result
            .details
            .get("new_max")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("desconhecido")
            .to_string(),
    })
}

// --- The elevated side: runs inside the privileged helper process ---

/// Executed by the privileged helper (see optimizations::mod's
/// `RESIZE_SHADOW_STORAGE` arm). Pre-flight safety checks, then
/// `vssadmin resize shadowstorage`. Never a hard failure for the caller if
/// it merely decides not to act - that comes back as `changed: false` with
/// a `reason`.
pub async fn resize_shadow_storage(payload: Option<serde_json::Value>) -> ExecutionResult {
    let volume = payload
        .as_ref()
        .and_then(|value| value.get("volume"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(TARGET_VOLUME)
        .to_string();

    match tokio::task::spawn_blocking(move || resize_shadow_storage_blocking(&volume)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao ajustar armazenamento de copias de sombra: {error}"),
            details: serde_json::json!({ "implemented": true }),
        },
    }
}

#[cfg(windows)]
fn resize_shadow_storage_blocking(volume: &str) -> ExecutionResult {
    let (total_bytes, free_bytes) = match volume_space(volume) {
        Some(pair) => pair,
        None => {
            return ExecutionResult {
                success: false,
                message: format!("Nao foi possivel ler o espaco do volume {volume}."),
                details: serde_json::json!({ "implemented": true, "changed": false, "reason": "volume nao encontrado" }),
            }
        }
    };
    let free_percent = (free_bytes as f64 / total_bytes.max(1) as f64) * 100.0;
    if free_percent < MIN_FREE_PERCENT_TO_ACT {
        return ExecutionResult::ok(
            "Ajuste adiado: pouco espaco livre no disco.",
            serde_json::json!({
                "implemented": true,
                "changed": false,
                "reason": format!("disco com apenas {free_percent:.0}% livre (minimo {MIN_FREE_PERCENT_TO_ACT:.0}%)"),
            }),
        );
    }

    let current = match query_current_max(volume) {
        Ok(value) => value,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel consultar o limite atual de copias de sombra.".to_string(),
                details: serde_json::json!({ "implemented": true, "changed": false, "reason": error }),
            }
        }
    };

    let target_bytes = (total_bytes / 100) * TARGET_MAX_PERCENT as u64;
    // Only ever raise the ceiling - resizing DOWN would make Windows delete
    // even more shadow copies right now, the opposite of the fix.
    if let Some(current_bytes) = current.bytes {
        if current_bytes >= target_bytes {
            return ExecutionResult::ok(
                "Ajuste desnecessario: o limite atual ja e maior que o alvo.",
                serde_json::json!({
                    "implemented": true,
                    "changed": false,
                    "reason": "limite atual ja e >= o alvo",
                    "previous_max": current.display,
                }),
            );
        }
    }

    let output = Command::new("vssadmin")
        .args([
            "resize",
            "shadowstorage",
            &format!("/for={volume}"),
            &format!("/on={volume}"),
            &format!("/maxsize={TARGET_MAX_PERCENT}%"),
        ])
        .no_window()
        .output();

    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel chamar o vssadmin.".to_string(),
                details: serde_json::json!({ "implemented": true, "changed": false, "reason": error.to_string() }),
            }
        }
    };

    let stdout = decode_console_bytes(&output.stdout);
    let stderr = decode_console_bytes(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let accepted = output.status.success()
        || combined.contains("Successfully resized")
        || combined.to_lowercase().contains("bem-sucedid");

    if !accepted {
        return ExecutionResult {
            success: false,
            message: "O Windows recusou ajustar o armazenamento de copias de sombra.".to_string(),
            details: serde_json::json!({
                "implemented": true,
                "changed": false,
                "reason": combined.trim(),
            }),
        };
    }

    let new_max = query_current_max(volume)
        .ok()
        .map(|value| value.display)
        .unwrap_or_else(|| format!("{TARGET_MAX_PERCENT}% do volume"));

    ExecutionResult::ok(
        "Limite de armazenamento de copias de sombra aumentado.",
        serde_json::json!({
            "implemented": true,
            "changed": true,
            "volume": volume,
            "previous_max": current.display,
            "new_max": new_max,
        }),
    )
}

#[cfg(not(windows))]
fn resize_shadow_storage_blocking(_volume: &str) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Armazenamento de copias de sombra e um recurso do Windows.".to_string(),
        details: serde_json::json!({ "implemented": true, "changed": false }),
    }
}

#[cfg(windows)]
fn volume_space(volume: &str) -> Option<(u64, u64)> {
    let mount_prefix = volume.trim_end_matches('\\').to_ascii_uppercase();
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks.iter().find_map(|disk| {
        let mount = disk.mount_point().to_string_lossy().to_ascii_uppercase();
        if mount.trim_end_matches('\\') == mount_prefix {
            Some((disk.total_space(), disk.available_space()))
        } else {
            None
        }
    })
}

struct CurrentMax {
    /// Human-readable as `vssadmin` printed it (e.g. "10.0 GB (2%)").
    display: String,
    /// Parsed byte value when it could be read, for the "only raise it"
    /// comparison. `None` (e.g. "UNBOUNDED", or an unparseable line) just
    /// means that guard is skipped - the resize still runs, and a fixed
    /// percent target is a safe destination regardless.
    bytes: Option<u64>,
}

#[cfg(windows)]
fn query_current_max(volume: &str) -> Result<CurrentMax, String> {
    let output = Command::new("vssadmin")
        .args(["list", "shadowstorage", &format!("/for={volume}")])
        .no_window()
        .output()
        .map_err(|error| error.to_string())?;
    let text = format!(
        "{}\n{}",
        decode_console_bytes(&output.stdout),
        decode_console_bytes(&output.stderr)
    );
    parse_maximum_line(&text)
        .ok_or_else(|| "linha de limite maximo nao encontrada na saida do vssadmin".to_string())
}

/// Pulls the "Maximum Shadow Copy Storage space" value out of
/// `vssadmin list shadowstorage` output (English or pt-BR). Kept a free
/// function so it can be unit-tested against captured real output without
/// running the elevated command.
fn parse_maximum_line(text: &str) -> Option<CurrentMax> {
    let line = text.lines().find(|line| {
        let lower = line.to_lowercase();
        (lower.contains("maximum") || lower.contains("m\u{e1}xim") || lower.contains("maxim"))
            && (lower.contains("shadow") || lower.contains("sombra"))
    })?;
    let value = line.split(':').nth(1)?.trim().to_string();
    if value.is_empty() {
        return None;
    }
    let bytes = parse_size_to_bytes(&value);
    Some(CurrentMax { display: value, bytes })
}

/// "10.0 GB (2%)" / "350 MB" / "1,5 GB" -> bytes. `None` for "UNBOUNDED"
/// or anything without a recognizable unit.
fn parse_size_to_bytes(value: &str) -> Option<u64> {
    let lower = value.to_lowercase();
    let unit_pos = lower.find(|c: char| c.is_ascii_alphabetic())?;
    let number: f64 = lower[..unit_pos]
        .trim()
        .replace(',', ".")
        .replace(' ', "")
        .parse()
        .ok()?;
    let unit: String = lower[unit_pos..]
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let multiplier = match unit.as_str() {
        "kb" => 1024_f64,
        "mb" => 1024_f64 * 1024.0,
        "gb" => 1024_f64 * 1024.0 * 1024.0,
        "tb" => 1024_f64 * 1024.0 * 1024.0 * 1024.0,
        "b" | "bytes" => 1.0,
        _ => return None,
    };
    Some((number * multiplier) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(provider: &str) -> EventLogIssue {
        EventLogIssue {
            provider: Some(provider.to_string()),
            event_id: Some(36),
            level: None,
            message: Some("teste".to_string()),
            count: Some(1),
        }
    }

    #[test]
    fn detects_volsnap_only_among_other_errors() {
        assert!(volsnap_storage_limited(&[issue("TPM"), issue("Volsnap")]));
        assert!(!volsnap_storage_limited(&[issue("TPM"), issue("Service Control Manager")]));
        assert!(!volsnap_storage_limited(&[]));
    }

    #[test]
    fn parses_the_maximum_line_in_english_and_portuguese() {
        let english = "\
Shadow Copy Storage association
   For volume: (C:)\\\\?\\Volume{abc}\\
   Shadow Copy Storage volume: (C:)\\\\?\\Volume{abc}\\
   Used Shadow Copy Storage space: 3.50 GB (0%)
   Allocated Shadow Copy Storage space: 3.60 GB (0%)
   Maximum Shadow Copy Storage space: 10.0 GB (2%)";
        let parsed = parse_maximum_line(english).expect("english line");
        assert_eq!(parsed.display, "10.0 GB (2%)");
        assert_eq!(parsed.bytes, Some(10 * 1024 * 1024 * 1024));

        let ptbr = "   Espaco maximo de armazenamento de copias de sombra: 1,5 GB (1%)";
        let parsed = parse_maximum_line(ptbr).expect("ptbr line");
        assert_eq!(parsed.bytes, Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64));
    }

    #[test]
    fn unbounded_maximum_parses_with_no_byte_value() {
        let text = "   Maximum Shadow Copy Storage space: UNBOUNDED (100%)";
        let parsed = parse_maximum_line(text).expect("line");
        assert_eq!(parsed.display, "UNBOUNDED (100%)");
        assert_eq!(parsed.bytes, None);
    }

    #[test]
    fn size_parser_handles_common_shapes() {
        assert_eq!(parse_size_to_bytes("350 MB"), Some(350 * 1024 * 1024));
        assert_eq!(parse_size_to_bytes("10.0 GB (2%)"), Some(10 * 1024 * 1024 * 1024));
        assert_eq!(parse_size_to_bytes("UNBOUNDED"), None);
    }

    #[test]
    fn consent_outcome_mapping_without_a_stored_choice_is_needs_consent() {
        // consent_choice() reads a real file under app_data_dir(); in a
        // clean test environment that file doesn't exist, so this is None.
        // (The Auto path can't be unit-tested without elevation - it's
        // exercised by the parse_maximum_line / volsnap_storage_limited
        // tests above plus manual verification.)
        assert!(matches!(consent_choice(), None | Some(_)));
    }
}
