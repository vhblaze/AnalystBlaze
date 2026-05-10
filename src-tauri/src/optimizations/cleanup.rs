use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::ExecutionResult;

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

    match tokio::task::spawn_blocking(move || clean_temp_files(min_age)).await {
        Ok(summary) => ExecutionResult::ok(
            "Arquivos temporarios antigos limpos com politica segura.",
            json!({
                "implemented": true,
                "targets": ["TEMP"],
                "deleted_files": summary.deleted_files,
                "deleted_dirs": summary.deleted_dirs,
                "failed_entries": summary.failed_entries,
                "freed_bytes": summary.freed_bytes,
                "min_age_hours": min_age.as_secs() / 3600,
            }),
        ),
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao executar limpeza TEMP: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

#[derive(Debug, Default)]
struct CleanupSummary {
    deleted_files: usize,
    deleted_dirs: usize,
    failed_entries: usize,
    freed_bytes: u64,
}

fn clean_temp_files(min_age: Duration) -> CleanupSummary {
    let temp_dir = std::env::temp_dir();
    let mut summary = CleanupSummary::default();
    clean_dir(&temp_dir, &temp_dir, min_age, &mut summary);
    summary
}

fn clean_dir(root: &Path, dir: &Path, min_age: Duration, summary: &mut CleanupSummary) {
    let Ok(entries) = fs::read_dir(dir) else {
        summary.failed_entries += 1;
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path_stays_inside(root, &path) {
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
            clean_dir(root, &path, min_age, summary);
            if is_old_enough(&metadata, min_age) {
                match fs::remove_dir(&path) {
                    Ok(()) => summary.deleted_dirs += 1,
                    Err(_) => {}
                }
            }
            continue;
        }

        if !metadata.is_file() || !is_old_enough(&metadata, min_age) {
            continue;
        }

        let len = metadata.len();
        match fs::remove_file(&path) {
            Ok(()) => {
                summary.deleted_files += 1;
                summary.freed_bytes = summary.freed_bytes.saturating_add(len);
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
