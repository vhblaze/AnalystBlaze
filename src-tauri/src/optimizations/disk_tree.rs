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

/// Lists the immediate children of `path`, sorted by size descending, with
/// each directory's total computed on the spot. Deliberately not cached
/// anywhere: nothing about a listing survives past this call, so browsing
/// away (or the scan finishing) can't leak memory the way holding a
/// whole-drive tree in AgentState did - every navigation just re-asks the
/// filesystem for whatever it's currently showing.
pub async fn list_directory(
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    path: String,
) -> Result<Vec<DiskTreeNodeSummary>, String> {
    let dir_path = PathBuf::from(&path);
    if !dir_path.is_dir() {
        return Err("not_a_directory".to_string());
    }
    tokio::task::spawn_blocking(move || list_directory_blocking(app, cancel, dir_path))
        .await
        .map_err(|error| format!("scan_join_error: {error}"))
}

fn list_directory_blocking(
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    dir_path: PathBuf,
) -> Vec<DiskTreeNodeSummary> {
    let scanner = Scanner::new(app, cancel);
    let entries: Vec<PathBuf> = fs::read_dir(&dir_path)
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
                summarize_entry(&scanner, &child_path)
            })
            .collect()
    });

    items.sort_by_key(|item| std::cmp::Reverse(item.size_bytes));
    scanner.emit_done();
    items
}

fn summarize_entry(scanner: &Scanner, path: &Path) -> Option<DiskTreeNodeSummary> {
    let metadata = fs::symlink_metadata(path).ok()?;
    // Reparse points (symlinks and NTFS junctions/mount points) are
    // treated as non-directories to avoid cycles.
    let is_dir = metadata.is_dir() && !metadata.file_type().is_symlink();
    let modified_at = metadata_modified_at(&metadata);
    let name = path.file_name()?.to_string_lossy().to_string();
    let protected = is_protected_app(&name);

    let size_bytes = if is_dir {
        compute_dir_size(scanner, path)
    } else {
        metadata.len()
    };

    Some(DiskTreeNodeSummary {
        path: path.display().to_string(),
        name,
        size_bytes,
        is_dir,
        modified_at,
        protected,
        actionable: !protected && !is_system_critical_path(path),
    })
}

/// Total size of `path`'s subtree, computed fresh (no persistent tree) -
/// parallelized on the bounded scan_pool so a big folder still overlaps
/// I/O wait across a few threads without competing for every core.
fn compute_dir_size(scanner: &Scanner, path: &Path) -> u64 {
    if !scanner.should_continue(&path.display().to_string()) {
        return 0;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();

    use rayon::prelude::*;
    children
        .into_par_iter()
        .map(|child_path| {
            if scanner.canceled.load(Ordering::Relaxed) || scanner.capped.load(Ordering::Relaxed) {
                return 0;
            }
            let Ok(metadata) = fs::symlink_metadata(&child_path) else {
                return 0;
            };
            if metadata.file_type().is_symlink() {
                return 0;
            }
            if metadata.is_dir() {
                compute_dir_size(scanner, &child_path)
            } else {
                metadata.len()
            }
        })
        .sum()
}

fn metadata_modified_at(metadata: &fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
}
