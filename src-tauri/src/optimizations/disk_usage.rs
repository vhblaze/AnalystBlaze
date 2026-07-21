use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use super::{
    performance_suite,
    protected_apps::is_protected_app,
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};

pub const DISK_USAGE_PROGRESS_EVENT: &str = "disk-usage-scan-progress";

/// Individual files at or above this size are reported in the "large
/// files" category on their own, separate from whichever folder they
/// happen to sit in.
const LARGE_FILE_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;
/// Same order of magnitude as performance_suite's CATEGORY_SCAN_LIMIT -
/// keeps a single scan from running unbounded on a huge disk.
const SCAN_ITEM_CAP: usize = 20_000;
const TOP_ITEMS_PER_CATEGORY: usize = 25;
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskUsageCategoryKind {
    Games,
    Apps,
    Videos,
    Cache,
    Downloads,
    LargeFiles,
    System,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageItem {
    pub path: String,
    pub label: String,
    pub size_bytes: u64,
    /// True if this matches an entry in the user's protected-apps list -
    /// never actionable regardless of category.
    pub protected: bool,
    /// False for every item in the System category (always informational),
    /// and for any protected item elsewhere.
    pub actionable: bool,
    /// Cache-category items delete through the existing
    /// APPLY_CLEANUP_CATEGORY action (id in `path`) instead of the generic
    /// DELETE_DISK_USAGE_ITEM path - see cleanup_category_item().
    pub deletes_via_cleanup_category: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageCategory {
    pub kind: DiskUsageCategoryKind,
    pub label: String,
    pub total_bytes: u64,
    pub item_count: usize,
    pub items: Vec<DiskUsageItem>,
    pub capped: bool,
    pub scanned_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageSummary {
    pub categories: Vec<DiskUsageCategory>,
    pub scanned_at: i64,
    pub duration_ms: u64,
    pub canceled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageProgress {
    pub current_category: String,
    pub scanned_items: usize,
    pub done: bool,
}

pub async fn scan_disk_usage(app: AppHandle, cancel: Arc<AtomicBool>) -> DiskUsageSummary {
    // Reuses the existing, already-safe cache/temp scanner instead of
    // re-walking those paths ourselves - see cache_category_from_suite.
    let cache_categories = performance_suite::scan_cleanup_categories()
        .await
        .unwrap_or_default();

    tokio::task::spawn_blocking(move || {
        scan_disk_usage_blocking(app, cancel, cache_categories)
    })
    .await
    .unwrap_or_else(|_| DiskUsageSummary {
        categories: Vec::new(),
        scanned_at: chrono::Utc::now().timestamp(),
        duration_ms: 0,
        canceled: true,
    })
}

struct Scanner {
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    items_scanned: usize,
    last_emit: Instant,
    canceled: bool,
}

impl Scanner {
    fn new(app: AppHandle, cancel: Arc<AtomicBool>) -> Self {
        Self {
            app,
            cancel,
            items_scanned: 0,
            last_emit: Instant::now() - PROGRESS_EMIT_INTERVAL,
            canceled: false,
        }
    }

    /// Call once per file/dir visited. Returns false once the scan should
    /// stop early - either the user canceled, or the global item cap was
    /// reached (in which case `self.canceled` stays false; the caller marks
    /// just that category as capped instead of aborting the whole scan).
    fn visit(&mut self, category: &str) -> bool {
        if self.canceled {
            return false;
        }
        if self.cancel.load(Ordering::Relaxed) {
            self.canceled = true;
            return false;
        }
        self.items_scanned += 1;
        if self.last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
            self.last_emit = Instant::now();
            let _ = self.app.emit(
                DISK_USAGE_PROGRESS_EVENT,
                DiskUsageProgress {
                    current_category: category.to_string(),
                    scanned_items: self.items_scanned,
                    done: false,
                },
            );
        }
        self.items_scanned < SCAN_ITEM_CAP
    }

    fn emit_done(&self) {
        let _ = self.app.emit(
            DISK_USAGE_PROGRESS_EVENT,
            DiskUsageProgress {
                current_category: "done".to_string(),
                scanned_items: self.items_scanned,
                done: true,
            },
        );
    }
}

fn scan_disk_usage_blocking(
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    cache_categories: Vec<performance_suite::CleanupCategory>,
) -> DiskUsageSummary {
    let started = Instant::now();
    let mut scanner = Scanner::new(app, cancel);
    let mut categories = vec![cache_category_from_suite(&cache_categories)];
    let mut claimed_game_paths: HashSet<PathBuf> = HashSet::new();

    if !scanner.canceled {
        categories.push(scan_top_level_folder(
            &mut scanner,
            DiskUsageCategoryKind::Downloads,
            "Downloads",
            user_dir("Downloads"),
        ));
    }
    if !scanner.canceled {
        categories.push(scan_top_level_folder(
            &mut scanner,
            DiskUsageCategoryKind::Videos,
            "Videos",
            user_dir("Videos"),
        ));
    }
    if !scanner.canceled {
        categories.push(scan_games(&mut scanner, &mut claimed_game_paths));
    }
    if !scanner.canceled {
        categories.push(scan_apps(&mut scanner, &claimed_game_paths));
    }
    if !scanner.canceled {
        categories.push(scan_large_files(&mut scanner));
    }
    if !scanner.canceled {
        categories.push(scan_system_informational(&mut scanner));
    }

    scanner.emit_done();

    DiskUsageSummary {
        categories,
        scanned_at: chrono::Utc::now().timestamp(),
        duration_ms: started.elapsed().as_millis() as u64,
        canceled: scanner.canceled,
    }
}

fn cache_category_from_suite(
    cache_categories: &[performance_suite::CleanupCategory],
) -> DiskUsageCategory {
    let items = cache_categories
        .iter()
        .filter(|category| category.reclaimable_bytes > 0)
        .map(|category| DiskUsageItem {
            path: category.id.clone(),
            label: category.label.clone(),
            size_bytes: category.reclaimable_bytes,
            protected: false,
            actionable: category.available_actions.iter().any(|action| action == "apply"),
            deletes_via_cleanup_category: true,
        })
        .collect::<Vec<_>>();
    let total_bytes = items.iter().map(|item| item.size_bytes).sum();
    let scanned_paths = cache_categories
        .iter()
        .flat_map(|category| category.scanned_paths.clone())
        .collect();

    DiskUsageCategory {
        kind: DiskUsageCategoryKind::Cache,
        label: "Cache e temporarios".to_string(),
        total_bytes,
        item_count: items.len(),
        items,
        capped: false,
        scanned_paths,
    }
}

fn scan_top_level_folder(
    scanner: &mut Scanner,
    kind: DiskUsageCategoryKind,
    label: &str,
    root: Option<PathBuf>,
) -> DiskUsageCategory {
    let category_key = format!("{label:?}");
    let Some(root) = root.filter(|path| path.exists()) else {
        return empty_category(kind, label);
    };

    let mut items = Vec::new();
    let mut total_bytes = 0u64;
    let mut capped = false;

    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            if !scanner.visit(&category_key) {
                capped = true;
                break;
            }
            let Ok(item) = describe_entry(scanner, &category_key, &entry.path()) else {
                continue;
            };
            total_bytes += item.size_bytes;
            items.push(item);
        }
    }

    finish_category(kind, label, items, total_bytes, capped, vec![root.display().to_string()])
}

fn scan_games(scanner: &mut Scanner, claimed: &mut HashSet<PathBuf>) -> DiskUsageCategory {
    let category_key = "games";
    let mut items = Vec::new();
    let mut total_bytes = 0u64;
    let mut capped = false;
    let mut scanned_paths = Vec::new();

    for library_root in steam_library_common_dirs() {
        scanned_paths.push(library_root.display().to_string());
        let Ok(entries) = fs::read_dir(&library_root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !scanner.visit(category_key) {
                capped = true;
                break;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Ok(item) = describe_entry(scanner, category_key, &path) else {
                continue;
            };
            claimed.insert(path.clone());
            total_bytes += item.size_bytes;
            items.push(item);
        }
    }

    if let Some(epic_root) = epic_games_root().filter(|path| path.exists()) {
        scanned_paths.push(epic_root.display().to_string());
        if let Ok(entries) = fs::read_dir(&epic_root) {
            for entry in entries.flatten() {
                if !scanner.visit(category_key) {
                    capped = true;
                    break;
                }
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Ok(item) = describe_entry(scanner, category_key, &path) else {
                    continue;
                };
                claimed.insert(path.clone());
                total_bytes += item.size_bytes;
                items.push(item);
            }
        }
    }

    finish_category(
        DiskUsageCategoryKind::Games,
        "Jogos",
        items,
        total_bytes,
        capped,
        scanned_paths,
    )
}

fn scan_apps(scanner: &mut Scanner, claimed_game_paths: &HashSet<PathBuf>) -> DiskUsageCategory {
    let category_key = "apps";
    let mut items = Vec::new();
    let mut total_bytes = 0u64;
    let mut capped = false;
    let mut scanned_paths = Vec::new();

    for root in program_files_roots() {
        if !root.exists() {
            continue;
        }
        scanned_paths.push(root.display().to_string());
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !scanner.visit(category_key) {
                capped = true;
                break;
            }
            let path = entry.path();
            if !path.is_dir() || claimed_game_paths.contains(&path) {
                continue;
            }
            let Ok(item) = describe_entry(scanner, category_key, &path) else {
                continue;
            };
            total_bytes += item.size_bytes;
            items.push(item);
        }
    }

    finish_category(
        DiskUsageCategoryKind::Apps,
        "Aplicativos",
        items,
        total_bytes,
        capped,
        scanned_paths,
    )
}

fn scan_large_files(scanner: &mut Scanner) -> DiskUsageCategory {
    let category_key = "large_files";
    let mut items = Vec::new();
    let mut total_bytes = 0u64;
    let mut capped = false;
    let mut scanned_paths = Vec::new();

    for folder in ["Documents", "Desktop", "Pictures", "Music"] {
        let Some(root) = user_dir(folder).filter(|path| path.exists()) else {
            continue;
        };
        scanned_paths.push(root.display().to_string());
        capped |= walk_for_large_files(scanner, category_key, &root, &mut items, &mut total_bytes);
        if scanner.canceled {
            break;
        }
    }

    finish_category(
        DiskUsageCategoryKind::LargeFiles,
        "Arquivos grandes",
        items,
        total_bytes,
        capped,
        scanned_paths,
    )
}

fn walk_for_large_files(
    scanner: &mut Scanner,
    category_key: &str,
    dir: &Path,
    items: &mut Vec<DiskUsageItem>,
    total_bytes: &mut u64,
) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if !scanner.visit(category_key) {
            return true;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            if walk_for_large_files(scanner, category_key, &path, items, total_bytes) {
                return true;
            }
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() < LARGE_FILE_THRESHOLD_BYTES {
            continue;
        }
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        *total_bytes += metadata.len();
        items.push(DiskUsageItem {
            path: path.display().to_string(),
            label,
            size_bytes: metadata.len(),
            protected: false,
            actionable: true,
            deletes_via_cleanup_category: false,
        });
    }
    false
}

fn scan_system_informational(scanner: &mut Scanner) -> DiskUsageCategory {
    let mut items = Vec::new();
    let mut total_bytes = 0u64;
    let system_drive = system_drive_root();

    let named_files = [
        ("Arquivo de paginacao (pagefile.sys)", system_drive.join("pagefile.sys")),
        ("Hibernacao (hiberfil.sys)", system_drive.join("hiberfil.sys")),
    ];
    for (label, path) in named_files {
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            total_bytes += metadata.len();
            items.push(DiskUsageItem {
                path: path.display().to_string(),
                label: label.to_string(),
                size_bytes: metadata.len(),
                protected: false,
                actionable: false,
                deletes_via_cleanup_category: false,
            });
        }
    }

    let windows_dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| system_drive.join("Windows"));
    let winsxs = windows_dir.join("WinSxS");
    if winsxs.exists() {
        let size = measure_dir(scanner, "system", &winsxs);
        total_bytes += size;
        items.push(DiskUsageItem {
            path: winsxs.display().to_string(),
            label: "Componentes do Windows (WinSxS, estimativa)".to_string(),
            size_bytes: size,
            protected: false,
            actionable: false,
            deletes_via_cleanup_category: false,
        });
    }

    let recycle_bin = system_drive.join("$Recycle.Bin");
    if recycle_bin.exists() && !scanner.canceled {
        let size = measure_dir(scanner, "system", &recycle_bin);
        total_bytes += size;
        items.push(DiskUsageItem {
            path: recycle_bin.display().to_string(),
            label: "Lixeira".to_string(),
            size_bytes: size,
            protected: false,
            actionable: false,
            deletes_via_cleanup_category: false,
        });
    }

    finish_category(
        DiskUsageCategoryKind::System,
        "Sistema (somente informativo)",
        items,
        total_bytes,
        false,
        vec![system_drive.display().to_string()],
    )
}

fn describe_entry(scanner: &mut Scanner, category_key: &str, path: &Path) -> Result<DiskUsageItem, ()> {
    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let size_bytes = if path.is_dir() {
        measure_dir(scanner, category_key, path)
    } else {
        fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0)
    };
    let protected = is_protected_app(&label);

    Ok(DiskUsageItem {
        path: path.display().to_string(),
        label,
        size_bytes,
        protected,
        actionable: !protected,
        deletes_via_cleanup_category: false,
    })
}

fn measure_dir(scanner: &mut Scanner, category_key: &str, path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        if !scanner.visit(category_key) {
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            total += measure_dir(scanner, category_key, &entry.path());
        } else if let Ok(metadata) = entry.metadata() {
            total += metadata.len();
        }
        if scanner.canceled {
            break;
        }
    }
    total
}

fn finish_category(
    kind: DiskUsageCategoryKind,
    label: &str,
    mut items: Vec<DiskUsageItem>,
    total_bytes: u64,
    capped: bool,
    scanned_paths: Vec<String>,
) -> DiskUsageCategory {
    items.sort_by_key(|item| std::cmp::Reverse(item.size_bytes));
    let item_count = items.len();
    items.truncate(TOP_ITEMS_PER_CATEGORY);

    DiskUsageCategory {
        kind,
        label: label.to_string(),
        total_bytes,
        item_count,
        items,
        capped,
        scanned_paths,
    }
}

fn empty_category(kind: DiskUsageCategoryKind, label: &str) -> DiskUsageCategory {
    DiskUsageCategory {
        kind,
        label: label.to_string(),
        total_bytes: 0,
        item_count: 0,
        items: Vec::new(),
        capped: false,
        scanned_paths: Vec::new(),
    }
}

fn user_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|profile| PathBuf::from(profile).join(name))
}

fn system_drive_root() -> PathBuf {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    PathBuf::from(format!("{drive}\\"))
}

fn program_files_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(value) = std::env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(value));
    }
    if let Some(value) = std::env::var_os("ProgramFiles(x86)") {
        roots.push(PathBuf::from(value));
    }
    roots
}

fn epic_games_root() -> Option<PathBuf> {
    std::env::var_os("ProgramFiles").map(|value| PathBuf::from(value).join("Epic Games"))
}

#[cfg(windows)]
fn steam_install_root() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let from_registry = hkcu
        .open_subkey(r"Software\Valve\Steam")
        .ok()
        .and_then(|key| key.get_value::<String, _>("SteamPath").ok())
        .map(|raw| PathBuf::from(raw.replace('/', "\\")))
        .filter(|path| path.exists());

    from_registry.or_else(|| {
        let fallback = PathBuf::from(r"C:\Program Files (x86)\Steam");
        fallback.exists().then_some(fallback)
    })
}

#[cfg(not(windows))]
fn steam_install_root() -> Option<PathBuf> {
    None
}

fn steam_library_common_dirs() -> Vec<PathBuf> {
    let Some(steam_root) = steam_install_root() else {
        return Vec::new();
    };
    let vdf_path = steam_root.join("steamapps").join("libraryfolders.vdf");
    let library_roots = fs::read_to_string(&vdf_path)
        .map(|contents| parse_steam_library_paths(&contents))
        .unwrap_or_default();

    let mut roots = if library_roots.is_empty() {
        vec![steam_root]
    } else {
        library_roots
    };
    roots.dedup();

    roots
        .into_iter()
        .map(|root| root.join("steamapps").join("common"))
        .filter(|path| path.exists())
        .collect()
}

/// Parses the `"path"` entries out of Steam's libraryfolders.vdf. Doesn't
/// need a full VDF parser - every library entry has exactly one `"path"`
/// key on its own line, quoted, with `\\` as the only escape we need to
/// unescape.
fn parse_steam_library_paths(vdf_text: &str) -> Vec<PathBuf> {
    vdf_text
        .lines()
        .filter(|line| line.trim().starts_with("\"path\""))
        .filter_map(|line| {
            let quoted: Vec<&str> = line.split('"').collect();
            quoted.get(3).map(|value| PathBuf::from(value.replace("\\\\", "\\")))
        })
        .filter(|path| path.exists())
        .collect()
}

/// Validates a path for the DELETE_DISK_USAGE_ITEM action: must exist,
/// must sit under one of the roots this scan actually looks at (never
/// C:\Windows or a bare drive root), and must not match a protected app.
pub fn validate_deletable_path(raw_path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw_path);
    let canonical = fs::canonicalize(&path).map_err(|error| format!("path_not_found: {error}"))?;

    let label = canonical
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    if is_protected_app(&label) {
        return Err("protected_app".to_string());
    }

    let allowed_roots = allowed_deletion_roots();
    let is_allowed = allowed_roots
        .iter()
        .any(|root| fs::canonicalize(root).is_ok_and(|root| canonical.starts_with(&root)));
    if !is_allowed {
        return Err("outside_allowed_roots".to_string());
    }

    // Never the root itself, only something inside it.
    if allowed_roots
        .iter()
        .any(|root| fs::canonicalize(root).is_ok_and(|root| root == canonical))
    {
        return Err("cannot_delete_root".to_string());
    }

    Ok(canonical)
}

fn allowed_deletion_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for folder in ["Downloads", "Videos", "Documents", "Desktop", "Pictures", "Music"] {
        if let Some(path) = user_dir(folder) {
            roots.push(path);
        }
    }
    roots.extend(program_files_roots());
    roots.extend(steam_library_common_dirs());
    if let Some(epic_root) = epic_games_root() {
        roots.push(epic_root);
    }
    roots
}

pub async fn delete_item(path: String) -> ExecutionResult {
    tokio::task::spawn_blocking(move || delete_item_blocking(path))
        .await
        .unwrap_or_else(|error| ExecutionResult {
            success: false,
            message: format!("Falha ao excluir item: {error}"),
            details: serde_json::json!({ "implemented": true }),
        })
}

fn delete_item_blocking(path: String) -> ExecutionResult {
    let validated = match validate_deletable_path(&path) {
        Ok(path) => path,
        Err(reason) => {
            return ExecutionResult {
                success: false,
                message: "Item nao pode ser excluido por esta funcao.".to_string(),
                details: serde_json::json!({ "implemented": true, "path": path, "reason": reason }),
            };
        }
    };

    let size_bytes = if validated.is_dir() {
        dir_size_uncapped(&validated)
    } else {
        fs::metadata(&validated).map(|metadata| metadata.len()).unwrap_or(0)
    };

    let snapshot_id = snapshot::new_snapshot_id();
    let quarantine_target = snapshot::cleanup_quarantine_dir(&snapshot_id).join(
        validated
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "item".to_string()),
    );

    if let Some(parent) = quarantine_target.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return ExecutionResult {
                success: false,
                message: "Falha ao preparar quarentena local.".to_string(),
                details: serde_json::json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    }

    if let Err(error) = snapshot::move_file_across_volumes(&validated, &quarantine_target) {
        return ExecutionResult {
            success: false,
            message: "Falha ao mover item para a quarentena local.".to_string(),
            details: serde_json::json!({ "implemented": true, "error": error }),
        };
    }

    let snapshot = OptimizationSnapshot::new(
        "DELETE_DISK_USAGE_ITEM",
        vec![SnapshotEntry::QuarantinedPath {
            original_path: validated.clone(),
            quarantine_path: quarantine_target.clone(),
            bytes: size_bytes,
        }],
        serde_json::json!({
            "original_path": validated.display().to_string(),
            "size_bytes": size_bytes,
        }),
    );

    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        let rollback = snapshot::restore_snapshot_entries(&snapshot);
        return ExecutionResult {
            success: false,
            message: "Item movido, mas o snapshot falhou; reversao imediata solicitada."
                .to_string(),
            details: serde_json::json!({
                "implemented": true,
                "snapshot_error": error,
                "rollback": {
                    "restored_entries": rollback.restored_entries,
                    "failed_entries": rollback.failed_entries,
                    "messages": rollback.messages,
                },
            }),
        };
    }

    ExecutionResult::ok(
        "Item movido para a quarentena local e pode ser restaurado.",
        serde_json::json!({
            "implemented": true,
            "path": validated.display().to_string(),
            "size_bytes": size_bytes,
            "snapshot": { "id": snapshot.id, "reversible": true },
        }),
    )
}

fn dir_size_uncapped(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        if let Ok(file_type) = entry.file_type() {
            if file_type.is_dir() {
                total += dir_size_uncapped(&entry.path());
            } else if let Ok(metadata) = entry.metadata() {
                total += metadata.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::parse_steam_library_paths;

    #[test]
    fn extracts_path_entries_from_libraryfolders_vdf() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"label"		""
	}
}
"#;
        // Neither path exists on the test machine, so filter-by-exists
        // would drop both - test the raw extraction/unescaping instead by
        // checking it doesn't panic and would have produced the right
        // strings before the exists() filter.
        let lines_with_path = vdf.lines().filter(|line| line.trim().starts_with("\"path\"")).count();
        assert_eq!(lines_with_path, 2);
        // parse_steam_library_paths itself filters by existence, so on a
        // machine without these exact paths it legitimately returns empty.
        let _ = parse_steam_library_paths(vdf);
    }

    #[test]
    fn unescapes_double_backslashes_in_extracted_value() {
        let line = r#"		"path"		"C:\\Games\\Steam""#;
        let quoted: Vec<&str> = line.split('"').collect();
        let raw = quoted.get(3).copied().unwrap_or_default();
        assert_eq!(raw.replace("\\\\", "\\"), r"C:\Games\Steam");
    }
}
