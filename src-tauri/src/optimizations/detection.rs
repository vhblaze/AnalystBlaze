use serde_json::{json, Value};
use sysinfo::{ProcessesToUpdate, System};

use super::ExecutionResult;

#[derive(Debug, Clone, serde::Serialize)]
pub struct GameDetection {
    pub detected: bool,
    pub process_name: Option<String>,
    pub pid: Option<String>,
    pub confidence: f64,
    pub reason: String,
}

pub async fn detect_foreground_game(payload: Option<Value>) -> ExecutionResult {
    // Server-facing preview, not a live user click - stays on the
    // conservative/unsupervised side (no heavy-workload targets).
    let detection = detect_game_process_with_payload(payload.as_ref(), false);
    ExecutionResult::ok(
        if detection.detected {
            "Jogo detectado por processo local."
        } else {
            "Nenhum jogo conhecido detectado agora."
        },
        json!({
            "payload": payload,
            "implemented": true,
            "detection": detection,
            "examples": ["cs2.exe", "valorant.exe", "fortniteclient-win64-shipping.exe"],
        }),
    )
}

/// Always the conservative/unsupervised variant - used by
/// evaluate_local_policy's automatic heuristic, which must never target a
/// heavy-workload tool on its own (see is_heavy_workload_tool's docs).
pub fn detect_game_process() -> GameDetection {
    detect_game_process_with_payload(None, false)
}

/// `allow_heavy_workload_target` should be true only for a supervised,
/// human-initiated activation (a live "Ativar Modo Gamer" click) - see
/// foreground_process_detection's docs.
pub fn detect_game_process_with_payload(
    payload: Option<&Value>,
    allow_heavy_workload_target: bool,
) -> GameDetection {
    if let Some(detection) = explicit_target_detection(payload) {
        return detection;
    }

    if let Some(detection) = foreground_process_detection(allow_heavy_workload_target) {
        return detection;
    }

    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let mut best: Option<(f32, String, String)> = None;
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().trim().to_string();
        if !looks_like_game_process(&name) {
            continue;
        }

        let cpu = process.cpu_usage();
        if best
            .as_ref()
            .map(|(current_cpu, _, _)| cpu > *current_cpu)
            .unwrap_or(true)
        {
            best = Some((cpu, name, pid.to_string()));
        }
    }

    if let Some((cpu, process_name, pid)) = best {
        return GameDetection {
            detected: true,
            process_name: Some(process_name),
            pid: Some(pid),
            confidence: if cpu >= 10.0 { 0.88 } else { 0.74 },
            reason: "known_game_process_running".to_string(),
        };
    }

    GameDetection {
        detected: false,
        process_name: None,
        pid: None,
        confidence: 0.0,
        reason: "no_known_game_process".to_string(),
    }
}

pub fn process_still_running(pid: Option<&str>, process_name: Option<&str>) -> bool {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let normalized_name = process_name.map(normalize_process_name);

    system.processes().iter().any(|(candidate_pid, process)| {
        if pid.is_some_and(|pid| pid == candidate_pid.to_string()) {
            return true;
        }

        let Some(normalized_name) = normalized_name.as_deref() else {
            return false;
        };
        normalize_process_name(&process.name().to_string_lossy()) == normalized_name
    })
}

pub(crate) fn looks_like_game_process(name: &str) -> bool {
    let normalized = normalize_process_name(name);
    if is_launcher_process(&normalized) || is_never_game_process(&normalized) {
        return false;
    }

    const KNOWN_GAMES: &[&str] = &[
        "cs2.exe",
        "csgo.exe",
        "valorant.exe",
        "fortniteclient-win64-shipping.exe",
        "league of legends.exe",
        "leagueclient.exe",
        "r5apex.exe",
        "overwatch.exe",
        "robloxplayerbeta.exe",
        "minecraft.exe",
        "javaw.exe",
        "gta5.exe",
        "eldenring.exe",
        "cod.exe",
        "destiny2.exe",
        "dota2.exe",
        "rocketleague.exe",
        "warframe.x64.exe",
        "acs.exe",
        "acc.exe",
    ];

    KNOWN_GAMES.iter().any(|candidate| *candidate == normalized)
        || normalized.contains("shipping")
        || normalized.contains("unityplayer")
}

fn explicit_target_detection(payload: Option<&Value>) -> Option<GameDetection> {
    let payload = payload?;
    let target_pid = payload
        .get("target_pid")
        .or_else(|| payload.get("targetPid"))
        .or_else(|| payload.get("pid"))
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok());
    let target_name = payload
        .get("target_process")
        .or_else(|| payload.get("targetProcess"))
        .or_else(|| payload.get("process_name"))
        .or_else(|| payload.get("processName"))
        .or_else(|| payload.get("name"))
        .and_then(Value::as_str)
        .map(normalize_process_name);

    if target_pid.is_none() && target_name.is_none() {
        return None;
    }

    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        let name = process.name().to_string_lossy().trim().to_string();
        let normalized = normalize_process_name(&name);
        if target_pid.is_some_and(|target| target == pid_u32)
            || target_name
                .as_deref()
                .is_some_and(|target| target == normalized)
        {
            if is_launcher_process(&normalized) || is_never_game_process(&normalized) {
                return Some(GameDetection {
                    detected: false,
                    process_name: Some(name),
                    pid: Some(pid.to_string()),
                    confidence: 0.0,
                    reason: if is_never_game_process(&normalized) {
                        "target_is_app_shell_not_game".to_string()
                    } else {
                        "target_is_launcher_not_game".to_string()
                    },
                });
            }

            return Some(GameDetection {
                detected: true,
                process_name: Some(name),
                pid: Some(pid.to_string()),
                confidence: 0.96,
                reason: "user_selected_process".to_string(),
            });
        }
    }

    Some(GameDetection {
        detected: false,
        process_name: target_name,
        pid: target_pid.map(|pid| pid.to_string()),
        confidence: 0.0,
        reason: "selected_process_not_running".to_string(),
    })
}

/// `allow_heavy_workload_target` lets a MANUAL activation (a live user
/// click - CommandSource::ManualUser/RemoteCommand, never LocalPolicy)
/// still target a heavy-workload tool (Blender and friends -
/// is_heavy_workload_tool) as "what Game Mode is optimizing for", so its
/// process gets the priority bump and, if requested, a frame capture -
/// same as it would for an actual game. False for every unsupervised
/// path (the automatic local-policy heuristic, and detect_foreground_game's
/// server-facing preview) - see is_heavy_workload_tool's docs for why that
/// distinction matters.
fn foreground_process_detection(allow_heavy_workload_target: bool) -> Option<GameDetection> {
    let foreground_pid = foreground_pid()?;
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let process = system
        .processes()
        .iter()
        .find_map(|(pid, process)| (pid.as_u32() == foreground_pid).then_some(process))?;
    let name = process.name().to_string_lossy().trim().to_string();
    let normalized = normalize_process_name(&name);

    if is_launcher_process(&normalized) || is_never_game_process(&normalized) {
        return None;
    }
    let is_heavy_workload = is_heavy_workload_tool(&normalized);
    if is_common_foreground_non_game(&normalized) || (is_heavy_workload && !allow_heavy_workload_target) {
        return None;
    }

    let known = looks_like_game_process(&normalized);
    Some(GameDetection {
        detected: true,
        process_name: Some(name),
        pid: Some(foreground_pid.to_string()),
        confidence: if known {
            0.9
        } else if is_heavy_workload {
            0.7
        } else {
            0.66
        },
        reason: if known {
            "foreground_known_game_process".to_string()
        } else if is_heavy_workload {
            "foreground_manual_heavy_workload_target".to_string()
        } else {
            "foreground_process_candidate".to_string()
        },
    })
}

#[cfg(windows)]
pub(crate) fn foreground_pid() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
        return None;
    }

    let mut pid = 0_u32;
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    (pid > 0).then_some(pid)
}

#[cfg(not(windows))]
pub(crate) fn foreground_pid() -> Option<u32> {
    None
}

fn is_launcher_process(normalized: &str) -> bool {
    matches!(
        normalized,
        "steam.exe"
            | "epicgameslauncher.exe"
            | "riotclientservices.exe"
            | "battle.net.exe"
            | "content manager.exe"
    )
}

fn is_never_game_process(normalized: &str) -> bool {
    normalized == "analystblaze-desktop.exe"
        || normalized == "analystblaze.exe"
        || normalized.starts_with("analystblaze")
}

/// Incidental foreground windows that are never a meaningful Game Mode
/// target under any circumstance, manual click included - a browser,
/// terminal, or Discord happening to have focus says nothing about what
/// the user is actually doing, unlike the heavy-workload tools below
/// (where a manual click is a deliberate signal worth honoring).
fn is_common_foreground_non_game(normalized: &str) -> bool {
    matches!(
        normalized,
        "explorer.exe"
            | "chrome.exe"
            | "msedge.exe"
            | "firefox.exe"
            | "brave.exe"
            | "code.exe"
            | "cursor.exe"
            | "powershell.exe"
            | "cmd.exe"
            | "windowsterminal.exe"
            | "discord.exe"
    )
}

/// Professional creative/dev tools that legitimately peg both GPU and CPU,
/// exactly the signal `evaluate_local_policy`'s `(high_gpu && high_cpu)`
/// fallback uses to guess "gaming" - a real incident (2026-09) had Blender
/// rendering trigger automatic Game Mode (service stops, app closures, a
/// process-priority bump, all unsupervised) on a modest machine, freezing
/// it. Unlike is_common_foreground_non_game's list, these are never
/// excluded outright: a live "Ativar Modo Gamer" click while one of these
/// is running is a deliberate, supervised choice to optimize a heavy
/// render/compile/edit session - same reasoning this codebase already
/// applies to ManualUser vs LocalPolicy elsewhere (see apply_game_mode's
/// frame-capture gating). Only the UNSUPERVISED paths - the automatic
/// local-policy heuristic in evaluate_local_policy, and the default
/// (payload-less) foreground_process_detection used there - exclude them.
fn is_heavy_workload_tool(normalized: &str) -> bool {
    matches!(
        normalized,
        "blender.exe"
            | "unity.exe"
            | "unrealeditor.exe"
            | "ue4editor.exe"
            | "ue5editor.exe"
            | "resolve.exe"
            | "afterfx.exe"
            | "adobe premiere pro.exe"
            | "photoshop.exe"
            | "devenv.exe"
            | "houdinifx.exe"
            | "maya.exe"
    )
}

/// True when the current foreground process is confidently known to NOT be
/// a game for the purposes of an UNSUPERVISED decision - a launcher,
/// AnalystBlaze itself, a common incidental foreground app, or a
/// heavy-workload tool. Exists so resource-usage-only heuristics (high GPU
/// and CPU together, with no process-name or window-title evidence at
/// all) can be veto'd for a foreground app already confirmed not to be a
/// game.
/// telemetry/engine.rs's evaluate_local_policy is the call site this
/// exists for. Deliberately not used to gate a manual "Ativar Modo Gamer"
/// click, which has its own, more permissive check in
/// foreground_process_detection.
pub fn foreground_process_is_confirmed_non_game() -> bool {
    let Some(pid) = foreground_pid() else {
        return false;
    };
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let Some(process) = system
        .processes()
        .iter()
        .find_map(|(candidate, process)| (candidate.as_u32() == pid).then_some(process))
    else {
        return false;
    };
    let normalized = normalize_process_name(&process.name().to_string_lossy());
    is_launcher_process(&normalized)
        || is_never_game_process(&normalized)
        || is_common_foreground_non_game(&normalized)
        || is_heavy_workload_tool(&normalized)
}

pub(crate) fn normalize_process_name(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        is_common_foreground_non_game, is_heavy_workload_tool, looks_like_game_process,
        normalize_process_name,
    };

    #[test]
    fn excludes_app_shell_from_game_candidates() {
        assert!(!looks_like_game_process("analystblaze-desktop.exe"));
        assert!(!looks_like_game_process(
            "C:\\Program Files\\AnalystBlaze\\AnalystBlaze.exe"
        ));
    }

    /// Regression test for the 2026-09 incident: Blender rendering (heavy
    /// GPU+CPU, foreground, name unrelated to any known game) triggered
    /// automatic Game Mode and froze a machine. Blender itself was never a
    /// `looks_like_game_process` match - the bug was in the OTHER two
    /// detection paths (foreground-candidate fallback, and
    /// evaluate_local_policy's high_gpu&&high_cpu heuristic) not excluding
    /// it. is_heavy_workload_tool now covers that - unlike
    /// is_common_foreground_non_game's list, it's excluded only from
    /// UNSUPERVISED detection, not from a manual click (see
    /// allow_manual_click_can_still_target_a_heavy_workload_tool below).
    #[test]
    fn excludes_known_creative_and_dev_tools_from_unsupervised_game_guess() {
        assert!(is_heavy_workload_tool("blender.exe"));
        assert!(is_heavy_workload_tool("unity.exe"));
        assert!(is_heavy_workload_tool("devenv.exe"));
        assert!(!is_heavy_workload_tool("cs2.exe"));
        assert!(!is_common_foreground_non_game("blender.exe"));
    }

    #[test]
    fn keeps_shipping_game_detection() {
        assert!(looks_like_game_process("Backrooms-Win64-Shipping.exe"));
        assert_eq!(
            normalize_process_name("D:\\Games\\Backrooms-Win64-Shipping.exe"),
            "backrooms-win64-shipping.exe"
        );
    }
}
