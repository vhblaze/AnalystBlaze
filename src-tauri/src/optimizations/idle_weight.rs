//! "Pesa sem você usar" (etapa 2 do perfil do PC).
//!
//! Cruza o que está rodando em segundo plano com o perfil de uso medido
//! (`usage_profile`): um app que abre com o Windows, consome memória ou CPU
//! agora, e não aparece em primeiro plano há `UNUSED_DAYS` dias é custo sem
//! uso. Só lista apps de inicialização (serviços não: o sinal de "sem uso"
//! deles não é confiável). Quem age é o usuário, ou a regra do perfil.

use serde::Serialize;
use std::collections::BTreeMap;

use super::app_usage::exe_name_from_command as app_usage_exe;
use super::{active_use, detection, usage_profile, windows_inventory};

/// Abaixo disto o item não vale a pena: o custo é menor que o risco.
pub const MIN_MEMORY_MB: f64 = 150.0;
pub const MIN_CPU_PERCENT: f64 = 1.0;
/// Dias sem aparecer em primeiro plano para contar como "sem uso".
pub const UNUSED_DAYS: i64 = 7;
/// Sem pelo menos isto de observação, "nunca visto" não significa nada.
pub const MIN_TRACKED_DAYS: usize = 2;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IdleWeightItem {
    /// "startup" (app que abre com o Windows) ou "service".
    pub kind: String,
    /// Nome do .exe ou do serviço (o que a ação recebe).
    pub name: String,
    pub display_name: String,
    /// Só para startup: onde a entrada está (o que DELAY_STARTUP_APP precisa).
    pub location: Option<String>,
    pub memory_mb: f64,
    pub cpu_percent: f64,
    /// None = nunca em primeiro plano no período observado.
    pub last_foreground_days_ago: Option<i64>,
    pub days_tracked: usize,
    /// "delay_startup" | "stop_service". Ambas com snapshot e Desfazer.
    pub action: String,
}

/// Um processo em segundo plano agregado por .exe.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundProcess {
    pub exe: String,
    pub memory_mb: f64,
    pub cpu_percent: f64,
    pub in_active_use: bool,
}

fn is_system_process(normalized: &str) -> bool {
    matches!(
        normalized,
        "system" | "registry" | "smss.exe" | "csrss.exe" | "wininit.exe" | "winlogon.exe" | "services.exe"
            | "lsass.exe" | "svchost.exe" | "dwm.exe" | "explorer.exe" | "fontdrvhost.exe" | "sihost.exe"
            | "taskhostw.exe" | "runtimebroker.exe" | "searchhost.exe" | "startmenuexperiencehost.exe"
            | "shellexperiencehost.exe" | "textinputhost.exe" | "ctfmon.exe" | "conhost.exe" | "audiodg.exe"
            | "memcompression" | "msmpeng.exe" | "nissrv.exe" | "securityhealthservice.exe" | "wudfhost.exe"
            | "spoolsv.exe" | "dllhost.exe" | "applicationframehost.exe" | "systemsettings.exe"
    ) || normalized.starts_with("analystblaze") || super::anticheat::is_anticheat_process(normalized)
}

/// Decide quais apps de inicialização pesam sem uso. Puro.
pub fn select_startup(
    processes: &[BackgroundProcess],
    startup_exes: &BTreeMap<String, (String, String)>,
    store: &usage_profile::UsageStore,
    today: &str,
) -> Vec<IdleWeightItem> {
    let days_tracked = usage_profile::days_tracked_in(store);
    if days_tracked < MIN_TRACKED_DAYS {
        return Vec::new();
    }
    let mut items: Vec<IdleWeightItem> = processes
        .iter()
        .filter(|process| !process.in_active_use)
        .filter(|process| process.memory_mb >= MIN_MEMORY_MB || process.cpu_percent >= MIN_CPU_PERCENT)
        .filter_map(|process| {
            let normalized = detection::normalize_process_name(&process.exe);
            if is_system_process(&normalized) {
                return None;
            }
            let (display_name, location) = startup_exes.get(&normalized)?;
            let last = usage_profile::last_foreground_days_ago_in(store, &normalized, today);
            if last.is_some_and(|days| days < UNUSED_DAYS) {
                return None;
            }
            Some(IdleWeightItem {
                kind: "startup".to_string(),
                name: display_name.clone(),
                display_name: display_name.clone(),
                location: Some(location.clone()),
                memory_mb: (process.memory_mb * 10.0).round() / 10.0,
                cpu_percent: (process.cpu_percent * 10.0).round() / 10.0,
                last_foreground_days_ago: last,
                days_tracked,
                action: "delay_startup".to_string(),
            })
        })
        .collect();
    items.sort_by(|a, b| b.memory_mb.partial_cmp(&a.memory_mb).unwrap_or(std::cmp::Ordering::Equal));
    items
}

#[cfg(target_os = "windows")]
fn background_processes() -> Vec<BackgroundProcess> {
    use sysinfo::{ProcessesToUpdate, System};
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    std::thread::sleep(std::time::Duration::from_millis(800));
    system.refresh_processes(ProcessesToUpdate::All, true);
    let cores = system.cpus().len().max(1) as f64;
    let foreground = active_use::foreground_process_id();
    let audio = active_use::processes_with_active_audio_session();
    let mut by_exe: BTreeMap<String, BackgroundProcess> = BTreeMap::new();
    for (pid, process) in system.processes() {
        let exe = process.name().to_string_lossy().to_string();
        if exe.is_empty() {
            continue;
        }
        let entry = by_exe.entry(detection::normalize_process_name(&exe)).or_insert_with(|| BackgroundProcess {
            exe: exe.clone(),
            memory_mb: 0.0,
            cpu_percent: 0.0,
            in_active_use: false,
        });
        entry.memory_mb += process.memory() as f64 / (1024.0 * 1024.0);
        entry.cpu_percent += f64::from(process.cpu_usage()) / cores;
        entry.in_active_use |= active_use::is_in_active_use(pid.as_u32(), foreground, &audio);
    }
    by_exe.into_values().collect()
}

#[cfg(not(target_os = "windows"))]
fn background_processes() -> Vec<BackgroundProcess> {
    Vec::new()
}

fn startup_exes() -> BTreeMap<String, (String, String)> {
    windows_inventory::startup_apps_only()
        .into_iter()
        .filter_map(|app| {
            let exe = app_usage_exe(&app.command).or_else(|| app_usage_exe(&app.name))?;
            Some((detection::normalize_process_name(&exe), (app.name, app.location)))
        })
        .collect()
}

/// Lista atual. Bloqueante (~1 s por causa da amostra de CPU).
pub fn candidates() -> Vec<IdleWeightItem> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let store = usage_profile::snapshot_store();
    // Só apps de inicialização. Serviços ficam de fora de propósito: os sinais de
    // "sem uso" (ex.: data da última conexão Bluetooth) não provam que o usuário
    // não depende do serviço, e pausar errado derruba teclado, mouse e fone.
    select_startup(&background_processes(), &startup_exes(), &store, &today)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(days: &[(&str, &[(&str, u64)])]) -> usage_profile::UsageStore {
        let mut store = usage_profile::UsageStore::default();
        for (day, apps) in days {
            for (exe, secs) in apps.iter() {
                usage_profile::add_sample(&mut store, day, 20, Some(exe), *secs);
            }
            if apps.is_empty() {
                usage_profile::add_sample(&mut store, day, 20, None, 60);
            }
        }
        store
    }

    fn proc(exe: &str, mb: f64, cpu: f64) -> BackgroundProcess {
        BackgroundProcess { exe: exe.to_string(), memory_mb: mb, cpu_percent: cpu, in_active_use: false }
    }

    fn startup(entries: &[(&str, &str)]) -> BTreeMap<String, (String, String)> {
        entries.iter().map(|(exe, name)| (exe.to_string(), (name.to_string(), "HKCU\\Run".to_string()))).collect()
    }

    #[test]
    fn startup_app_heavy_and_never_in_foreground_is_listed() {
        let s = store(&[("2026-11-01", &[("chrome.exe", 600)]), ("2026-11-04", &[("chrome.exe", 600)])]);
        let items = select_startup(&[proc("EpicGamesLauncher.exe", 310.0, 0.2)], &startup(&[("epicgameslauncher.exe", "Epic Games Launcher")]), &s, "2026-11-05");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action, "delay_startup");
        assert_eq!(items[0].last_foreground_days_ago, None);
    }

    #[test]
    fn recently_used_app_is_not_listed() {
        let s = store(&[("2026-11-01", &[("chrome.exe", 600)]), ("2026-11-04", &[("discord.exe", 600)])]);
        let items = select_startup(&[proc("Discord.exe", 400.0, 0.5)], &startup(&[("discord.exe", "Discord")]), &s, "2026-11-05");
        assert!(items.is_empty());
    }

    #[test]
    fn light_process_is_not_listed() {
        let s = store(&[("2026-11-01", &[("chrome.exe", 600)]), ("2026-11-04", &[("chrome.exe", 600)])]);
        let items = select_startup(&[proc("Updater.exe", 20.0, 0.1)], &startup(&[("updater.exe", "Updater")]), &s, "2026-11-05");
        assert!(items.is_empty());
    }

    #[test]
    fn anticheat_processes_are_never_listed() {
        let s = store(&[("2026-11-01", &[("chrome.exe", 600)]), ("2026-11-04", &[("chrome.exe", 600)])]);
        let items = select_startup(&[proc("vgtray.exe", 400.0, 2.0)], &startup(&[("vgtray.exe", "Riot Vanguard")]), &s, "2026-11-07");
        assert!(items.is_empty());
    }

    #[test]
    fn needs_two_days_of_observation() {
        let s = store(&[("2026-11-04", &[("chrome.exe", 600)])]);
        let items = select_startup(&[proc("EpicGamesLauncher.exe", 310.0, 0.2)], &startup(&[("epicgameslauncher.exe", "Epic")]), &s, "2026-11-05");
        assert!(items.is_empty());
    }

    #[test]
    fn system_and_non_startup_processes_are_ignored() {
        let s = store(&[("2026-11-01", &[]), ("2026-11-04", &[])]);
        let items = select_startup(
            &[proc("svchost.exe", 900.0, 3.0), proc("someapp.exe", 500.0, 2.0)],
            &startup(&[("svchost.exe", "svc")]),
            &s,
            "2026-11-05",
        );
        assert!(items.is_empty());
    }
}
