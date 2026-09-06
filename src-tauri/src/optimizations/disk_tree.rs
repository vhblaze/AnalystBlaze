use rayon::{ThreadPool, ThreadPoolBuilder};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use super::disk_usage::is_system_critical_path;
use super::protected_apps::is_protected_app;

pub const DISK_TREE_PROGRESS_EVENT: &str = "disk-tree-scan-progress";
/// Emitted once per directory child after its real recursive size (and
/// protected-descendant check) finishes in the background - see
/// `list_directory`'s docs for why that's no longer computed before the
/// listing itself is returned.
pub const DISK_TREE_ITEM_READY_EVENT: &str = "disk-tree-item-ready";

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(250);
/// Safety valve for pathological folders (millions of tiny files) - stops
/// walking and reports `capped` instead of running unbounded.
const NODE_CAP: usize = 1_500_000;

/// Deliberately NOT the full core count. A whole-drive scan on the global
/// rayon pool (num_cpus threads, no ceiling) pegged every core and made the
/// rest of the machine feel unusable while it ran - this app is a
/// background helper, not the user's foreground task, so its own scans get
/// a bounded slice instead of competing for every core. Floor of 2 so it's
/// still meaningfully parallel on dual-core machines.
fn scan_pool() -> &'static ThreadPool {
    static POOL: OnceLock<ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(4);
        let threads = (cores / 2).clamp(2, 6);
        ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("disk-scan-{index}"))
            .build()
            .expect("failed to build bounded disk-scan thread pool")
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskVolumeInfo {
    pub mount_point: String,
    pub label: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub file_system: String,
    pub is_removable: bool,
}

/// Lists mounted volumes for the drive picker. NTFS is where this scanner
/// performs best; other filesystems (exFAT/FAT32 on external/removable
/// drives, etc.) still work through the same conventional walk, just
/// without a future MFT fast-path.
pub fn list_volumes() -> Vec<DiskVolumeInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .map(|disk| {
            let raw_name = disk.name().to_string_lossy().to_string();
            let mount_point = disk.mount_point().to_string_lossy().to_string();
            DiskVolumeInfo {
                label: if raw_name.trim().is_empty() {
                    mount_point.clone()
                } else {
                    raw_name
                },
                mount_point,
                total_bytes: disk.total_space(),
                available_bytes: disk.available_space(),
                file_system: disk.file_system().to_string_lossy().to_string(),
                is_removable: disk.is_removable(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskTreeNodeSummary {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub is_dir: bool,
    pub modified_at: Option<i64>,
    pub protected: bool,
    /// False for protected apps and anything under
    /// `is_system_critical_path` - informational only in the UI, same
    /// contract as the categorized disk-usage scan's System category.
    pub actionable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskTreeProgress {
    pub current_path: String,
    pub scanned_nodes: usize,
    pub done: bool,
}

/// The final, authoritative size/protection state for one directory child,
/// replacing the placeholder `list_directory` returned for it. Carries the
/// full DiskTreeNodeSummary contract (self name/descendant protection
/// already folded together, same as `protected`/`actionable` everywhere
/// else) rather than a raw byte count, so the frontend can just overwrite
/// that row's fields wholesale instead of merging partial state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskTreeItemUpdate {
    pub path: String,
    pub size_bytes: u64,
    pub protected: bool,
    pub actionable: bool,
}

/// Filesystem walks are I/O-bound (mostly waiting on `stat`/`read_dir`
/// syscalls, not CPU), so splitting work across threads overlaps that wait
/// time instead of doing it serially. `should_continue` is called
/// concurrently from every worker, so every counter is atomic.
struct Scanner {
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    node_count: AtomicUsize,
    started: Instant,
    last_emit_millis: AtomicU64,
    canceled: AtomicBool,
    capped: AtomicBool,
}

impl Scanner {
    fn new(app: AppHandle, cancel: Arc<AtomicBool>) -> Self {
        Self {
            app,
            cancel,
            node_count: AtomicUsize::new(0),
            started: Instant::now(),
            last_emit_millis: AtomicU64::new(0),
            canceled: AtomicBool::new(false),
            capped: AtomicBool::new(false),
        }
    }

    /// Called once per node (file or directory) visited, from any worker
    /// thread - cheap enough (a few atomic ops, an occasional event emit
    /// only for whichever thread wins the CAS) to check on every entry
    /// rather than only when descending into directories, so cancel stays
    /// responsive even inside one huge flat folder.
    fn should_continue(&self, current_path: &str) -> bool {
        if self.canceled.load(Ordering::Relaxed) {
            return false;
        }
        if self.cancel.load(Ordering::Relaxed) {
            self.canceled.store(true, Ordering::Relaxed);
            return false;
        }
        let count = self.node_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= NODE_CAP {
            self.capped.store(true, Ordering::Relaxed);
            return false;
        }
        let now = self.started.elapsed().as_millis() as u64;
        let last = self.last_emit_millis.load(Ordering::Relaxed);
        let interval = PROGRESS_EMIT_INTERVAL.as_millis() as u64;
        if now.saturating_sub(last) >= interval
            && self
                .last_emit_millis
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            let _ = self.app.emit(
                DISK_TREE_PROGRESS_EVENT,
                DiskTreeProgress {
                    current_path: current_path.to_string(),
                    scanned_nodes: count,
                    done: false,
                },
            );
        }
        true
    }

    fn emit_item_ready(&self, update: DiskTreeItemUpdate) {
        let _ = self.app.emit(DISK_TREE_ITEM_READY_EVENT, update);
    }

    fn emit_done(&self) {
        let _ = self.app.emit(
            DISK_TREE_PROGRESS_EVENT,
            DiskTreeProgress {
                current_path: "done".to_string(),
                scanned_nodes: self.node_count.load(Ordering::Relaxed),
                done: true,
            },
        );
    }
}

/// Lists the immediate children of `path` instantly - one non-recursive
/// `read_dir` plus a single `stat` per entry, no descending into any
/// subfolder - then resolves each directory child's real recursive size
/// (and whether it hides a protected item deep inside) in the background,
/// emitting one `DISK_TREE_ITEM_READY_EVENT` per directory as its result
/// lands.
///
/// This used to compute every visible directory's full recursive size
/// before returning anything at all, which meant opening a folder with
/// even one huge subfolder (AppData, Program Files, node_modules) blocked
/// the ENTIRE listing - including the cheap file entries sitting right
/// next to it - on however long the slowest subfolder's whole subtree took
/// to walk. On a folder like a drive root, where every visible entry is
/// itself a huge subtree, that meant the screen could show nothing at all
/// for a long time while burning CPU on stat() calls across most of the
/// drive. Returning the listing after only the fast phase means the user
/// sees names and file sizes immediately; directory sizes fill in as they
/// finish instead of gating the whole screen.
///
/// Deliberately not cached anywhere beyond that in-flight background
/// resolution: nothing about a listing survives past this call, so
/// browsing away (or the scan finishing) can't leak memory the way holding
/// a whole-drive tree in AgentState did - every navigation just re-asks
/// the filesystem for whatever it's currently showing.
pub async fn list_directory(
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    path: String,
) -> Result<Vec<DiskTreeNodeSummary>, String> {
    let dir_path = PathBuf::from(&path);
    if !dir_path.is_dir() {
        return Err("not_a_directory".to_string());
    }
    let scanner = Arc::new(Scanner::new(app, cancel));

    let fast_scanner = scanner.clone();
    let fast_dir_path = dir_path.clone();
    let items = tokio::task::spawn_blocking(move || list_directory_fast(&fast_scanner, &fast_dir_path))
        .await
        .map_err(|error| format!("scan_join_error: {error}"))?;

    let pending_dirs: Vec<PathBuf> = items
        .iter()
        .filter(|item| item.is_dir)
        .map(|item| PathBuf::from(&item.path))
        .collect();

    if pending_dirs.is_empty() {
        // Nothing to resolve in the background (an empty folder, or one
        // holding only files) - the listing above is already the final
        // state, so signal "done" right away instead of leaving the
        // progress indicator waiting on a background phase that was never
        // going to run.
        scanner.emit_done();
    } else {
        let bg_scanner = scanner.clone();
        tokio::task::spawn_blocking(move || resolve_pending_sizes(&bg_scanner, pending_dirs));
    }

    Ok(items)
}

fn list_directory_fast(scanner: &Scanner, dir_path: &Path) -> Vec<DiskTreeNodeSummary> {
    let entries: Vec<PathBuf> = fs::read_dir(dir_path)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();

    let mut items: Vec<DiskTreeNodeSummary> = scan_pool().install(|| {
        use rayon::prelude::*;
        entries
            .into_par_iter()
            .filter_map(|child_path| {
                if scanner.canceled.load(Ordering::Relaxed) || scanner.capped.load(Ordering::Relaxed) {
                    return None;
                }
                summarize_entry_fast(&child_path)
            })
            .collect()
    });

    items.sort_by_key(|item| std::cmp::Reverse(item.size_bytes));
    items
}

/// Top-level-only: never descends into a directory, so this is always just
/// the cost of one `stat` regardless of how big the entry's own subtree
/// is. A directory's `size_bytes` is a "still calculating" placeholder
/// (`0`) and `protected`/`actionable` only reflect its own name/path here.
/// `resolve_pending_sizes` fills in the authoritative version once its
/// subtree walk finishes, which can only ever add protection, never
/// remove it.
fn summarize_entry_fast(path: &Path) -> Option<DiskTreeNodeSummary> {
    let metadata = fs::symlink_metadata(path).ok()?;
    // Reparse points (symlinks and NTFS junctions/mount points) are
    // treated as non-directories to avoid cycles.
    let is_dir = metadata.is_dir() && !metadata.file_type().is_symlink();
    let modified_at = metadata_modified_at(&metadata);
    let name = path.file_name()?.to_string_lossy().to_string();
    let self_protected = is_protected_app(&name);
    let size_bytes = if is_dir { 0 } else { metadata.len() };

    Some(DiskTreeNodeSummary {
        path: path.display().to_string(),
        name,
        size_bytes,
        is_dir,
        modified_at,
        protected: self_protected,
        actionable: !self_protected && !is_system_critical_path(path),
    })
}

/// The expensive part: one full recursive subtree walk per pending
/// directory, parallelized across the same bounded pool as everything else
/// here. Runs after `list_directory` has already returned the listing, so
/// it never blocks the screen from showing up - only these size numbers
/// arrive late, via one `DISK_TREE_ITEM_READY_EVENT` per directory as its
/// own walk finishes (not batched, since the immediate-children count this
/// runs over is always small - tens, not millions).
fn resolve_pending_sizes(scanner: &Scanner, dirs: Vec<PathBuf>) {
    use rayon::prelude::*;
    scan_pool().install(|| {
        dirs.into_par_iter().for_each(|dir_path| {
            if scanner.canceled.load(Ordering::Relaxed) {
                return;
            }
            // For a directory, `has_protected_descendant` folds in the SAME
            // walk that already computes size - a folder whose own name is
            // completely harmless can still recursively contain something
            // that must never be deleted (a security tool's data directory
            // nested a few levels down, say), and this is what surfaces
            // that in the listing before the user ever tries to delete it.
            // disk_usage.rs's validate_deletable_path runs the equivalent
            // check again at actual delete time (the real enforcement
            // point, not just this UI hint) via find_protected_descendant.
            let (size_bytes, has_protected_descendant) = scan_subtree(scanner, &dir_path);
            let name = dir_path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let protected = is_protected_app(&name) || has_protected_descendant;
            scanner.emit_item_ready(DiskTreeItemUpdate {
                path: dir_path.display().to_string(),
                size_bytes,
                protected,
                actionable: !protected && !is_system_critical_path(&dir_path),
            });
        });
    });
    scanner.emit_done();
}

/// Total size of `path`'s subtree AND whether it contains anything
/// is_protected_app/is_system_critical_path would refuse to delete on its
/// own - computed together in one walk (parallelized on the bounded
/// scan_pool, same as before) rather than two separate passes over
/// potentially the same few hundred thousand files.
fn scan_subtree(scanner: &Scanner, path: &Path) -> (u64, bool) {
    if !scanner.should_continue(&path.display().to_string()) {
        return (0, false);
    }
    let Ok(entries) = fs::read_dir(path) else {
        return (0, false);
    };
    let children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();

    use rayon::prelude::*;
    children
        .into_par_iter()
        .map(|child_path| {
            if scanner.canceled.load(Ordering::Relaxed) || scanner.capped.load(Ordering::Relaxed) {
                return (0, false);
            }
            let Ok(metadata) = fs::symlink_metadata(&child_path) else {
                return (0, false);
            };
            if metadata.file_type().is_symlink() {
                return (0, false);
            }
            let name = child_path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let child_protected = is_protected_app(&name) || is_system_critical_path(&child_path);
            if metadata.is_dir() {
                let (size, descendant_protected) = scan_subtree(scanner, &child_path);
                (size, child_protected || descendant_protected)
            } else {
                (metadata.len(), child_protected)
            }
        })
        .reduce(|| (0, false), |a, b| (a.0 + b.0, a.1 || b.1))
}

fn metadata_modified_at(metadata: &fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
}
