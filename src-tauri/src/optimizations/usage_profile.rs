//! Perfil de uso medido (etapa 1 do "perfil do PC").
//!
//! A cada minuto anota o que está em primeiro plano (só o nome do .exe) e se
//! o PC está ocioso. Guarda 30 dias em `usage-profile.json`, agregado por dia:
//! segundos por app, por categoria e por hora. Nada sai do PC nesta etapa.
//!
//! `classify` é puro: nomeia o perfil ("games", "work", "creation", "light",
//! "mixed") a partir das parcelas medidas e diz o porquê. Com menos de
//! `MIN_DAYS` dias observados, o perfil é "collecting". O perfil nunca muda
//! nada sozinho; só descreve.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::sync::Mutex;

use super::{adaptive, detection, snapshot};

pub const SAMPLE_SECS: u64 = 60;
pub const KEEP_DAYS: usize = 30;
pub const WINDOW_DAYS: usize = 7;
pub const MIN_DAYS: usize = 2;
/// Sem entrada por 5 min conta como ocioso (mesmo limiar da telemetria).
pub const IDLE_AFTER_SECS: u64 = 300;

static FILE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AppUse {
    pub category: String,
    pub seconds: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DayUse {
    pub apps: BTreeMap<String, AppUse>,
    /// Segundos ativos por hora do dia (24 posições).
    pub hours: Vec<u64>,
    pub idle_seconds: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UsageStore {
    /// "AAAA-MM-DD" (hora local) → uso do dia.
    pub days: BTreeMap<String, DayUse>,
}

fn store_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join("usage-profile.json")
}

fn read_store() -> UsageStore {
    fs::read_to_string(store_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_store(store: &UsageStore) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let text = serde_json::to_string(store).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}

/// Categoria de um .exe em primeiro plano. Listas curtas e conhecidas; o
/// resto é "other". Jogos usam o mesmo detector do Modo Jogo.
pub fn categorize(exe: &str) -> &'static str {
    let name = detection::normalize_process_name(exe);
    let stem = name.trim_end_matches(".exe");
    if matches!(stem, "chrome" | "msedge" | "firefox" | "brave" | "opera" | "operagx" | "vivaldi") {
        return "browser";
    }
    if matches!(
        stem,
        "code" | "cursor" | "devenv" | "idea64" | "pycharm64" | "rider64" | "webstorm64" | "excel" | "winword"
            | "powerpnt" | "outlook" | "onenote" | "notion" | "figma" | "obsidian" | "windowsterminal"
            | "powershell" | "cmd" | "acrobat" | "acrord32"
    ) {
        return "work";
    }
    if matches!(
        stem,
        "premiere" | "adobe premiere pro" | "afterfx" | "photoshop" | "illustrator" | "lightroom" | "blender"
            | "resolve" | "obs64" | "obs32" | "audacity" | "unity" | "unrealeditor" | "davinci resolve"
            | "capcut" | "vegas200" | "vegas210"
    ) {
        return "creation";
    }
    if matches!(stem, "discord" | "whatsapp" | "whatsapp.root" | "telegram" | "zoom" | "teams" | "ms-teams" | "slack" | "skype" | "signal") {
        return "comm";
    }
    if matches!(stem, "spotify" | "vlc" | "mpc-hc64" | "mpc-hc" | "wmplayer" | "musicbee" | "itunes" | "applemusic") {
        return "media";
    }
    if matches!(stem, "explorer" | "searchhost" | "startmenuexperiencehost" | "lockapp" | "shellexperiencehost" | "applicationframehost") {
        return "system";
    }
    if detection::is_launcher_process(&name) {
        return "launcher";
    }
    if detection::looks_like_game_process(&name) {
        return "game";
    }
    "other"
}

/// Soma `seconds` para `exe` (ou ocioso) no dia/hora dados. Puro.
pub fn add_sample(store: &mut UsageStore, day: &str, hour: usize, exe: Option<&str>, seconds: u64) {
    let entry = store.days.entry(day.to_string()).or_default();
    if entry.hours.len() != 24 {
        entry.hours = vec![0; 24];
    }
    match exe {
        None => entry.idle_seconds += seconds,
        Some(exe) => {
            let name = detection::normalize_process_name(exe);
            if name.is_empty() {
                return;
            }
            let category = categorize(&name).to_string();
            let app = entry.apps.entry(name).or_default();
            app.category = category;
            app.seconds += seconds;
            entry.hours[hour.min(23)] += seconds;
        }
    }
    while store.days.len() > KEEP_DAYS {
        let oldest = store.days.keys().next().cloned();
        if let Some(key) = oldest {
            store.days.remove(&key);
        }
    }
}

#[cfg(target_os = "windows")]
fn foreground_exe() -> Option<String> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let pid = detection::foreground_pid()?;
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    let process = system.process(Pid::from_u32(pid))?;
    let name = process.name().to_string_lossy().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(not(target_os = "windows"))]
fn foreground_exe() -> Option<String> {
    None
}

/// Uma amostra: lê o primeiro plano agora e grava. Chamada pelo loop.
pub fn sample_once() {
    let now = chrono::Local::now();
    let day = now.format("%Y-%m-%d").to_string();
    let hour = usize::try_from(chrono::Timelike::hour(&now)).unwrap_or(0);
    let exe = if adaptive::idle_seconds() >= IDLE_AFTER_SECS { None } else { foreground_exe() };
    let Ok(_guard) = FILE_LOCK.lock() else { return };
    let mut store = read_store();
    add_sample(&mut store, &day, hour, exe.as_deref(), SAMPLE_SECS);
    let _ = write_store(&store);
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopApp {
    pub name: String,
    pub category: String,
    pub seconds: u64,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageProfile {
    /// "collecting" | "games" | "work" | "creation" | "light" | "mixed"
    pub profile: String,
    /// 0..1: dias observados sobre a janela.
    pub confidence: f64,
    pub days_observed: usize,
    pub window_days: usize,
    pub active_seconds: u64,
    pub idle_seconds: u64,
    /// Parcela de cada categoria sobre o tempo ativo (0..1).
    pub shares: BTreeMap<String, f64>,
    pub top_apps: Vec<TopApp>,
    /// Horas (0-23) com mais uso ativo, em ordem.
    pub peak_hours: Vec<u8>,
    /// Horas em que o PC costuma estar ligado e ocioso (boas para manutenção).
    pub idle_hours: Vec<u8>,
    /// Chaves do porquê, para a tela traduzir.
    pub reasons: Vec<String>,
}

/// Nomeia o perfil a partir dos últimos `window_days` dias. Puro.
pub fn classify(store: &UsageStore, window_days: usize) -> UsageProfile {
    let days: Vec<&DayUse> = store.days.values().rev().take(window_days).collect();
    let days_observed = days.iter().filter(|day| day.apps.values().map(|app| app.seconds).sum::<u64>() > 0).count();

    let mut by_category: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_app: BTreeMap<String, (String, u64)> = BTreeMap::new();
    let mut hours = vec![0u64; 24];
    let mut idle_seconds = 0u64;
    for day in &days {
        idle_seconds += day.idle_seconds;
        for (index, value) in day.hours.iter().enumerate().take(24) {
            hours[index] += value;
        }
        for (name, app) in &day.apps {
            *by_category.entry(app.category.clone()).or_default() += app.seconds;
            let entry = by_app.entry(name.clone()).or_insert_with(|| (app.category.clone(), 0));
            entry.1 += app.seconds;
        }
    }
    let active_seconds: u64 = by_category.values().sum();
    let share = |key: &str| -> f64 {
        if active_seconds == 0 {
            0.0
        } else {
            *by_category.get(key).unwrap_or(&0) as f64 / active_seconds as f64
        }
    };
    let shares: BTreeMap<String, f64> = by_category
        .keys()
        .map(|key| (key.clone(), (share(key) * 1000.0).round() / 1000.0))
        .collect();

    let mut top: Vec<TopApp> = by_app
        .into_iter()
        .filter(|(_, (category, _))| category != "system")
        .map(|(name, (category, seconds))| TopApp {
            share: if active_seconds == 0 { 0.0 } else { (seconds as f64 / active_seconds as f64 * 1000.0).round() / 1000.0 },
            name,
            category,
            seconds,
        })
        .collect();
    top.sort_by(|a, b| b.seconds.cmp(&a.seconds));
    top.truncate(5);

    let mut hour_order: Vec<(u8, u64)> = hours.iter().enumerate().map(|(h, s)| (h as u8, *s)).collect();
    hour_order.sort_by(|a, b| b.1.cmp(&a.1));
    let peak_hours: Vec<u8> = hour_order.iter().filter(|(_, s)| *s > 0).take(3).map(|(h, _)| *h).collect();
    // Horas em que o PC ficou ligado mas quase nada ativo: candidatas a manutenção.
    let on_days = days.len().max(1) as u64;
    let idle_hours: Vec<u8> = (0..24u8)
        .filter(|h| hours[*h as usize] < on_days * 300 && idle_seconds > 0)
        .take(6)
        .collect();

    let avg_active_per_day = if days_observed == 0 { 0 } else { active_seconds / days_observed as u64 };
    let mut reasons = Vec::new();
    let profile = if days_observed < MIN_DAYS {
        reasons.push("collecting".to_string());
        "collecting"
    } else if share("game") >= 0.40 {
        reasons.push("games_share".to_string());
        "games"
    } else if share("creation") >= 0.30 {
        reasons.push("creation_share".to_string());
        "creation"
    } else if avg_active_per_day < 3600 {
        reasons.push("light_use".to_string());
        "light"
    } else if share("browser") + share("work") >= 0.50 {
        reasons.push("work_share".to_string());
        "work"
    } else {
        reasons.push("mixed".to_string());
        "mixed"
    };
    if !peak_hours.is_empty() {
        reasons.push("peak_hours".to_string());
    }

    UsageProfile {
        profile: profile.to_string(),
        confidence: ((days_observed as f64) / (window_days as f64)).clamp(0.0, 1.0),
        days_observed,
        window_days,
        active_seconds,
        idle_seconds,
        shares,
        top_apps: top,
        peak_hours,
        idle_hours,
        reasons,
    }
}

/// O perfil atual, lido do arquivo.
pub fn current() -> UsageProfile {
    let _guard = FILE_LOCK.lock();
    classify(&read_store(), WINDOW_DAYS)
}

pub fn current_json() -> Value {
    json!(current())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(days: &[(&str, &[(&str, u64)])]) -> UsageStore {
        let mut store = UsageStore::default();
        for (day, apps) in days {
            for (exe, secs) in apps.iter() {
                add_sample(&mut store, day, 21, Some(exe), *secs);
            }
        }
        store
    }

    #[test]
    fn categorizes_known_apps() {
        assert_eq!(categorize("chrome.exe"), "browser");
        assert_eq!(categorize("Code.exe"), "work");
        assert_eq!(categorize("blender.exe"), "creation");
        assert_eq!(categorize("steam.exe"), "launcher");
        assert_eq!(categorize("explorer.exe"), "system");
    }

    #[test]
    fn fewer_than_two_days_is_collecting() {
        let store = store_with(&[("2026-11-01", &[("chrome.exe", 7200)])]);
        assert_eq!(classify(&store, 7).profile, "collecting");
    }

    #[test]
    fn mostly_browser_and_work_is_work() {
        let store = store_with(&[
            ("2026-11-01", &[("chrome.exe", 5000), ("code.exe", 4000)]),
            ("2026-11-02", &[("chrome.exe", 6000), ("spotify.exe", 1000)]),
        ]);
        let profile = classify(&store, 7);
        assert_eq!(profile.profile, "work");
        assert_eq!(profile.days_observed, 2);
        assert_eq!(profile.top_apps[0].name, "chrome.exe");
        assert!(profile.reasons.contains(&"work_share".to_string()));
    }

    #[test]
    fn under_one_hour_a_day_is_light() {
        let store = store_with(&[("2026-11-01", &[("chrome.exe", 600)]), ("2026-11-02", &[("chrome.exe", 900)])]);
        assert_eq!(classify(&store, 7).profile, "light");
    }

    #[test]
    fn keeps_only_thirty_days() {
        let mut store = UsageStore::default();
        for day in 1..=40 {
            add_sample(&mut store, &format!("2026-10-{day:02}"), 10, Some("chrome.exe"), 60);
        }
        assert_eq!(store.days.len(), KEEP_DAYS);
        assert!(store.days.contains_key("2026-10-40"));
    }

    #[test]
    fn idle_goes_to_idle_seconds_not_apps() {
        let mut store = UsageStore::default();
        add_sample(&mut store, "2026-11-01", 4, None, 60);
        let day = &store.days["2026-11-01"];
        assert_eq!(day.idle_seconds, 60);
        assert!(day.apps.is_empty());
    }
}
