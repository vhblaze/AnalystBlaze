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
    let detection = detect_game_process();
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

fn looks_like_game_process(name: &str) -> bool {
    let normalized = normalize_process_name(name);
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
        "steam.exe",
        "epicgameslauncher.exe",
        "riotclientservices.exe",
        "battle.net.exe",
    ];

    KNOWN_GAMES.iter().any(|candidate| *candidate == normalized)
        || normalized.contains("shipping")
        || normalized.contains("unityplayer")
}

fn normalize_process_name(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}
