use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::thread;
use std::time::Duration;

use super::{latency, snapshot, ExecutionResult};
use crate::audit;

const FOCUS_SESSION_FILE: &str = "focus-session.json";
const DEFAULT_FOCUS_TTL_SECONDS: i64 = 60 * 60;
const MIN_FOCUS_TTL_SECONDS: i64 = 5 * 60;
const MAX_FOCUS_TTL_SECONDS: i64 = 4 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusSession {
    pub id: String,
    pub profile: String,
    pub label: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub status: String,
    pub restore_reason: Option<String>,
    pub restored_at: Option<i64>,
    pub snapshot_ids: Vec<String>,
    pub effects: FocusSessionEffects,
    pub quiet_details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusSessionEffects {
    pub suppress_agent_notifications: bool,
    pub visual_polling_min_interval_seconds: u64,
    pub pause_heavy_scans: bool,
    pub delay_non_critical_uploads: bool,
    pub non_critical_upload_delay_seconds: u64,
    pub background_quiet_mode: bool,
    pub reduce_secondary_processes: bool,
    pub session_tag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusProfile {
    Work,
    Game,
    Call,
    Study,
    Focus,
}

impl FocusProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Game => "game",
            Self::Call => "call",
            Self::Study => "study",
            Self::Focus => "focus",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Work => "Modo Foco para trabalho",
            Self::Game => "Modo Foco para jogo",
            Self::Call => "Modo Foco para chamada/reuniao",
            Self::Study => "Modo Foco para estudo",
            Self::Focus => "Modo Foco",
        }
    }

    fn default_ttl_seconds(self) -> i64 {
        match self {
            Self::Work => 90 * 60,
            Self::Game => 2 * 60 * 60,
            Self::Call => 60 * 60,
            Self::Study => 90 * 60,
            Self::Focus => DEFAULT_FOCUS_TTL_SECONDS,
        }
    }

    fn from_alias(value: &str) -> Option<Self> {
        match normalize_alias(value).as_str() {
            "work" | "trabalho" | "job" | "office" => Some(Self::Work),
            "game" | "gaming" | "jogo" | "gamer" => Some(Self::Game),
            "call" | "meeting" | "reuniao" | "reuniao_online" | "chamada" | "call_meeting" => {
                Some(Self::Call)
            }
            "study" | "estudo" | "school" | "aula" => Some(Self::Study),
            "focus" | "foco" | "deep_work" | "concentracao" => Some(Self::Focus),
            _ => None,
        }
    }
}

pub async fn enter_focus_mode(payload: Option<Value>) -> ExecutionResult {
    let profile = profile_from_payload(payload.as_ref());
    let ttl_seconds = ttl_seconds_from_payload(payload.as_ref(), profile);
    let now = chrono::Utc::now().timestamp();
    let effects = effects_for_profile(profile);

    if active_focus_session().is_some() {
        let _ = restore_focus_session(Some("replaced_by_new_focus_session".to_string()));
    }

    let quiet_payload = focus_quiet_payload(payload.as_ref(), profile);
    let background_quiet = if payload_bool(payload.as_ref(), "background_quiet", true)
        && payload_bool(payload.as_ref(), "backgroundQuiet", true)
    {
        latency::apply_background_quiet_mode(Some(quiet_payload.clone())).await
    } else {
        ExecutionResult::ok(
            "Background Quiet ignorado pela policy do Modo Foco.",
            json!({
                "implemented": true,
                "skipped_by_policy": true,
                "quiet_payload": quiet_payload,
            }),
        )
    };

    let snapshot_ids = collect_snapshot_ids([&background_quiet.details]);
    let quiet_details = json!({
        "success": background_quiet.success,
        "message": background_quiet.message,
        "details": background_quiet.details,
    });
    let session = FocusSession {
        id: uuid::Uuid::new_v4().simple().to_string(),
        profile: profile.as_str().to_string(),
        label: profile.label().to_string(),
        created_at: now,
        expires_at: now + ttl_seconds,
        status: "active".to_string(),
        restore_reason: None,
        restored_at: None,
        snapshot_ids: snapshot_ids.clone(),
        effects: effects.clone(),
        quiet_details: quiet_details.clone(),
    };

    if let Err(error) = write_focus_session(&session) {
        return ExecutionResult {
            success: false,
            message: format!(
                "Modo Foco nao foi ativado porque a sessao nao pode ser salva: {error}"
            ),
            details: json!({
                "implemented": true,
                "profile": profile.as_str(),
                "error": error,
            }),
        };
    }

    spawn_focus_restore_monitor(session.id.clone(), session.expires_at);

    let details = json!({
        "implemented": true,
        "profile": profile.as_str(),
        "label": profile.label(),
        "reversible": !snapshot_ids.is_empty(),
        "restoreStatus": "monitoring",
        "restoreSession": session,
        "snapshotIds": snapshot_ids,
        "policy": {
            "suppressAgentNotifications": effects.suppress_agent_notifications,
            "visualPollingMinIntervalSeconds": effects.visual_polling_min_interval_seconds,
            "pauseHeavyScans": effects.pause_heavy_scans,
            "delayNonCriticalUploads": effects.delay_non_critical_uploads,
            "nonCriticalUploadDelaySeconds": effects.non_critical_upload_delay_seconds,
            "backgroundQuietMode": effects.background_quiet_mode,
            "reduceSecondaryProcesses": effects.reduce_secondary_processes,
            "sessionTag": effects.session_tag,
        },
        "steps": {
            "backgroundQuiet": quiet_details,
        },
        "notes": [
            "Notificacoes do proprio AnalystBlaze ficam suprimidas para consumidores da policy local.",
            "Uploads em lote nao criticos sao atrasados enquanto a sessao estiver ativa.",
            "Scans pesados automaticos da policy local ficam pausados ate restaurar a sessao."
        ],
    });

    let _ = audit::record_event(
        "info",
        "focus.session_started",
        "Modo Foco ativado com policy local reversivel.",
        details.clone(),
    );

    ExecutionResult {
        success: true,
        message: format!(
            "{} ativado com Background Quiet e restauracao automatica.",
            profile.label()
        ),
        details,
    }
}

pub fn restore_focus_session(reason: Option<String>) -> snapshot::RestoreReport {
    let Some(mut session) = read_focus_session() else {
        return focus_restore_report("Nenhuma sessao ativa de Modo Foco encontrada.");
    };
    if session.restored_at.is_some() || session.status == "restored" {
        return focus_restore_report("Sessao de Modo Foco ja foi restaurada.");
    }

    let mut report = if session.snapshot_ids.is_empty() {
        focus_restore_report("Modo Foco encerrado; nao havia snapshots de processo para restaurar.")
    } else {
        snapshot::restore_snapshots_by_ids(&session.snapshot_ids)
    };
    let reason = reason.unwrap_or_else(|| "manual_restore".to_string());
    session.status = "restored".to_string();
    session.restored_at = Some(chrono::Utc::now().timestamp());
    session.restore_reason = Some(reason.clone());
    let _ = write_focus_session(&session);
    report.messages.push(format!(
        "Sessao {} encerrada por {}.",
        session.profile, reason
    ));

    let _ = audit::record_event(
        "info",
        "focus.session_restored",
        "Modo Foco restaurado por snapshots locais.",
        json!({
            "reason": &reason,
            "session": &session,
            "report": &report,
        }),
    );

    report
}

pub fn active_focus_session() -> Option<FocusSession> {
    let session = read_focus_session()?;
    if session.restored_at.is_some() || session.status == "restored" {
        return None;
    }
    if session.expires_at <= chrono::Utc::now().timestamp() {
        let _ = restore_focus_session(Some("ttl_expired".to_string()));
        return None;
    }
    Some(session)
}

pub fn focus_runtime_policy() -> Option<FocusSessionEffects> {
    active_focus_session().map(|session| session.effects)
}

pub fn should_pause_heavy_scans() -> bool {
    focus_runtime_policy().is_some_and(|effects| effects.pause_heavy_scans)
}

pub fn should_delay_non_critical_uploads() -> bool {
    focus_runtime_policy().is_some_and(|effects| effects.delay_non_critical_uploads)
}

pub fn visual_polling_min_interval_seconds() -> Option<u64> {
    focus_runtime_policy().map(|effects| effects.visual_polling_min_interval_seconds)
}

fn effects_for_profile(profile: FocusProfile) -> FocusSessionEffects {
    let visual_polling_min_interval_seconds = match profile {
        FocusProfile::Game => 4,
        FocusProfile::Call => 6,
        FocusProfile::Work | FocusProfile::Study => 5,
        FocusProfile::Focus => 5,
    };
    let non_critical_upload_delay_seconds = match profile {
        FocusProfile::Call | FocusProfile::Game => 10 * 60,
        _ => 5 * 60,
    };

    FocusSessionEffects {
        suppress_agent_notifications: true,
        visual_polling_min_interval_seconds,
        pause_heavy_scans: true,
        delay_non_critical_uploads: true,
        non_critical_upload_delay_seconds,
        background_quiet_mode: true,
        reduce_secondary_processes: true,
        session_tag: profile.as_str().to_string(),
    }
}

fn focus_quiet_payload(payload: Option<&Value>, profile: FocusProfile) -> Value {
    let mut targets = focus_background_targets(profile)
        .into_iter()
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    for target in payload_background_targets(payload) {
        targets.insert(target);
    }

    json!({
        "background_priority": "below_normal",
        "max_background_processes": max_background_processes(profile),
        "background_targets": targets.into_iter().collect::<Vec<_>>(),
        "focus_profile": profile.as_str(),
        "reason": "focus_mode",
    })
}

fn focus_background_targets(profile: FocusProfile) -> Vec<&'static str> {
    match profile {
        FocusProfile::Work => vec![
            "steam.exe",
            "steamwebhelper.exe",
            "epicgameslauncher.exe",
            "epicwebhelper.exe",
            "battle.net.exe",
            "spotify.exe",
            "onedrive.exe",
            "dropbox.exe",
            "googledrivesync.exe",
            "creative cloud.exe",
        ],
        FocusProfile::Game => vec![
            "chrome.exe",
            "msedge.exe",
            "firefox.exe",
            "spotify.exe",
            "discord.exe",
            "slack.exe",
            "teams.exe",
            "onedrive.exe",
            "dropbox.exe",
            "googledrivesync.exe",
            "steamwebhelper.exe",
            "epicwebhelper.exe",
        ],
        FocusProfile::Call => vec![
            "steam.exe",
            "steamwebhelper.exe",
            "epicgameslauncher.exe",
            "epicwebhelper.exe",
            "battle.net.exe",
            "spotify.exe",
            "onedrive.exe",
            "dropbox.exe",
            "googledrivesync.exe",
            "creative cloud.exe",
        ],
        FocusProfile::Study => vec![
            "steam.exe",
            "steamwebhelper.exe",
            "epicgameslauncher.exe",
            "epicwebhelper.exe",
            "battle.net.exe",
            "discord.exe",
            "spotify.exe",
            "onedrive.exe",
            "dropbox.exe",
            "googledrivesync.exe",
        ],
        FocusProfile::Focus => vec![
            "steam.exe",
            "steamwebhelper.exe",
            "epicgameslauncher.exe",
            "epicwebhelper.exe",
            "battle.net.exe",
            "spotify.exe",
            "onedrive.exe",
            "dropbox.exe",
            "googledrivesync.exe",
        ],
    }
}

fn max_background_processes(profile: FocusProfile) -> usize {
    match profile {
        FocusProfile::Game => 24,
        FocusProfile::Call => 12,
        FocusProfile::Work | FocusProfile::Study => 18,
        FocusProfile::Focus => 16,
    }
}

fn profile_from_payload(payload: Option<&Value>) -> FocusProfile {
    payload
        .and_then(|payload| {
            [
                "profile",
                "mode",
                "focus_profile",
                "focusProfile",
                "scenario",
                "session_tag",
                "sessionTag",
            ]
            .iter()
            .find_map(|key| payload.get(*key).and_then(Value::as_str))
        })
        .and_then(FocusProfile::from_alias)
        .unwrap_or(FocusProfile::Focus)
}

fn ttl_seconds_from_payload(payload: Option<&Value>, profile: FocusProfile) -> i64 {
    payload
        .and_then(|payload| {
            payload
                .get("ttl_seconds")
                .or_else(|| payload.get("ttlSeconds"))
                .or_else(|| payload.get("duration_seconds"))
                .or_else(|| payload.get("durationSeconds"))
                .and_then(Value::as_i64)
        })
        .unwrap_or_else(|| profile.default_ttl_seconds())
        .clamp(MIN_FOCUS_TTL_SECONDS, MAX_FOCUS_TTL_SECONDS)
}

fn payload_bool(payload: Option<&Value>, key: &str, default: bool) -> bool {
    payload
        .and_then(|payload| payload.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn payload_background_targets(payload: Option<&Value>) -> Vec<String> {
    payload
        .and_then(|payload| {
            payload
                .get("background_targets")
                .or_else(|| payload.get("backgroundTargets"))
        })
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter_map(sanitize_process_target)
                .take(40)
                .collect()
        })
        .unwrap_or_default()
}

fn sanitize_process_target(value: &str) -> Option<String> {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty()
        || trimmed.len() > 80
        || trimmed.contains("..")
        || trimmed.contains('\\')
        || trimmed.contains('/')
    {
        return None;
    }
    if trimmed.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ' ')
    }) {
        Some(trimmed)
    } else {
        None
    }
}

fn collect_snapshot_ids<'a>(details: impl IntoIterator<Item = &'a Value>) -> Vec<String> {
    details
        .into_iter()
        .filter_map(|details| details.pointer("/snapshot/id").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect()
}

fn read_focus_session() -> Option<FocusSession> {
    let raw = fs::read_to_string(focus_session_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_focus_session(session: &FocusSession) -> Result<(), String> {
    let path = focus_session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn focus_session_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join(FOCUS_SESSION_FILE)
}

fn focus_restore_report(message: &str) -> snapshot::RestoreReport {
    snapshot::RestoreReport {
        restored_snapshots: 0,
        failed_snapshots: 0,
        restored_entries: 0,
        failed_entries: 0,
        skipped_conflicts: 0,
        messages: vec![message.to_string()],
    }
}

fn spawn_focus_restore_monitor(session_id: String, expires_at: i64) {
    thread::spawn(move || {
        let _ = audit::record_event(
            "info",
            "focus.monitor_started",
            "Monitor de Modo Foco iniciado para restaurar a sessao no vencimento.",
            json!({
                "session_id": session_id,
                "expires_at": expires_at,
            }),
        );

        loop {
            thread::sleep(Duration::from_secs(5));
            let Some(session) = read_focus_session() else {
                return;
            };
            if session.id != session_id
                || session.restored_at.is_some()
                || session.status == "restored"
            {
                return;
            }
            if chrono::Utc::now().timestamp() >= expires_at {
                let report = restore_focus_session(Some("ttl_expired".to_string()));
                let _ = audit::record_event(
                    "info",
                    "focus.restored_after_ttl",
                    "Modo Foco restaurado automaticamente apos expirar.",
                    serde_json::to_value(&report).unwrap_or(Value::Null),
                );
                return;
            }
        }
    });
}

fn normalize_alias(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .replace(['\u{00e3}', '\u{00e1}', '\u{00e0}', '\u{00e2}'], "a")
        .replace(['\u{00e9}', '\u{00ea}'], "e")
        .replace('\u{00ed}', "i")
        .replace(['\u{00f3}', '\u{00f4}'], "o")
        .replace('\u{00fa}', "u")
        .replace('\u{00e7}', "c")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        effects_for_profile, focus_background_targets, profile_from_payload,
        ttl_seconds_from_payload, FocusProfile, MAX_FOCUS_TTL_SECONDS, MIN_FOCUS_TTL_SECONDS,
    };

    #[test]
    fn supports_common_focus_profile_aliases() {
        assert_eq!(
            profile_from_payload(Some(&json!({ "profile": "trabalho" }))),
            FocusProfile::Work
        );
        assert_eq!(
            profile_from_payload(Some(&json!({ "mode": "jogo" }))),
            FocusProfile::Game
        );
        assert_eq!(
            profile_from_payload(Some(&json!({ "scenario": "reuni\u{00e3}o" }))),
            FocusProfile::Call
        );
        assert_eq!(
            profile_from_payload(Some(&json!({ "focusProfile": "estudo" }))),
            FocusProfile::Study
        );
    }

    #[test]
    fn focus_effects_pause_costly_work_and_delay_uploads() {
        let effects = effects_for_profile(FocusProfile::Work);

        assert!(effects.suppress_agent_notifications);
        assert!(effects.pause_heavy_scans);
        assert!(effects.delay_non_critical_uploads);
        assert!(effects.background_quiet_mode);
        assert!(effects.visual_polling_min_interval_seconds >= 4);
    }

    #[test]
    fn call_profile_keeps_meeting_apps_out_of_quiet_targets() {
        let targets = focus_background_targets(FocusProfile::Call);

        assert!(!targets.contains(&"teams.exe"));
        assert!(!targets.contains(&"zoom.exe"));
        assert!(!targets.contains(&"chrome.exe"));
    }

    #[test]
    fn ttl_from_payload_is_clamped() {
        assert_eq!(
            ttl_seconds_from_payload(Some(&json!({ "ttlSeconds": 1 })), FocusProfile::Focus),
            MIN_FOCUS_TTL_SECONDS
        );
        assert_eq!(
            ttl_seconds_from_payload(
                Some(&json!({ "ttl_seconds": 999_999 })),
                FocusProfile::Focus
            ),
            MAX_FOCUS_TTL_SECONDS
        );
    }
}
