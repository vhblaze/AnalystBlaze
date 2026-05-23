use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};

pub async fn empty_temp(payload: Option<Value>) -> ExecutionResult {
    let min_age = Duration::from_secs(
        payload
            .as_ref()
            .and_then(|value| value.get("min_age_hours"))
            .and_then(Value::as_u64)
            .unwrap_or(24)
            * 60
            * 60,
    );
    let snapshot_id = snapshot::new_snapshot_id();

    match tokio::task::spawn_blocking(move || quarantine_temp_files(min_age, &snapshot_id)).await {
        Ok(summary) => cleanup_result(summary, min_age),
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao executar limpeza TEMP: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

#[derive(Debug, Default)]
struct CleanupSummary {
    snapshot_id: String,
    quarantined_files: usize,
    removed_empty_dirs: usize,
    failed_entries: usize,
    quarantined_bytes: u64,
    entries: Vec<SnapshotEntry>,
}

fn quarantine_temp_files(min_age: Duration, snapshot_id: &str) -> CleanupSummary {
    let temp_dir = std::env::temp_dir();
    let quarantine_root = snapshot::cleanup_quarantine_dir(snapshot_id);
    let mut summary = CleanupSummary {
        snapshot_id: snapshot_id.to_string(),
        ..CleanupSummary::default()
    };
    quarantine_dir(
        &temp_dir,
        &temp_dir,
        &quarantine_root,
        min_age,
        &mut summary,
    );
    summary
}

fn cleanup_result(summary: CleanupSummary, min_age: Duration) -> ExecutionResult {
    if summary.entries.is_empty() {
        return ExecutionResult::ok(
            "Nenhum arquivo temporario antigo elegivel para quarentena.",
            json!({
                "implemented": true,
                "targets": ["TEMP"],
                "quarantined_files": 0,
                "removed_empty_dirs": 0,
                "failed_entries": summary.failed_entries,
                "quarantined_bytes": 0,
                "min_age_hours": min_age.as_secs() / 3600,
                "snapshot": null,
            }),
        );
    }

    let snapshot = OptimizationSnapshot {
        id: summary.snapshot_id.clone(),
        action_name: "EMPTY_TEMP".to_string(),
        created_at: chrono::Utc::now().timestamp(),
        restored_at: None,
        entries: summary.entries.clone(),
        details: json!({
            "target": "TEMP",
            "quarantined_files": summary.quarantined_files,
            "removed_empty_dirs": summary.removed_empty_dirs,
            "quarantined_bytes": summary.quarantined_bytes,
            "min_age_hours": min_age.as_secs() / 3600,
        }),
    };

    match snapshot::save_snapshot(&snapshot) {
        Ok(()) => ExecutionResult::ok(
            "Arquivos temporarios antigos movidos para quarentena reversivel.",
            json!({
                "implemented": true,
                "targets": ["TEMP"],
                "quarantined_files": summary.quarantined_files,
                "removed_empty_dirs": summary.removed_empty_dirs,
                "failed_entries": summary.failed_entries,
                "quarantined_bytes": summary.quarantined_bytes,
                "min_age_hours": min_age.as_secs() / 3600,
                "snapshot": {
                    "id": snapshot.id,
                    "entries": snapshot.entries.len(),
                    "reversible": true,
                    "space_reclaim_pending": true,
                },
            }),
        ),
        Err(error) => {
            let rollback = snapshot::restore_snapshot_entries(&snapshot);
            ExecutionResult {
                success: false,
                message: "Limpeza revertida porque nao foi possivel salvar o snapshot local."
                    .to_string(),
                details: json!({
                    "implemented": true,
                    "targets": ["TEMP"],
                    "snapshot_error": error,
                    "rollback": {
                        "restored_entries": rollback.restored_entries,
                        "failed_entries": rollback.failed_entries,
                        "skipped_conflicts": rollback.skipped_conflicts,
                        "messages": rollback.messages,
                    },
                }),
            }
        }
    }
}

fn quarantine_dir(
    root: &Path,
    dir: &Path,
    quarantine_root: &Path,
    min_age: Duration,
    summary: &mut CleanupSummary,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        summary.failed_entries += 1;
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path_stays_inside(root, &path) || path_stays_inside(quarantine_root, &path) {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            summary.failed_entries += 1;
            continue;
        };

        if metadata.file_type().is_symlink() {
            continue;
        }

        if metadata.is_dir() {
            quarantine_dir(root, &path, quarantine_root, min_age, summary);
            if is_old_enough(&metadata, min_age) {
                match fs::remove_dir(&path) {
                    Ok(()) => {
                        summary.removed_empty_dirs += 1;
                        summary.entries.push(SnapshotEntry::RemovedEmptyDir {
                            original_path: path,
                        });
                    }
                    Err(_) => {}
                }
            }
            continue;
        }

        if !metadata.is_file() || !is_old_enough(&metadata, min_age) {
            continue;
        }

        let len = metadata.len();
        let Ok(relative_path) = path.strip_prefix(root) else {
            summary.failed_entries += 1;
            continue;
        };
        let quarantine_path = quarantine_root.join(relative_path);
        if let Some(parent) = quarantine_path.parent() {
            if fs::create_dir_all(parent).is_err() {
                summary.failed_entries += 1;
                continue;
            }
        }

        match snapshot::move_file_across_volumes(&path, &quarantine_path) {
            Ok(()) => {
                summary.quarantined_files += 1;
                summary.quarantined_bytes = summary.quarantined_bytes.saturating_add(len);
                summary.entries.push(SnapshotEntry::QuarantinedPath {
                    original_path: path,
                    quarantine_path,
                    bytes: len,
                });
            }
            Err(_) => summary.failed_entries += 1,
        }
    }
}

fn path_stays_inside(root: &Path, path: &Path) -> bool {
    let Some(root) = canonicalize_existing(root) else {
        return false;
    };
    let candidate = canonicalize_existing(path).unwrap_or_else(|| path.to_path_buf());
    candidate.starts_with(root)
}

fn canonicalize_existing(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

fn is_old_enough(metadata: &fs::Metadata, min_age: Duration) -> bool {
    let modified = metadata
        .modified()
        .or_else(|_| metadata.created())
        .unwrap_or(SystemTime::now());
    modified
        .elapsed()
        .map(|age| age >= min_age)
        .unwrap_or(false)
}
