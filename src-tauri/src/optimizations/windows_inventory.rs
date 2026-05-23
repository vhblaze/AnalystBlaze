use serde::Serialize;
use serde_json::json;

use super::safety;
use crate::audit;

#[derive(Debug, Clone, Serialize)]
pub struct WindowsInventory {
    pub startup_apps: Vec<StartupApp>,
    pub services: Vec<WindowsService>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartupApp {
    pub name: String,
    pub command: String,
    pub location: String,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WindowsService {
    pub name: String,
    pub display_name: Option<String>,
    pub start_type: Option<u32>,
    pub classification: String,
    pub can_modify: bool,
}

pub fn collect_windows_inventory() -> WindowsInventory {
    let inventory = collect_windows_inventory_inner();
    let _ = audit::record_event(
        "info",
        "windows.inventory_collected",
        "Inventario local de inicializacao e servicos coletado em modo somente leitura.",
        json!({
            "startup_apps": inventory.startup_apps.len(),
            "services": inventory.services.len(),
        }),
    );
    inventory
}

#[cfg(windows)]
fn collect_windows_inventory_inner() -> WindowsInventory {
    WindowsInventory {
        startup_apps: startup_apps(),
        services: services(),
    }
}

#[cfg(not(windows))]
fn collect_windows_inventory_inner() -> WindowsInventory {
    WindowsInventory {
        startup_apps: Vec::new(),
        services: Vec::new(),
    }
}

#[cfg(windows)]
fn startup_apps() -> Vec<StartupApp> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let roots = [
        (
            RegKey::predef(HKEY_CURRENT_USER),
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        ),
        (
            RegKey::predef(HKEY_LOCAL_MACHINE),
            "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        ),
        (
            RegKey::predef(HKEY_CURRENT_USER),
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
        ),
        (
            RegKey::predef(HKEY_LOCAL_MACHINE),
            "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
        ),
    ];

    let mut apps = Vec::new();
    for (root, path) in roots {
        let Some((hive, subkey)) = path.split_once('\\') else {
            continue;
        };
        let Ok(key) = root.open_subkey(subkey) else {
            continue;
        };

        for item in key.enum_values().flatten() {
            let name = item.0;
            let command = registry_value_to_string(item.1);
            apps.push(StartupApp {
                risk: startup_risk(&name, &command).to_string(),
                name,
                command,
                location: hive.to_string() + "\\" + subkey,
            });
        }
    }

    apps
}

#[cfg(windows)]
fn services() -> Vec<WindowsService> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(root) = hklm.open_subkey("SYSTEM\\CurrentControlSet\\Services") else {
        return Vec::new();
    };

    root.enum_keys()
        .flatten()
        .filter_map(|name| {
            let key = root.open_subkey(&name).ok()?;
            let display_name: Option<String> = key.get_value("DisplayName").ok();
            let start_type: Option<u32> = key.get_value("Start").ok();
            let classification = classify_service(&name, display_name.as_deref());
            Some(WindowsService {
                name,
                display_name,
                start_type,
                can_modify: classification == "safe",
                classification,
            })
        })
        .collect()
}

#[cfg(windows)]
fn registry_value_to_string(value: winreg::RegValue) -> String {
    value.to_string().trim().to_string()
}

fn classify_service(name: &str, display_name: Option<&str>) -> String {
    if safety::is_critical_service(name) || display_name.is_some_and(safety::is_critical_service) {
        return "critical".to_string();
    }

    let normalized = format!(
        "{} {}",
        name.to_ascii_lowercase(),
        display_name.unwrap_or_default().to_ascii_lowercase()
    );

    if normalized.contains("security")
        || normalized.contains("update")
        || normalized.contains("driver")
        || normalized.contains("network")
        || normalized.contains("audio")
        || normalized.contains("vpn")
    {
        "sensitive".to_string()
    } else {
        "safe".to_string()
    }
}

fn startup_risk(name: &str, command: &str) -> &'static str {
    let normalized = format!(
        "{} {}",
        name.to_ascii_lowercase(),
        command.to_ascii_lowercase()
    );
    if normalized.contains("defender")
        || normalized.contains("security")
        || normalized.contains("antivirus")
        || normalized.contains("driver")
        || normalized.contains("vpn")
    {
        "sensitive"
    } else {
        "safe"
    }
}
