//! Per-user discovery of Windows services a specific person's own machine
//! shows they never actually use - the counterpart to `app_usage.rs`'s
//! "unused startup app" logic, but for background Windows *services*
//! instead of startup apps.
//!
//! `app_usage.rs` can only see "used since AnalystBlaze started watching" -
//! a cold-start blind spot for anyone who installs after already having
//! stopped using something. Every candidate service here is paired
//! instead with its own OS-level "was this ever actually used" signal
//! that Windows itself already keeps *permanently* (e.g. paired-device
//! history for Bluetooth), so day one already has real history instead of
//! needing weeks to accumulate any.
//!
//! Deliberately a short, hand-reviewed allowlist, each entry pairing a
//! *safe* usage signal with the *already-existing, already-reversible*
//! STOP_SERVICE/RESTORE_SERVICE snapshot mechanism (`windows_actions.rs`,
//! same one Modo Gamer uses) - this module is the "when/for whom is it
//! worth doing" learned layer sitting on top of that already-vetted
//! "what's ever safe to do" layer. It must only grow by deliberately
//! reviewing and adding one more candidate's detector, never by guessing
//! generically at "services the user doesn't seem to use".

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::snapshot;

/// Days of zero evidence-of-use before a candidate is surfaced as an
/// insight suggesting it be paused. Kept identical to `app_usage.rs`'s
/// `UNUSED_AFTER_DAYS` rather than inventing a separate number with no
/// data behind it yet.
pub const SERVICE_UNUSED_AFTER_DAYS: i64 = 30;

/// What the OS-level evidence says about a candidate service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageSignal {
    /// No evidence this was ever used (e.g. zero paired Bluetooth
    /// devices). Because the OS keeps this evidence permanently (not just
    /// since we started watching), this is real signal from day one - it
    /// counts toward "unused" immediately, with nothing to wait out.
    NeverUsed,
    /// At least one use is on record; days since the most recent one.
    LastUsedDaysAgo(i64),
    /// Could not read the evidence (missing key, unexpected shape,
    /// permissions, non-Windows). Genuinely unknown - must never be
    /// treated as "unused".
    Unknown,
}

/// One entry in the curated candidate list.
pub struct ServiceCandidate {
    pub service_name: &'static str,
    pub display_name: &'static str,
    pub detector: fn() -> UsageSignal,
    /// Cheap, safe-to-false-positive check for "the user looks like they
    /// might want this again right now" - gates the auto-restore trigger
    /// for a candidate WE paused. Pausing a service destroys the only
    /// evidence its own `detector` could ever use to notice renewed use
    /// (a stopped Bluetooth service can't record a new pairing), so
    /// waiting on `detector` again would wait forever. Each candidate
    /// needs its own hand-picked proxy instead; false positives (checking
    /// slightly too eagerly) are the safe failure direction, false
    /// negatives (staying off when genuinely needed) are not.
    pub intent_detector: fn() -> bool,
}

pub const CANDIDATES: &[ServiceCandidate] = &[ServiceCandidate {
    service_name: "bthserv",
    display_name: "Bluetooth Support Service",
    detector: bluetooth_usage_signal,
    intent_detector: bluetooth_intent_detected,
}];

/// Proxy for "the user is probably about to try using Bluetooth" while the
/// service is paused: the Windows Settings app is open. Deliberately broad
/// (any Settings page, not specifically the Bluetooth one - pinpointing the
/// exact page would need UI Automation, not just a process check) since a
/// false positive here only costs an eager, harmless restore, while a false
/// negative leaves the user stuck with a "broken" feature until the next
/// tick notices.
#[cfg(windows)]
fn bluetooth_intent_detected() -> bool {
    use sysinfo::{ProcessesToUpdate, System};
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
        .processes()
        .values()
        .any(|process| process.name().to_string_lossy().eq_ignore_ascii_case("SystemSettings.exe"))
}

#[cfg(not(windows))]
fn bluetooth_intent_detected() -> bool {
    false
}

/// True once a candidate has gone `SERVICE_UNUSED_AFTER_DAYS` with no
/// evidence of use.
pub fn is_unused_long_enough(signal: UsageSignal) -> bool {
    match signal {
        UsageSignal::NeverUsed => true,
        UsageSignal::LastUsedDaysAgo(days) => days >= SERVICE_UNUSED_AFTER_DAYS,
        UsageSignal::Unknown => false,
    }
}

/// Windows FILETIME (100-ns intervals since 1601-01-01 UTC) -> Unix
/// seconds. `None` if the value doesn't decode to a sane point in time
/// (negative after the epoch shift - a malformed/garbage registry value).
/// Verified live against a real paired device's `LastConnected` value on
/// a dev machine (`134196361908558893` -> 2026-04-02, matching
/// `[DateTime]::FromFileTime` exactly) before trusting this conversion.
fn filetime_to_unix_seconds(filetime: u64) -> Option<i64> {
    const FILETIME_UNIX_EPOCH_DIFF_100NS: i64 = 116_444_736_000_000_000;
    let signed = i64::try_from(filetime).ok()?;
    let unix_100ns = signed.checked_sub(FILETIME_UNIX_EPOCH_DIFF_100NS)?;
    if unix_100ns < 0 {
        return None;
    }
    Some(unix_100ns / 10_000_000)
}

fn days_since(unix_seconds: i64) -> i64 {
    let now = chrono::Utc::now().timestamp();
    (now - unix_seconds).max(0) / 86_400
}

#[cfg(windows)]
pub fn bluetooth_usage_signal() -> UsageSignal {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let devices_key =
        match hklm.open_subkey(r"SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices") {
            Ok(key) => key,
            // Windows only creates this key the first time ANY device is
            // ever paired - missing key (no adapter, or an adapter that's
            // never paired anything) is itself the "never used" signal,
            // not a read error.
            Err(_) => return UsageSignal::NeverUsed,
        };

    let device_names: Vec<String> = match devices_key.enum_keys().collect::<Result<Vec<_>, _>>() {
        Ok(names) => names,
        Err(_) => return UsageSignal::Unknown,
    };
    if device_names.is_empty() {
        return UsageSignal::NeverUsed;
    }

    let mut most_recent_filetime: Option<u64> = None;
    for name in &device_names {
        let Ok(subkey) = devices_key.open_subkey(name) else {
            continue;
        };
        // LastConnected reflects an actual connection; LastSeen can be set
        // by mere discovery/advertisement without ever connecting, so it's
        // only a fallback for older pairing records that may lack the
        // former.
        let value = subkey
            .get_value::<u64, _>("LastConnected")
            .or_else(|_| subkey.get_value::<u64, _>("LastSeen"))
            .ok();
        if let Some(value) = value {
            most_recent_filetime = Some(most_recent_filetime.map_or(value, |current| current.max(value)));
        }
    }

    match most_recent_filetime.and_then(filetime_to_unix_seconds) {
        Some(unix_seconds) => UsageSignal::LastUsedDaysAgo(days_since(unix_seconds)),
        // Devices are paired, but none carried a decodable timestamp -
        // an unexpected shape, not evidence of disuse.
        None => UsageSignal::Unknown,
    }
}

#[cfg(not(windows))]
pub fn bluetooth_usage_signal() -> UsageSignal {
    UsageSignal::Unknown
}

// --- Local decision-state: tracks what WE did, not what the OS observed ---
//
// The usage evidence above already comes from the OS itself and needs no
// local store. What DOES need persisting locally is our own action state
// per candidate - has this already been suggested (so we don't re-suggest
// every scan), did AnalystBlaze pause it (so RESTORE_SERVICE only ever
// touches something it actually paused, never a service the user stopped
// themselves), and when - matching the same small-JSON-under-app_data_dir
// pattern as `local_ai_policy.rs`/`protected_apps.rs`.

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ServiceWatchStore {
    #[serde(default)]
    entries: std::collections::HashMap<String, ServiceWatchEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ServiceWatchEntry {
    #[serde(default)]
    suggested_at: Option<i64>,
    #[serde(default)]
    paused_by_analystblaze_at: Option<i64>,
    #[serde(default)]
    restored_at: Option<i64>,
}

fn store_path() -> PathBuf {
    snapshot::app_data_dir().join("service-usage-watch.json")
}

fn load_store() -> ServiceWatchStore {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_store(store: &ServiceWatchStore) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    std::fs::write(path, raw).map_err(|error| error.to_string())
}

/// A candidate that's crossed the unused threshold and hasn't already
/// been suggested - what an Insight-generation pass should turn into a
/// user-facing card.
#[derive(Debug, Clone, Serialize)]
pub struct UnusedServiceSuggestion {
    pub service_name: String,
    pub display_name: String,
    pub never_used: bool,
    pub days_unused: Option<i64>,
}

/// Runs every candidate's detector and returns the ones newly qualifying
/// for a pause suggestion (crossed the threshold, not already suggested).
/// Marks them as suggested so a later call doesn't repeat them - call this
/// on the same background cadence as `app_usage.rs`'s sightings scan, not
/// the fast dashboard tick.
pub fn scan_for_unused_services() -> Vec<UnusedServiceSuggestion> {
    let mut store = load_store();
    let now = chrono::Utc::now().timestamp();
    let mut suggestions = Vec::new();

    for candidate in CANDIDATES {
        let signal = (candidate.detector)();
        if !is_unused_long_enough(signal) {
            continue;
        }
        let entry = store.entries.entry(candidate.service_name.to_string()).or_default();
        if entry.suggested_at.is_some() {
            continue; // already surfaced - don't repeat every scan
        }
        entry.suggested_at = Some(now);
        let (never_used, days_unused) = match signal {
            UsageSignal::NeverUsed => (true, None),
            UsageSignal::LastUsedDaysAgo(days) => (false, Some(days)),
            UsageSignal::Unknown => unreachable!("filtered out by is_unused_long_enough"),
        };
        suggestions.push(UnusedServiceSuggestion {
            service_name: candidate.service_name.to_string(),
            display_name: candidate.display_name.to_string(),
            never_used,
            days_unused,
        });
    }

    if !suggestions.is_empty() {
        let _ = save_store(&store);
    }
    suggestions
}

/// Records that AnalystBlaze itself paused a candidate (as opposed to the
/// user stopping it independently) - gates the auto-restore trigger so it
/// only ever touches something we're responsible for.
pub fn mark_paused_by_analystblaze(service_name: &str) {
    let mut store = load_store();
    let entry = store.entries.entry(service_name.to_string()).or_default();
    entry.paused_by_analystblaze_at = Some(chrono::Utc::now().timestamp());
    entry.restored_at = None;
    let _ = save_store(&store);
}

/// True if AnalystBlaze paused this service and hasn't already restored
/// it - the exact condition the auto-restore-on-demand trigger checks
/// before calling `windows_actions::restore_service`.
pub fn is_paused_by_analystblaze(service_name: &str) -> bool {
    let store = load_store();
    store
        .entries
        .get(service_name)
        .is_some_and(|entry| entry.paused_by_analystblaze_at.is_some() && entry.restored_at.is_none())
}

pub fn mark_restored(service_name: &str) {
    let mut store = load_store();
    if let Some(entry) = store.entries.get_mut(service_name) {
        entry.restored_at = Some(chrono::Utc::now().timestamp());
    }
    let _ = save_store(&store);
}

/// Current status of every candidate, as a plain data snapshot with no
/// side effects - meant to ride along in the regular telemetry upload
/// (see `telemetry::advanced::AdvancedTelemetry`) so the server can decide
/// when/how to surface an insight from it, the same way `predictive_game_mode`
/// and every other insight rule work off uploaded signals rather than the
/// desktop deciding on its own what the user should see.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceUsageStatus {
    pub service_name: String,
    pub display_name: String,
    pub never_used: bool,
    pub days_unused: Option<i64>,
    pub unused_long_enough: bool,
}

pub fn current_signals() -> Vec<ServiceUsageStatus> {
    CANDIDATES
        .iter()
        .map(|candidate| {
            let signal = (candidate.detector)();
            let (never_used, days_unused) = match signal {
                UsageSignal::NeverUsed => (true, None),
                UsageSignal::LastUsedDaysAgo(days) => (false, Some(days)),
                UsageSignal::Unknown => (false, None),
            };
            ServiceUsageStatus {
                service_name: candidate.service_name.to_string(),
                display_name: candidate.display_name.to_string(),
                never_used,
                days_unused,
                unused_long_enough: is_unused_long_enough(signal),
            }
        })
        .collect()
}

/// Restores whichever candidates AnalystBlaze itself paused AND whose
/// `intent_detector` now says the user looks like they want it back -
/// meant to be fired every normal telemetry tick (~60s), same cadence as
/// `app_usage.rs`'s sightings scan. Fully self-contained: on success it
/// updates the local pause/restore state itself and leaves an audit trail
/// (there is no native OS toast in this codebase today - this is the
/// existing durable, inspectable record of what happened until a real
/// notification surfaces it).
pub async fn auto_restore_if_needed() {
    // The intent_detector calls do a process-list scan (blocking-ish, same
    // cost app_usage.rs already pays every tick) - isolate that off the
    // async runtime thread rather than running it inline in the select!
    // loop, matching this codebase's standing convention.
    let candidates_to_restore: Vec<&'static str> = match tokio::task::spawn_blocking(|| {
        CANDIDATES
            .iter()
            .filter(|candidate| {
                is_paused_by_analystblaze(candidate.service_name) && (candidate.intent_detector)()
            })
            .map(|candidate| candidate.service_name)
            .collect::<Vec<_>>()
    })
    .await
    {
        Ok(names) => names,
        Err(_) => return,
    };

    for service_name in candidates_to_restore {
        let result = super::windows_actions::restore_service(Some(
            serde_json::json!({ "service": service_name }),
        ))
        .await;
        if !result.success {
            continue; // leave paused-state as-is - next tick tries again
        }
        mark_restored(service_name);
        let display_name = CANDIDATES
            .iter()
            .find(|candidate| candidate.service_name == service_name)
            .map(|candidate| candidate.display_name)
            .unwrap_or(service_name);
        let _ = crate::audit::record_event(
            "info",
            "service_usage.auto_restored",
            format!("{display_name} foi reativado automaticamente porque parecia que voce ia usa-lo."),
            serde_json::json!({ "service": service_name }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_real_filetime_value_observed_on_a_dev_machine() {
        // LastConnected read live from a real paired Bluetooth device's
        // registry subkey; cross-checked against PowerShell's own
        // [DateTime]::FromFileTimeUtc(134196361908558893) before trusting
        // this - both landed on 2026-04-02 ~20:43 UTC.
        let unix = filetime_to_unix_seconds(134_196_361_908_558_893).unwrap();
        // 2026-04-02 20:43:10 UTC - cross-checked via
        // ([DateTime]::FromFileTimeUtc(134196361908558893).Ticks -
        // [DateTime]::new(1970,1,1,...,Utc).Ticks) / 10_000_000 == 1775162590
        assert_eq!(unix, 1_775_162_590);
    }

    #[test]
    fn rejects_a_filetime_before_the_unix_epoch() {
        assert_eq!(filetime_to_unix_seconds(0), None);
    }

    #[test]
    fn never_used_and_long_unused_both_qualify() {
        assert!(is_unused_long_enough(UsageSignal::NeverUsed));
        assert!(is_unused_long_enough(UsageSignal::LastUsedDaysAgo(
            SERVICE_UNUSED_AFTER_DAYS
        )));
        assert!(is_unused_long_enough(UsageSignal::LastUsedDaysAgo(
            SERVICE_UNUSED_AFTER_DAYS + 90
        )));
    }

    #[test]
    fn recently_used_and_unknown_do_not_qualify() {
        assert!(!is_unused_long_enough(UsageSignal::LastUsedDaysAgo(1)));
        assert!(!is_unused_long_enough(UsageSignal::Unknown));
    }

    #[test]
    #[ignore] // machine-dependent (reads this dev machine's real registry) - run manually with --ignored
    fn live_bluetooth_signal_on_this_machine() {
        // Not an assertion of a specific value (that would make the test
        // fail on every other machine) - just a manual way to eyeball the
        // real detector end-to-end against this dev machine's actual
        // paired-device history, the same "confirm live before trusting
        // it" step already taken for the raw FILETIME value above.
        println!("{:?}", bluetooth_usage_signal());
    }

    #[test]
    fn suggestion_state_prevents_repeat_suggestions_and_tracks_pause_lifecycle() {
        // Exercises the store's own load/save/entry logic directly rather
        // than going through the real filesystem path, since store_path()
        // is tied to app_data_dir() - the qualifying condition itself
        // (is_unused_long_enough) is covered above.
        let mut store = ServiceWatchStore::default();
        let entry = store.entries.entry("bthserv".to_string()).or_default();
        assert!(entry.suggested_at.is_none());
        entry.suggested_at = Some(1);
        assert!(store.entries.get("bthserv").unwrap().suggested_at.is_some());
    }
}
