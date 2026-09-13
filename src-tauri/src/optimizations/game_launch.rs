//! "The game closes seconds after opening, again and again" - detected and
//! diagnosed entirely on this machine, so that the Insights screen can tell
//! the user *why* instead of leaving them to reinstall blindly.
//!
//! The symptom is cheap to see: Modo Gamer activates when the game process
//! appears and restores when it exits, so a session that lasted under
//! FAST_EXIT_SECONDS, more than once in a short window, is a launch that
//! keeps failing. The diagnosis is a small catalogue of known causes,
//! grown from real cases - the first one (2026-09-13) was a Steam-purchased
//! Rockstar title that refused to start with "support for Windows 7 and 8
//! has ended": steam.exe had a Windows 8 compatibility layer, and Windows
//! hands that layer down to every child process (via __COMPAT_LAYER), so
//! the Rockstar launcher genuinely believed it was on Windows 8.
//!
//! Privacy boundary: the session history (with process names) never leaves
//! `app_data_dir()`, and the registry paths read here are never reported.
//! The only thing that reaches the server is `GameLaunchIssue` - a category,
//! a launcher *kind*, a layer token and a count - which is what the server
//! needs to render the card and nothing more.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use super::snapshot;

const HISTORY_LIMIT: usize = 30;
/// A real play session is minutes to hours; a launcher that bounces is gone
/// in well under a minute even on a slow disk.
const FAST_EXIT_SECONDS: i64 = 90;
const REPEAT_WINDOW_SECONDS: i64 = 15 * 60;
const REPEAT_THRESHOLD: usize = 2;

/// Compatibility layer tokens that make Windows lie about its own version to
/// the process. RUNASADMIN, HIGHDPIAWARE etc. are harmless and ignored.
const OS_VERSION_LAYERS: &[&str] = &[
    "WIN95", "WIN98", "WINXPSP2", "WINXPSP3", "VISTARTM", "VISTASP1", "VISTASP2", "WIN7RTM", "WIN8RTM",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionEnd {
    process_name: String,
    started_at: i64,
    ended_at: i64,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameLaunchIssue {
    /// "compat_layer_on_game" (the game's own exe carries the layer) or
    /// "compat_layer_inherited" (a launcher it is started through does).
    pub kind: String,
    /// Launcher kind the layer was found on - "steam", "epic", "rockstar",
    /// "ubisoft", "ea", "battlenet", "gog", "riot". None for the game itself.
    pub launcher: Option<String>,
    /// The offending token, e.g. "WIN8RTM".
    pub layer: String,
    pub fast_exits: u32,
}

fn history_path() -> PathBuf {
    snapshot::app_data_dir().join("game-launch-history.json")
}

fn read_history() -> Vec<SessionEnd> {
    fs::read_to_string(history_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Called whenever a Modo Gamer session ends. Only sessions that ended
/// because the target process exited are meaningful here; manual restores,
/// timeouts and startup reconciliation say nothing about the game.
pub fn record_session_end(process_name: Option<&str>, started_at: i64, ended_at: i64, reason: &str) {
    let Some(process_name) = process_name.map(str::trim).filter(|name| !name.is_empty()) else {
        return;
    };
    let mut history = read_history();
    history.push(SessionEnd {
        process_name: process_name.to_string(),
        started_at,
        ended_at,
        reason: reason.to_string(),
    });
    if history.len() > HISTORY_LIMIT {
        let excess = history.len() - HISTORY_LIMIT;
        history.drain(..excess);
    }
    if let Some(parent) = history_path().parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string(&history) {
        let _ = fs::write(history_path(), raw);
    }
}

/// The process that has been bouncing, and how many times, if any.
fn repeated_fast_exits(history: &[SessionEnd], now: i64) -> Option<(String, u32)> {
    let mut best: Option<(String, u32)> = None;
    let mut seen = std::collections::HashSet::new();
    for end in history {
        if !seen.insert(end.process_name.to_ascii_lowercase()) {
            continue;
        }
        let count = history
            .iter()
            .filter(|other| {
                other.process_name.eq_ignore_ascii_case(&end.process_name)
                    && other.reason == "target_process_exit"
                    && other.ended_at - other.started_at < FAST_EXIT_SECONDS
                    && now - other.ended_at <= REPEAT_WINDOW_SECONDS
            })
            .count();
        if count >= REPEAT_THRESHOLD && best.as_ref().is_none_or(|(_, n)| count as u32 > *n) {
            best = Some((end.process_name.clone(), count as u32));
        }
    }
    best
}

/// The OS-version token in an AppCompatFlags\Layers value such as
/// "~ RUNASADMIN WIN8RTM", if there is one.
fn os_compat_layer(flags: &str) -> Option<String> {
    flags
        .split_whitespace()
        .map(|token| token.trim_start_matches('~').to_ascii_uppercase())
        .find(|token| OS_VERSION_LAYERS.contains(&token.as_str()))
}

fn file_name_lower(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase()
}

/// Which store front / launcher an executable belongs to. "launcher.exe" is
/// too generic on its own (Rockstar's is literally called that), so that one
/// is only accepted inside a Rockstar Games folder.
fn launcher_kind(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    let name = file_name_lower(path);
    match name.as_str() {
        "steam.exe" => Some("steam"),
        "epicgameslauncher.exe" => Some("epic"),
        "rockstarsteamhelper.exe" | "launcherpatcher.exe" | "socialclubhelper.exe" => Some("rockstar"),
        "launcher.exe" if lower.contains("rockstar") => Some("rockstar"),
        "upc.exe" | "ubisoftconnect.exe" | "ubisoftgamelauncher.exe" => Some("ubisoft"),
        "eadesktop.exe" | "origin.exe" => Some("ea"),
        "battle.net.exe" | "battle.net launcher.exe" => Some("battlenet"),
        "galaxyclient.exe" => Some("gog"),
        "riotclientservices.exe" => Some("riot"),
        _ => None,
    }
}

/// Pure: given the bouncing process and every (path, flags) pair from the
/// compatibility registry, name the cause if it is one we recognise. The
/// game's own exe wins over a launcher, since that is the more direct fault.
fn diagnose(process_name: &str, fast_exits: u32, layers: &[(String, String)]) -> Option<GameLaunchIssue> {
    let game = process_name.to_ascii_lowercase();
    let mut inherited: Option<GameLaunchIssue> = None;
    for (path, flags) in layers {
        let Some(layer) = os_compat_layer(flags) else {
            continue;
        };
        if file_name_lower(path) == game {
            return Some(GameLaunchIssue {
                kind: "compat_layer_on_game".to_string(),
                launcher: None,
                layer,
                fast_exits,
            });
        }
        if inherited.is_none() {
            if let Some(kind) = launcher_kind(path) {
                inherited = Some(GameLaunchIssue {
                    kind: "compat_layer_inherited".to_string(),
                    launcher: Some(kind.to_string()),
                    layer,
                    fast_exits,
                });
            }
        }
    }
    inherited
}

#[cfg(windows)]
fn read_compat_layers() -> Vec<(String, String)> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    const SUBKEY: &str = "Software\\Microsoft\\Windows NT\\CurrentVersion\\AppCompatFlags\\Layers";
    let mut out = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let Ok(key) = RegKey::predef(hive).open_subkey(SUBKEY) else {
            continue;
        };
        for entry in key.enum_values().flatten() {
            let (name, value) = entry;
            let flags: String = String::from_utf16_lossy(
                &value
                    .bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .take_while(|unit| *unit != 0)
                    .collect::<Vec<_>>(),
            );
            out.push((name, flags));
        }
    }
    out
}

#[cfg(not(windows))]
fn read_compat_layers() -> Vec<(String, String)> {
    Vec::new()
}

/// What the advanced-telemetry refresh reports. Only ever non-None while the
/// symptom is live (a game bounced at least twice in the last 15 minutes)
/// AND a known cause was found - a bouncing game with no recognised cause
/// stays silent rather than guessing.
pub fn current_issue() -> Option<GameLaunchIssue> {
    let history = read_history();
    let (process_name, fast_exits) = repeated_fast_exits(&history, chrono::Utc::now().timestamp())?;
    diagnose(&process_name, fast_exits, &read_compat_layers())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end(name: &str, started: i64, ended: i64, reason: &str) -> SessionEnd {
        SessionEnd {
            process_name: name.to_string(),
            started_at: started,
            ended_at: ended,
            reason: reason.to_string(),
        }
    }

    #[test]
    fn two_fast_exits_of_the_same_game_within_the_window_count() {
        let now = 10_000;
        let history = vec![
            end("RDR2.exe", now - 600, now - 560, "target_process_exit"),
            end("rdr2.exe", now - 120, now - 60, "target_process_exit"),
        ];
        assert_eq!(repeated_fast_exits(&history, now), Some(("RDR2.exe".to_string(), 2)));
    }

    #[test]
    fn a_real_play_session_is_not_a_fast_exit() {
        let now = 10_000;
        let history = vec![
            end("RDR2.exe", now - 4000, now - 400, "target_process_exit"),
            end("RDR2.exe", now - 120, now - 60, "target_process_exit"),
        ];
        assert_eq!(repeated_fast_exits(&history, now), None);
    }

    #[test]
    fn manual_restores_and_old_exits_do_not_count() {
        let now = 10_000;
        let history = vec![
            end("RDR2.exe", now - 120, now - 100, "manual_restore"),
            end("RDR2.exe", now - 5000, now - 4980, "target_process_exit"),
            end("RDR2.exe", now - 60, now - 30, "target_process_exit"),
        ];
        assert_eq!(repeated_fast_exits(&history, now), None);
    }

    #[test]
    fn only_os_version_layers_are_a_cause() {
        assert_eq!(os_compat_layer("~ RUNASADMIN WIN8RTM"), Some("WIN8RTM".to_string()));
        assert_eq!(os_compat_layer("~ WIN8RTM WIN7RTM"), Some("WIN8RTM".to_string()));
        assert_eq!(os_compat_layer("~ RUNASADMIN"), None);
        assert_eq!(os_compat_layer("HIGHDPIAWARE"), None);
        assert_eq!(os_compat_layer(""), None);
    }

    #[test]
    fn launcher_kinds_are_recognised_by_file_name_only() {
        assert_eq!(launcher_kind(r"C:\Program Files (x86)\Steam\steam.exe"), Some("steam"));
        assert_eq!(launcher_kind(r"C:\Program Files\Rockstar Games\Launcher\Launcher.exe"), Some("rockstar"));
        // A generic launcher.exe outside a Rockstar folder is not assumed to be one.
        assert_eq!(launcher_kind(r"D:\Games\Some Indie\launcher.exe"), None);
        assert_eq!(launcher_kind(r"D:\SteamLibrary\steamapps\common\RDR2\RDR2.exe"), None);
    }

    #[test]
    fn the_real_case_steam_on_win8_layer_is_diagnosed_as_inherited() {
        // Exactly what was on the machine that motivated this module.
        let layers = vec![
            (r"C:\Program Files (x86)\Steam\steam.exe".to_string(), "~ WIN8RTM".to_string()),
            (r"D:\SteamLibrary\steamapps\common\Red Dead Redemption 2\RDR2.exe".to_string(), "~ RUNASADMIN".to_string()),
            (r"C:\Program Files\Rockstar Games\Launcher\Launcher.exe".to_string(), "~ RUNASADMIN".to_string()),
        ];
        let issue = diagnose("RDR2.exe", 3, &layers).expect("should be diagnosed");
        assert_eq!(issue.kind, "compat_layer_inherited");
        assert_eq!(issue.launcher.as_deref(), Some("steam"));
        assert_eq!(issue.layer, "WIN8RTM");
        assert_eq!(issue.fast_exits, 3);
    }

    #[test]
    fn a_layer_on_the_game_itself_wins_over_a_launcher() {
        let layers = vec![
            (r"C:\Program Files (x86)\Steam\steam.exe".to_string(), "~ WIN8RTM".to_string()),
            (r"D:\Games\Old Game\oldgame.exe".to_string(), "~ WIN7RTM".to_string()),
        ];
        let issue = diagnose("OLDGAME.EXE", 2, &layers).unwrap();
        assert_eq!(issue.kind, "compat_layer_on_game");
        assert_eq!(issue.launcher, None);
        assert_eq!(issue.layer, "WIN7RTM");
    }

    #[test]
    #[ignore = "reads this machine's real compatibility registry"]
    fn live_compat_layers_on_this_machine() {
        let layers = read_compat_layers();
        println!("entries: {}", layers.len());
        for (path, flags) in &layers {
            if let Some(layer) = os_compat_layer(flags) {
                println!("  {layer:8} launcher={:?}  {path}", launcher_kind(path));
            }
        }
        println!("diagnose(RDR2.exe) = {:?}", diagnose("RDR2.exe", 2, &layers));
    }

    #[test]
    fn run_as_admin_alone_is_not_diagnosed() {
        let layers = vec![
            (r"C:\Program Files (x86)\Steam\steam.exe".to_string(), "~ RUNASADMIN".to_string()),
            (r"D:\SteamLibrary\steamapps\common\RDR2\RDR2.exe".to_string(), "~ RUNASADMIN HIGHDPIAWARE".to_string()),
        ];
        assert_eq!(diagnose("RDR2.exe", 2, &layers), None);
    }
}
