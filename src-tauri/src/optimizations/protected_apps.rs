use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use super::snapshot;
use crate::audit;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedApp {
    pub name: String,
    pub reason: Option<String>,
    pub created_at: i64,
}

pub fn list_protected_apps() -> Vec<ProtectedApp> {
    let mut apps = load_user_apps();
    for default_name in default_protected_apps() {
        if !apps
            .iter()
            .any(|app| same_app_name(&app.name, default_name))
        {
            apps.push(ProtectedApp {
                name: default_name.to_string(),
                reason: Some("default".to_string()),
                created_at: 0,
            });
        }
    }
    apps.sort_by(|left, right| {
        normalize_app_name(&left.name).cmp(&normalize_app_name(&right.name))
    });
    apps
}

pub fn add_protected_app(
    name: String,
    reason: Option<String>,
) -> Result<Vec<ProtectedApp>, String> {
    let normalized = normalize_app_name(&name);
    if normalized.is_empty() || normalized.len() > 120 || normalized.contains(['\\', '/', '\0']) {
        return Err("Nome de app protegido invalido.".to_string());
    }

    let mut apps = load_user_apps();
    if !apps.iter().any(|app| same_app_name(&app.name, &normalized)) {
        apps.push(ProtectedApp {
            name: normalized.clone(),
            reason,
            created_at: chrono::Utc::now().timestamp(),
        });
        save_user_apps(&apps)?;
        let _ = audit::record_event(
            "info",
            "protected_apps.added",
            "App protegido adicionado pelo usuario.",
            json!({ "name": normalized }),
        );
    }

    Ok(list_protected_apps())
}

pub fn remove_protected_app(name: String) -> Result<Vec<ProtectedApp>, String> {
    let normalized = normalize_app_name(&name);
    let mut apps = load_user_apps();
    let before = apps.len();
    apps.retain(|app| !same_app_name(&app.name, &normalized));
    if apps.len() != before {
        save_user_apps(&apps)?;
        let _ = audit::record_event(
            "info",
            "protected_apps.removed",
            "App protegido removido pelo usuario.",
            json!({ "name": normalized }),
        );
    }
    Ok(list_protected_apps())
}

pub fn is_protected_app(name: &str) -> bool {
    let normalized = normalize_app_name(name);
    if normalized.is_empty() {
        return false;
    }

    default_protected_apps()
        .iter()
        .any(|app| same_app_name(app, &normalized))
        || load_user_apps()
            .iter()
            .any(|app| same_app_name(&app.name, &normalized))
}

/// `is_protected_app` (via this) is now called on every single file/folder
/// visited by the disk-tree walk and the protected-descendant scan, not
/// just once per top-level item - re-reading and re-parsing this JSON file
/// from disk on every one of those calls is what made browsing/deleting
/// large folders burn CPU. Cached keyed on the file's own mtime (a single
/// cheap stat) instead: unchanged between calls (the overwhelming common
/// case mid-scan) skips the read+parse entirely, while an edit made
/// through the Protected Apps UI is still picked up on the next call.
type UserAppsCache = Mutex<Option<(SystemTime, Vec<ProtectedApp>)>>;

fn load_user_apps() -> Vec<ProtectedApp> {
    static CACHE: OnceLock<UserAppsCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));

    let path = protected_apps_path();
    let mtime = fs::metadata(&path).and_then(|meta| meta.modified()).ok();

    let mut guard = cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let (Some(mtime), Some((cached_mtime, cached_apps))) = (mtime, guard.as_ref()) {
        if *cached_mtime == mtime {
            return cached_apps.clone();
        }
    }

    let Ok(raw) = fs::read_to_string(&path) else {
        *guard = None;
        return Vec::new();
    };
    let apps: Vec<ProtectedApp> = serde_json::from_str::<Vec<ProtectedApp>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .filter(|app| !normalize_app_name(&app.name).is_empty())
        .collect();

    if let Some(mtime) = mtime {
        *guard = Some((mtime, apps.clone()));
    }
    apps
}

fn save_user_apps(apps: &[ProtectedApp]) -> Result<(), String> {
    let path = protected_apps_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let raw = serde_json::to_string_pretty(apps).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn protected_apps_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join("protected-apps.json")
}

fn normalize_app_name(name: &str) -> String {
    let mut value = name
        .trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase();
    if !value.ends_with(".exe") && !value.contains('.') {
        value.push_str(".exe");
    }
    value
}

fn same_app_name(left: &str, right: &str) -> bool {
    normalize_app_name(left) == normalize_app_name(right)
}

fn default_protected_apps() -> &'static [&'static str] {
    &[
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "discord.exe",
        "steam.exe",
        "code.exe",
        "devenv.exe",
        "photoshop.exe",
        "teams.exe",
        "zoom.exe",
        "obs64.exe",
        "valorant.exe",
        "cs2.exe",
        "fortniteclient-win64-shipping.exe",
        "leagueclient.exe",
        "minecraft.exe",
    ]
}
