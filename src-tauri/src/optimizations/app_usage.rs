use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};

use super::snapshot;
use super::windows_inventory;

/// Entries not seen running for this long are dropped from the store so it
/// stays bounded even across app reinstalls/replaced startup entries,
/// instead of accumulating forever.
const STALE_AFTER_DAYS: i64 = 400;

/// How "unused" a safe startup app has to be before `last_used_recommendation`
/// upgrades it from "delay" to "disable". Chosen to comfortably clear a
/// user's typical monthly cadence for apps they do still use occasionally.
const UNUSED_AFTER_DAYS: i64 = 30;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AppUsageStore {
    #[serde(default)]
    entries: HashMap<String, AppUsageEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppUsageEntry {
    first_seen_at: i64,
    last_seen_at: i64,
    seen_count: u64,
}

/// Cross-references the current process list against the executable names
/// implied by the user's startup apps, and bumps `last_seen_at` for the ones
/// actually running right now.
///
/// Deliberately scoped to startup-app names only (a few dozen at most)
/// rather than the full running process list (200-400+ entries) - the
/// latter can't be sent/stored in full on every tick without either an
/// arbitrary truncation (unreliable for "was X used recently") or unbounded
/// growth, while the startup-app set is small and exactly what
/// `last_used_recommendation` below needs.
///
/// Meant to run on the ~60s normal telemetry cadence, not the 2s dashboard
/// cadence - call via `tokio::task::spawn_blocking` from async contexts,
/// matching `scan_startup_impact_blocking`'s pattern.
pub fn record_startup_app_sightings_blocking() {
    let startup_apps = windows_inventory::collect_windows_inventory().startup_apps;
    if startup_apps.is_empty() {
        return;
    }

    let watched: Vec<String> = startup_apps
        .iter()
        .filter_map(|app| exe_name_from_command(&app.command).or_else(|| exe_name_from_command(&app.name)))
        .collect();
    if watched.is_empty() {
        return;
    }

    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let running: HashSet<String> = system
        .processes()
        .values()
        .map(|process| process.name().to_string_lossy().trim().to_ascii_lowercase())
        .collect();

    let now = chrono::Utc::now().timestamp();
    let mut store = load_store();
    let mut changed = false;
    for name in watched {
        if !running.contains(&name) {
            continue;
        }
        let entry = store.entries.entry(name).or_insert(AppUsageEntry {
            first_seen_at: now,
            last_seen_at: now,
            seen_count: 0,
        });
        entry.last_seen_at = now;
        entry.seen_count = entry.seen_count.saturating_add(1);
        changed = true;
    }

    if changed {
        prune_stale(&mut store, now);
        let _ = write_json_file(&usage_store_path(), &store);
    }
}

/// Days since a startup app (matched the same way `record_startup_app_sightings_blocking`
/// matches it - command first, then display name) was last seen running.
/// `None` means "never observed running" - either genuinely unused since
/// tracking started, or the app predates this feature's rollout.
fn days_since_last_seen(command: &str, display_name: &str) -> Option<i64> {
    let name = exe_name_from_command(command).or_else(|| exe_name_from_command(display_name))?;
    let store = load_store();
    let entry = store.entries.get(&name)?;
    let now = chrono::Utc::now().timestamp();
    Some(((now - entry.last_seen_at).max(0)) / 86_400)
}

/// Upgrades a name-heuristic startup recommendation to "disable" once usage
/// data shows the app genuinely hasn't run in a while - never downgrades a
/// non-safe recommendation, and never recommends disabling something we
/// have no usage history for yet (that's "insufficient data", not "unused").
pub fn last_used_recommendation(
    command: &str,
    display_name: &str,
    risk: &str,
    base_recommendation: &str,
) -> (String, Option<i64>) {
    let days_unused = days_since_last_seen(command, display_name);
    let recommendation = apply_unused_threshold(base_recommendation, risk, days_unused);
    (recommendation, days_unused)
}

fn apply_unused_threshold(base_recommendation: &str, risk: &str, days_unused: Option<i64>) -> String {
    if risk == "safe" && days_unused.is_some_and(|days| days >= UNUSED_AFTER_DAYS) {
        "disable".to_string()
    } else {
        base_recommendation.to_string()
    }
}

fn prune_stale(store: &mut AppUsageStore, now: i64) {
    let cutoff = now - STALE_AFTER_DAYS * 86_400;
    store.entries.retain(|_, entry| entry.last_seen_at >= cutoff);
}

/// Extracts a bare, lowercase executable file name (e.g. "discord.exe") from
/// a startup command line or display name. Registry `command` values are
/// unreliable to tokenize by whitespace - an unquoted path containing
/// spaces (`C:\Program Files\Spotify\Spotify.exe /minimized`) is genuinely
/// ambiguous to split without probing the filesystem - so this instead
/// anchors on the first ".exe" occurrence (quoted or not) and takes the
/// path's file name from everything up to and including it. Only returns
/// something for inputs that plausibly identify an executable.
fn exe_name_from_command(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let exe_end = lower.find(".exe")? + 4;
    let candidate = lower[..exe_end].trim_start_matches('"').trim();
    Path::new(candidate).file_name()?.to_str().map(str::to_string)
}

fn load_store() -> AppUsageStore {
    read_json_file(&usage_store_path()).unwrap_or_default()
}

fn usage_store_path() -> PathBuf {
    snapshot::app_data_dir().join("app-usage.json")
}

fn read_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let raw = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, raw).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_exe_name_from_quoted_command_with_args() {
        assert_eq!(
            exe_name_from_command("\"C:\\Users\\vitor\\AppData\\Local\\Discord\\Update.exe\" --processStart Discord.exe"),
            Some("update.exe".to_string())
        );
    }

    #[test]
    fn extracts_exe_name_from_unquoted_command() {
        assert_eq!(
            exe_name_from_command("C:\\Program Files\\Spotify\\Spotify.exe /minimized"),
            Some("spotify.exe".to_string())
        );
    }

    #[test]
    fn returns_none_when_first_token_has_no_exe_extension() {
        assert_eq!(exe_name_from_command("rundll32.dll,SomeEntryPoint"), None);
        assert_eq!(exe_name_from_command(""), None);
    }

    #[test]
    fn falls_back_to_display_name_when_command_has_no_exe() {
        assert_eq!(exe_name_from_command("OneDrive"), None);
    }

    #[test]
    fn upgrades_to_disable_once_unused_past_the_threshold() {
        assert_eq!(
            apply_unused_threshold("delay", "safe", Some(UNUSED_AFTER_DAYS)),
            "disable"
        );
        assert_eq!(
            apply_unused_threshold("delay", "safe", Some(UNUSED_AFTER_DAYS + 90)),
            "disable"
        );
    }

    #[test]
    fn keeps_base_recommendation_when_recently_used_or_unknown() {
        assert_eq!(apply_unused_threshold("delay", "safe", Some(1)), "delay");
        assert_eq!(apply_unused_threshold("observe", "safe", None), "observe");
    }

    #[test]
    fn never_recommends_disabling_a_non_safe_entry() {
        assert_eq!(
            apply_unused_threshold("keep", "sensitive", Some(UNUSED_AFTER_DAYS + 90)),
            "keep"
        );
    }
}
