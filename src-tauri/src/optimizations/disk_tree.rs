use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use super::disk_usage::is_system_critical_path;
use super::protected_apps::is_protected_app;

pub const DISK_TREE_PROGRESS_EVENT: &str = "disk-tree-scan-progress";

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(250);
/// Safety valve for pathological trees (millions of tiny files). WizTree
/// avoids this entirely by reading the MFT directly instead of walking
/// each file with `read_dir`/`stat` - see the D6 audit notes on why this
/// conventional scanner was chosen for the first cut and what an MFT
/// reader (via the privileged helper) would remove. Past this many nodes
/// the scan stops growing the in-memory tree and reports `capped` instead
/// of running unbounded.
const NODE_CAP: usize = 1_500_000;

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
pub struct DiskTreeScanSummary {
    pub root: String,
    pub total_size_bytes: u64,
    pub dir_count: usize,
    pub file_count: usize,
    pub scanned_at: i64,
    pub duration_ms: u64,
    pub canceled: bool,
    pub capped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskTreeProgress {
    pub current_path: String,
    pub scanned_nodes: usize,
    pub done: bool,
}

/// In-memory only - never serialized wholesale to the frontend. A full
/// drive can have millions of nodes; only `children_of`/`node_summary`
/// slices cross the Tauri IPC boundary, sized to whatever the UI is
/// currently browsing.
#[derive(Debug)]
struct TreeNode {
    size_bytes: u64,
    is_dir: bool,
    modified_at: Option<i64>,
    protected: bool,
    children: HashMap<String, TreeNode>,
}

/// Held in AgentState behind a mutex between scans so the UI can browse
/// (children_of) without re-scanning.
pub struct DiskTree {
    root_path: PathBuf,
    root: TreeNode,
}

struct Scanner {
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    node_count: usize,
    dir_count: usize,
    file_count: usize,
    last_emit: Instant,
    canceled: bool,
    capped: bool,
}

impl Scanner {
    fn new(app: AppHandle, cancel: Arc<AtomicBool>) -> Self {
        Self {
            app,
            cancel,
            node_count: 0,
            dir_count: 0,
            file_count: 0,
            last_emit: Instant::now() - PROGRESS_EMIT_INTERVAL,
            canceled: false,
            capped: false,
        }
    }

    /// Called once per node (file or directory) visited - cheap enough
    /// (an atomic load plus an occasional event emit) to check on every
    /// entry rather than only when descending into directories, so
    /// cancel stays responsive even inside one huge flat folder.
    fn should_continue(&mut self, current_path: &str) -> bool {
        if self.canceled {
            return false;
        }
        if self.cancel.load(Ordering::Relaxed) {
            self.canceled = true;
            return false;
        }
        if self.node_count >= NODE_CAP {
            self.capped = true;
            return false;
        }
        if self.last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
            self.last_emit = Instant::now();
            let _ = self.app.emit(
                DISK_TREE_PROGRESS_EVENT,
                DiskTreeProgress {
                    current_path: current_path.to_string(),
                    scanned_nodes: self.node_count,
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
                scanned_nodes: self.node_count,
                done: true,
            },
        );
    }
}

pub async fn scan_disk_tree(
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    root: String,
) -> Result<(DiskTree, DiskTreeScanSummary), String> {
    let root_path = PathBuf::from(&root);
    if !root_path.exists() {
        return Err("root_not_found".to_string());
    }
    tokio::task::spawn_blocking(move || scan_disk_tree_blocking(app, cancel, root_path))
        .await
        .map_err(|error| format!("scan_join_error: {error}"))
}

fn scan_disk_tree_blocking(
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    root_path: PathBuf,
) -> (DiskTree, DiskTreeScanSummary) {
    let started = Instant::now();
    let mut scanner = Scanner::new(app, cancel);
    let root_node = build_node(&mut scanner, &root_path);
    scanner.emit_done();

    let summary = DiskTreeScanSummary {
        root: root_path.display().to_string(),
        total_size_bytes: root_node.size_bytes,
        dir_count: scanner.dir_count,
        file_count: scanner.file_count,
        scanned_at: chrono::Utc::now().timestamp(),
        duration_ms: started.elapsed().as_millis() as u64,
        canceled: scanner.canceled,
        capped: scanner.capped,
    };

    (
        DiskTree {
            root_path,
            root: root_node,
        },
        summary,
    )
}

fn build_node(scanner: &mut Scanner, path: &Path) -> TreeNode {
    let metadata = fs::symlink_metadata(path).ok();
    // Reparse points (symlinks and NTFS junctions/mount points) are
    // treated as non-directories to avoid cycles - see module docs.
    let is_dir = metadata
        .as_ref()
        .map(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        .unwrap_or(false);
    let modified_at = metadata.as_ref().and_then(metadata_modified_at);
    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let protected = is_protected_app(&label);
    let path_str = path.display().to_string();

    scanner.node_count += 1;
    if is_dir {
        scanner.dir_count += 1;
    } else {
        scanner.file_count += 1;
    }

    if !scanner.should_continue(&path_str) || !is_dir {
        return TreeNode {
            size_bytes: if is_dir { 0 } else { metadata.map(|meta| meta.len()).unwrap_or(0) },
            is_dir,
            modified_at,
            protected,
            children: HashMap::new(),
        };
    }

    let mut children = HashMap::new();
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if scanner.canceled || scanner.capped {
                break;
            }
            let child_name = entry.file_name().to_string_lossy().to_string();
            let child_node = build_node(scanner, &entry.path());
            total += child_node.size_bytes;
            children.insert(child_name, child_node);
        }
    }

    TreeNode {
        size_bytes: total,
        is_dir: true,
        modified_at,
        protected,
        children,
    }
}

fn metadata_modified_at(metadata: &fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
}

fn find_node<'a>(tree: &'a DiskTree, target: &Path) -> Result<&'a TreeNode, String> {
    if target == tree.root_path {
        return Ok(&tree.root);
    }
    let relative = target
        .strip_prefix(&tree.root_path)
        .map_err(|_| "outside_scanned_root".to_string())?;
    let mut node = &tree.root;
    for component in relative.components() {
        let key = component.as_os_str().to_string_lossy().to_string();
        node = node
            .children
            .get(&key)
            .ok_or_else(|| "node_not_found".to_string())?;
    }
    Ok(node)
}

fn summarize(target_path: &Path, node: &TreeNode) -> DiskTreeNodeSummary {
    let name = target_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| target_path.display().to_string());
    DiskTreeNodeSummary {
        path: target_path.display().to_string(),
        name,
        size_bytes: node.size_bytes,
        is_dir: node.is_dir,
        modified_at: node.modified_at,
        protected: node.protected,
        actionable: !node.protected && !is_system_critical_path(target_path),
    }
}

/// Metadata for a single already-scanned node (used for the breadcrumb
/// header / treemap headline when the user drills into a folder).
pub fn node_summary(tree: &DiskTree, target: &str) -> Result<DiskTreeNodeSummary, String> {
    let target_path = PathBuf::from(target);
    let node = find_node(tree, &target_path)?;
    Ok(summarize(&target_path, node))
}

/// Immediate children of `target` within an already-scanned tree, sorted
/// by size descending - the only thing that crosses the IPC boundary
/// while browsing; the full tree itself is never serialized at once.
pub fn children_of(tree: &DiskTree, target: &str) -> Result<Vec<DiskTreeNodeSummary>, String> {
    let target_path = PathBuf::from(target);
    let node = find_node(tree, &target_path)?;
    let mut items: Vec<DiskTreeNodeSummary> = node
        .children
        .iter()
        .map(|(name, child)| summarize(&target_path.join(name), child))
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.size_bytes));
    Ok(items)
}
