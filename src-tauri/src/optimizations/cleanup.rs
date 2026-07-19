use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};

pub async fn empty_temp(payload: Option<Value>) -> ExecutionResult {
    let mode = payload
        .as_ref()
        .and_then(|value| value.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("safe")
        .to_string();
    let default_minutes = if mode == "deep_confirmed" { 5 } else { 60 };
    let min_age_minutes = payload
        .as_ref()
        .and_then(|value| value.get("min_age_minutes"))
        .and_then(Value::as_u64)
        .or_else(|| {
            payload
                .as_ref()
                .and_then(|value| value.get("min_age_hours"))
                .and_then(Value::as_u64)
                .map(|hours| hours.saturating_mul(60))
        })
        .unwrap_or(default_minutes)
        .max(default_minutes);
    let include_windows_temp = payload
        .as_ref()
        .and_then(|value| value.get("include_windows_temp"))
        .and_then(Value::as_bool)
        .unwrap_or(mode == "deep_confirmed");
    let min_age = Duration::from_secs(min_age_minutes * 60);
    let snapshot_id = snapshot::new_snapshot_id();

    match tokio::task::spawn_blocking(move || {
        quarantine_temp_files(
            min_age,
            min_age_minutes,
            &mode,
            include_windows_temp,
            &snapshot_id,
        )
    })
    .await
    {
        Ok(summary) => cleanup_result(summary),
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao executar limpeza TEMP: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn purge_cleanup_quarantine(payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(move || purge_cleanup_quarantine_blocking(payload)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao limpar quarentena: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

fn purge_cleanup_quarantine_blocking(payload: Option<Value>) -> ExecutionResult {
    if !purge_confirmed(payload.as_ref()) {
        return ExecutionResult {
            success: false,
            message: "Purge da quarentena exige confirmacao explicita do usuario.".to_string(),
            details: json!({
                "implemented": true,
                "confirmed": false,
                "reversible": false,
            }),
        };
    }

    let ignored_payload_path = payload
        .as_ref()
        .and_then(payload_quarantine_root)
        .map(ToString::to_string);
    let root = snapshot::cleanup_quarantine_root();
    let validation = validate_purge_quarantine_root(&root);
    let before_bytes = validation
        .as_ref()
        .map(|_| safe_dir_size(&root))
        .unwrap_or_default();
    let existed = fs::symlink_metadata(&root).is_ok();
    let result = validation.and_then(|_| remove_quarantine_tree(&root));

    match result {
        Ok(()) => {
            let purged_snapshots = snapshot::mark_cleanup_snapshots_purged()
                .map(|value| value as u64)
                .unwrap_or_default();
            ExecutionResult::ok(
                if existed {
                    "Quarentena de limpeza apagada permanentemente."
                } else {
                    "Nenhuma quarentena de limpeza encontrada."
                },
                json!({
                    "implemented": true,
                    "quarantine_root": root,
                    "ignored_payload_quarantine_root": ignored_payload_path,
                    "existed": existed,
                    "bytes_freed": before_bytes,
                    "purged_snapshots": purged_snapshots,
                    "reversible": false,
                }),
            )
        }
        Err(error) => ExecutionResult {
            success: false,
            message: "Nao foi possivel apagar a quarentena de limpeza.".to_string(),
            details: json!({
                "implemented": true,
                "quarantine_root": root,
                "ignored_payload_quarantine_root": ignored_payload_path,
                "bytes_pending": before_bytes,
                "error": error,
            }),
        },
    }
}

#[derive(Debug, Default)]
struct CleanupSummary {
    snapshot_id: String,
    targets: Vec<String>,
    scanned_files: usize,
    scanned_dirs: usize,
    skipped_recent_files: usize,
    skipped_special_entries: usize,
    quarantined_files: usize,
    removed_empty_dirs: usize,
    failed_entries: usize,
    quarantined_bytes: u64,
    entries: Vec<SnapshotEntry>,
    mode: String,
    min_age_minutes: u64,
}

fn quarantine_temp_files(
    min_age: Duration,
    min_age_minutes: u64,
    mode: &str,
    include_windows_temp: bool,
    snapshot_id: &str,
) -> CleanupSummary {
    let quarantine_root = snapshot::cleanup_quarantine_dir(snapshot_id);
    let targets = temp_targets(include_windows_temp);
    let mut summary = CleanupSummary {
        snapshot_id: snapshot_id.to_string(),
        targets: targets
            .iter()
            .map(|target| target.display().to_string())
            .collect(),
        mode: mode.to_string(),
        min_age_minutes,
        ..CleanupSummary::default()
    };

    for (index, target) in targets.iter().enumerate() {
        let target_quarantine_root = quarantine_root.join(format!("target-{index}"));
        quarantine_dir(
            target,
            target,
            &target_quarantine_root,
            min_age,
            &mut summary,
        );
    }

    summary
}

fn cleanup_result(summary: CleanupSummary) -> ExecutionResult {
    let min_age_minutes = summary.min_age_minutes;
    let mode = summary.mode.clone();
    let targets = summary.targets.clone();

    if summary.entries.is_empty() {
        return ExecutionResult::ok(
            if summary.scanned_files == 0 {
                "Nenhum arquivo temporario encontrado nos alvos acessiveis."
            } else {
                "Nenhum arquivo temporario elegivel; arquivos recentes ou em uso permaneceram."
            },
            json!({
                "implemented": true,
                "targets": targets,
                "scanned_files": summary.scanned_files,
                "scanned_dirs": summary.scanned_dirs,
                "skipped_recent_files": summary.skipped_recent_files,
                "skipped_special_entries": summary.skipped_special_entries,
                "quarantined_files": 0,
                "removed_empty_dirs": 0,
                "failed_entries": summary.failed_entries,
                "quarantined_bytes": 0,
                "min_age_minutes": min_age_minutes,
                "mode": mode,
                "snapshot": null,
                "note": "Arquivos recentes, travados por aplicativos ou sem permissao nao sao removidos pelo modo seguro.",
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
            "targets": targets.clone(),
            "scanned_files": summary.scanned_files,
            "scanned_dirs": summary.scanned_dirs,
            "skipped_recent_files": summary.skipped_recent_files,
            "skipped_special_entries": summary.skipped_special_entries,
            "quarantined_files": summary.quarantined_files,
            "removed_empty_dirs": summary.removed_empty_dirs,
            "failed_entries": summary.failed_entries,
            "quarantined_bytes": summary.quarantined_bytes,
            "min_age_minutes": min_age_minutes,
            "mode": mode.clone(),
        }),
    };

    match snapshot::save_snapshot(&snapshot) {
        Ok(()) => ExecutionResult::ok(
            "Arquivos temporarios elegiveis movidos para quarentena reversivel.",
            json!({
                "implemented": true,
                "targets": targets,
                "scanned_files": summary.scanned_files,
                "scanned_dirs": summary.scanned_dirs,
                "skipped_recent_files": summary.skipped_recent_files,
                "skipped_special_entries": summary.skipped_special_entries,
                "quarantined_files": summary.quarantined_files,
                "removed_empty_dirs": summary.removed_empty_dirs,
                "failed_entries": summary.failed_entries,
                "quarantined_bytes": summary.quarantined_bytes,
                "min_age_minutes": min_age_minutes,
                "mode": mode,
                "snapshot": {
                    "id": snapshot.id,
                    "entries": snapshot.entries.len(),
                    "reversible": true,
                    "space_reclaim_pending": true,
                },
                "note": "Itens que continuam na TEMP geralmente sao recentes, estao em uso ou exigem permissao elevada.",
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
                    "targets": targets,
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

fn temp_targets(include_windows_temp: bool) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();

    let mut push_target = |path: PathBuf| {
        if !path.is_dir() {
            return;
        }
        let key = fs::canonicalize(&path)
            .unwrap_or(path.clone())
            .to_string_lossy()
            .to_ascii_lowercase();
        if seen.insert(key) {
            targets.push(path);
        }
    };

    push_target(std::env::temp_dir());
    for key in ["TEMP", "TMP"] {
        if let Some(path) = std::env::var_os(key).map(PathBuf::from) {
            push_target(path);
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        push_target(local_app_data.join("Temp"));
    }
    if include_windows_temp {
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(windows_dir) = std::env::var_os(key).map(PathBuf::from) {
                push_target(windows_dir.join("Temp"));
            }
        }
    }

    targets
}

fn purge_confirmed(payload: Option<&Value>) -> bool {
    let Some(payload) = payload else {
        return false;
    };
    let confirmed = payload
        .get("user_confirmed_purge")
        .or_else(|| payload.get("userConfirmedPurge"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let confirmation_matches = payload
        .get("confirmation")
        .or_else(|| payload.get("confirm"))
        .and_then(Value::as_str)
        .is_some_and(|value| value == "purge_cleanup_quarantine");

    confirmed && confirmation_matches
}

fn payload_quarantine_root(payload: &Value) -> Option<&str> {
    payload
        .get("quarantine_root")
        .or_else(|| payload.get("quarantineRoot"))
        .and_then(Value::as_str)
}

fn validate_purge_quarantine_root(root: &Path) -> Result<(), String> {
    let allowed_root = snapshot::cleanup_quarantine_root();
    if normalize_path_for_policy(root) != normalize_path_for_policy(&allowed_root) {
        return Err("Purge bloqueado: caminho de quarentena inesperado.".to_string());
    }
    if path_components_have_parent(root) {
        return Err("Purge bloqueado: caminho de quarentena contem '..'.".to_string());
    }
    if !path_is_under_policy_root(root, &snapshot::app_data_dir()) {
        return Err("Purge bloqueado: quarentena fora do diretorio permitido.".to_string());
    }
    if is_critical_purge_path(root) {
        return Err("Purge bloqueado: caminho aponta para pasta critica.".to_string());
    }

    ensure_existing_path_has_no_links(root)?;

    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink()
        || metadata_has_reparse_point(&metadata)
        || !metadata.file_type().is_dir()
    {
        return Err("Purge bloqueado: quarentena e link, junction ou nao-diretorio.".to_string());
    }

    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let canonical_app_data =
        fs::canonicalize(snapshot::app_data_dir()).map_err(|error| error.to_string())?;
    if !canonical_root.starts_with(&canonical_app_data) {
        return Err(
            "Purge bloqueado: quarentena canonica fora do diretorio permitido.".to_string(),
        );
    }
    if is_critical_purge_path(&canonical_root) {
        return Err("Purge bloqueado: caminho canonico aponta para pasta critica.".to_string());
    }

    validate_quarantine_tree(root, &canonical_root)
}

fn validate_quarantine_tree(path: &Path, canonical_root: &Path) -> Result<(), String> {
    let entries = fs::read_dir(path).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        validate_quarantine_child_path(&path, canonical_root)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || metadata_has_reparse_point(&metadata) {
            return Err("Purge bloqueado: quarentena contem link ou junction.".to_string());
        }
        if is_critical_purge_path(&path) {
            return Err(
                "Purge bloqueado: item da quarentena aponta para pasta critica.".to_string(),
            );
        }
        if metadata.file_type().is_dir() {
            validate_quarantine_tree(&path, canonical_root)?;
        } else if !metadata.file_type().is_file() {
            return Err("Purge bloqueado: quarentena contem item especial.".to_string());
        }
    }
    Ok(())
}

fn remove_quarantine_tree(root: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink()
        || metadata_has_reparse_point(&metadata)
        || !metadata.file_type().is_dir()
    {
        return Err("Purge bloqueado: quarentena e link, junction ou nao-diretorio.".to_string());
    }

    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    remove_quarantine_contents(root, &canonical_root)?;
    fs::remove_dir(root)
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| error.to_string())
}

fn remove_quarantine_contents(path: &Path, canonical_root: &Path) -> Result<(), String> {
    let entries = fs::read_dir(path).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        validate_quarantine_child_path(&path, canonical_root)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || metadata_has_reparse_point(&metadata) {
            return Err("Purge bloqueado: quarentena contem link ou junction.".to_string());
        }
        if metadata.file_type().is_dir() {
            remove_quarantine_contents(&path, canonical_root)?;
            fs::remove_dir(&path).map_err(|error| error.to_string())?;
        } else if metadata.file_type().is_file() {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
        } else {
            return Err("Purge bloqueado: quarentena contem item especial.".to_string());
        }
    }
    Ok(())
}

fn validate_quarantine_child_path(path: &Path, canonical_root: &Path) -> Result<(), String> {
    if path_components_have_parent(path) {
        return Err("Purge bloqueado: item da quarentena contem '..'.".to_string());
    }
    let canonical_path = fs::canonicalize(path).map_err(|error| error.to_string())?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(
            "Purge bloqueado: item da quarentena escapa do diretorio permitido.".to_string(),
        );
    }
    Ok(())
}

fn safe_dir_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_symlink() || metadata_has_reparse_point(&metadata) {
        return 0;
    }
    if metadata.file_type().is_file() {
        return metadata.len();
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| safe_dir_size(&entry.path()))
        .fold(0_u64, u64::saturating_add)
}

fn ensure_existing_path_has_no_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        if metadata.file_type().is_symlink() || metadata_has_reparse_point(&metadata) {
            return Err("Purge bloqueado: caminho contem symlink ou junction.".to_string());
        }
    }
    Ok(())
}

fn metadata_has_reparse_point(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn path_components_have_parent(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}

fn path_is_under_policy_root(path: &Path, root: &Path) -> bool {
    let path = normalize_path_for_policy(path);
    let root = normalize_path_for_policy(root);
    path == root || path.starts_with(&format!("{root}\\"))
}

fn normalize_path_for_policy(path: &Path) -> String {
    let mut value = path
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_string();
    }
    value
}

fn is_critical_purge_path(path: &Path) -> bool {
    let normalized = normalize_path_for_policy(path);
    if normalized.len() <= 3 && normalized.ends_with(':') || normalized.ends_with(":\\") {
        return true;
    }
    if normalized == normalize_path_for_policy(&snapshot::app_data_dir()) {
        return true;
    }

    ["USERPROFILE", "LOCALAPPDATA", "APPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .map(|path| normalize_path_for_policy(&path))
        .any(|critical| normalized == critical)
        || [
            "SystemRoot",
            "WINDIR",
            "PROGRAMFILES",
            "PROGRAMFILES(X86)",
            "ProgramW6432",
            "PROGRAMDATA",
        ]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .map(|path| normalize_path_for_policy(&path))
        .any(|critical| {
            !critical.is_empty()
                && (normalized == critical || normalized.starts_with(&format!("{critical}\\")))
        })
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
            summary.skipped_special_entries += 1;
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            summary.failed_entries += 1;
            continue;
        };

        if metadata.file_type().is_symlink() {
            summary.skipped_special_entries += 1;
            continue;
        }

        if metadata.is_dir() {
            summary.scanned_dirs += 1;
            quarantine_dir(root, &path, quarantine_root, min_age, summary);
            if is_old_enough(&metadata, min_age) {
                if let Ok(()) = fs::remove_dir(&path) {
                    summary.removed_empty_dirs += 1;
                    summary.entries.push(SnapshotEntry::RemovedEmptyDir {
                        original_path: path,
                    });
                }
            }
            continue;
        }

        if !metadata.is_file() {
            summary.skipped_special_entries += 1;
            continue;
        }

        summary.scanned_files += 1;
        if !is_old_enough(&metadata, min_age) {
            summary.skipped_recent_files += 1;
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn purge_requires_explicit_confirmation_payload() {
        let result = purge_cleanup_quarantine_blocking(None);

        assert!(!result.success);
        assert_eq!(result.details["confirmed"], false);
    }

    #[test]
    fn cleanup_quarantine_root_is_forced_to_agent_quarantine() {
        let root = snapshot::cleanup_quarantine_root();

        assert_eq!(
            root.file_name().and_then(|value| value.to_str()),
            Some("quarantine")
        );
        assert!(path_is_under_policy_root(&root, &snapshot::app_data_dir()));
    }

    #[test]
    fn purge_confirmation_ignores_caller_path_authority() {
        let payload = json!({
            "user_confirmed_purge": true,
            "confirmation": "purge_cleanup_quarantine",
            "quarantine_root": "C:\\Windows",
        });

        assert!(purge_confirmed(Some(&payload)));
        assert_eq!(payload_quarantine_root(&payload), Some("C:\\Windows"));
        assert_eq!(
            snapshot::cleanup_quarantine_root()
                .file_name()
                .and_then(|value| value.to_str()),
            Some("quarantine")
        );
    }

    #[test]
    fn purge_validation_rejects_parent_dir_components() {
        let root_with_parent = snapshot::cleanup_quarantine_root().join("..");
        let error = validate_purge_quarantine_root(&root_with_parent)
            .expect_err("parent traversal should be rejected");

        assert!(error.contains("inesperado") || error.contains(".."));
    }
}
