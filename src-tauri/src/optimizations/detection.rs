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
    let detection = detect_game_process_with_payload(payload.as_ref());
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

pub fn detect_game_process() -> GameDetection {
    detect_game_process_with_payload(None)
}

pub fn detect_game_process_with_payload(payload: Option<&Value>) -> GameDetection {
    if let Some(detection) = explicit_target_detection(payload) {
        return detection;
    }

    if let Some(detection) = foreground_process_detection() {
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

fn foreground_process_detection() -> Option<GameDetection> {
    let foreground_pid = foreground_pid()?;
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let process = system
        .processes()
        .iter()
        .find_map(|(pid, process)| (pid.as_u32() == foreground_pid).then_some(process))?;
    let name = process.name().to_string_lossy().trim().to_string();
    let normalized = normalize_process_name(&name);

    if is_launcher_process(&normalized)
        || is_never_game_process(&normalized)
        || is_common_foreground_non_game(&normalized)
    {
        return None;
    }

    let known = looks_like_game_process(&normalized);
    Some(GameDetection {
        detected: true,
        process_name: Some(name),
        pid: Some(foreground_pid.to_string()),
        confidence: if known { 0.9 } else { 0.66 },
        reason: if known {
            "foreground_known_game_process".to_string()
        } else {
            "foreground_process_candidate".to_string()
        },
    })
}

#[cfg(windows)]
fn foreground_pid() -> Option<u32> {
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
fn foreground_pid() -> Option<u32> {
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

fn normalize_process_name(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{looks_like_game_process, normalize_process_name};

    #[test]
    fn excludes_app_shell_from_game_candidates() {
        assert!(!looks_like_game_process("analystblaze-desktop.exe"));
        assert!(!looks_like_game_process(
            "C:\\Program Files\\AnalystBlaze\\AnalystBlaze.exe"
        ));
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
