//! Limites da quarentena: até 5 GB e até 7 dias.
//!
//! A quarentena guarda o que a limpeza e o Disco tiram, uma pasta por
//! snapshot (`quarantine/<snapshot_id>/`). Este módulo apaga sozinho, de
//! tempos em tempos:
//! - tudo o que está lá há mais de `MAX_AGE_DAYS`;
//! - depois, se ainda passar de `MAX_BYTES`, os mais antigos até caber.
//! Cada pasta apagada marca o snapshot como "sem volta" e entra no
//! Histórico pelo evento `quarantine.auto_pruned`. Nunca abre janela.

use serde_json::json;
use std::fs;
use std::path::Path;

use super::snapshot;
use crate::audit;

pub const MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub const MAX_AGE_DAYS: i64 = 7;

#[derive(Debug, Clone, PartialEq)]
pub struct QuarantineDir {
    pub snapshot_id: String,
    /// Unix (s) de quando o item entrou na quarentena.
    pub created_at: i64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PruneDecision {
    pub snapshot_id: String,
    pub bytes: u64,
    /// "age" (passou de 7 dias) ou "size" (precisou caber em 5 GB).
    pub reason: &'static str,
}

/// Decide o que apagar. Puro: só olha idade e tamanho.
pub fn plan_prune(dirs: &[QuarantineDir], now: i64, max_bytes: u64, max_age_days: i64) -> Vec<PruneDecision> {
    let cutoff = now - max_age_days * 24 * 60 * 60;
    let mut decisions = Vec::new();
    let mut kept: Vec<&QuarantineDir> = Vec::new();
    for dir in dirs {
        if dir.created_at < cutoff {
            decisions.push(PruneDecision { snapshot_id: dir.snapshot_id.clone(), bytes: dir.bytes, reason: "age" });
        } else {
            kept.push(dir);
        }
    }
    let mut total: u64 = kept.iter().map(|dir| dir.bytes).sum();
    kept.sort_by_key(|dir| dir.created_at);
    for dir in kept {
        if total <= max_bytes {
            break;
        }
        total = total.saturating_sub(dir.bytes);
        decisions.push(PruneDecision { snapshot_id: dir.snapshot_id.clone(), bytes: dir.bytes, reason: "size" });
    }
    decisions
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else { return 0 };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => dir_size(&path),
                Ok(meta) => meta.len(),
                Err(_) => 0,
            }
        })
        .sum()
}

fn dir_created_at(path: &Path) -> Option<i64> {
    let meta = fs::metadata(path).ok()?;
    let time = meta.created().or_else(|_| meta.modified()).ok()?;
    let secs = time.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

/// Lê as pastas da quarentena com a data do snapshot (ou da pasta, sem snapshot).
pub fn list_quarantine_dirs() -> Vec<QuarantineDir> {
    let root = snapshot::cleanup_quarantine_root();
    let Ok(entries) = fs::read_dir(&root) else { return Vec::new() };
    let snapshots = snapshot::list_snapshots(1000).unwrap_or_default();
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let snapshot_id = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            let created_at = snapshots
                .iter()
                .find(|snapshot| snapshot.id == snapshot_id)
                .map(|snapshot| snapshot.created_at)
                .or_else(|| dir_created_at(&path))?;
            Some(QuarantineDir { snapshot_id, created_at, bytes: dir_size(&path) })
        })
        .collect()
}

/// Aplica os limites agora. Devolve quantas pastas saíram e quantos bytes.
pub fn prune_quarantine() -> (usize, u64) {
    let dirs = list_quarantine_dirs();
    let now = chrono::Utc::now().timestamp();
    let decisions = plan_prune(&dirs, now, MAX_BYTES, MAX_AGE_DAYS);
    if decisions.is_empty() {
        return (0, 0);
    }
    let root = snapshot::cleanup_quarantine_root();
    let mut removed = 0usize;
    let mut freed = 0u64;
    let mut by_age = 0usize;
    for decision in &decisions {
        let path = root.join(&decision.snapshot_id);
        // Nunca sai da raiz da quarentena.
        if !path.starts_with(&root) || !path.is_dir() {
            continue;
        }
        if fs::remove_dir_all(&path).is_ok() {
            removed += 1;
            freed += decision.bytes;
            if decision.reason == "age" {
                by_age += 1;
            }
            let _ = snapshot::mark_snapshot_purged(&decision.snapshot_id);
        }
    }
    if removed > 0 {
        let _ = audit::record_event(
            "info",
            "quarantine.auto_pruned",
            "Quarentena: itens antigos apagados para respeitar o limite de 5 GB e 7 dias.",
            json!({
                "removed": removed,
                "freed_bytes": freed,
                "by_age": by_age,
                "by_size": removed - by_age,
                "max_bytes": MAX_BYTES,
                "max_age_days": MAX_AGE_DAYS,
            }),
        );
    }
    (removed, freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60;
    const GB: u64 = 1024 * 1024 * 1024;

    fn dir(id: &str, age_days: i64, bytes: u64, now: i64) -> QuarantineDir {
        QuarantineDir { snapshot_id: id.to_string(), created_at: now - age_days * DAY, bytes }
    }

    #[test]
    fn older_than_seven_days_goes_regardless_of_size() {
        let now = 1_800_000_000;
        let dirs = vec![dir("old", 8, 10, now), dir("fresh", 1, 10, now)];
        let plan = plan_prune(&dirs, now, MAX_BYTES, MAX_AGE_DAYS);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].snapshot_id, "old");
        assert_eq!(plan[0].reason, "age");
    }

    #[test]
    fn over_five_gb_drops_the_oldest_until_it_fits() {
        let now = 1_800_000_000;
        let dirs = vec![dir("a", 3, 2 * GB, now), dir("b", 2, 2 * GB, now), dir("c", 1, 2 * GB, now)];
        let plan = plan_prune(&dirs, now, MAX_BYTES, MAX_AGE_DAYS);
        assert_eq!(plan.len(), 1, "{plan:?}");
        assert_eq!(plan[0].snapshot_id, "a");
        assert_eq!(plan[0].reason, "size");
    }

    #[test]
    fn under_the_limits_nothing_is_touched() {
        let now = 1_800_000_000;
        let dirs = vec![dir("a", 6, GB, now), dir("b", 0, GB, now)];
        assert!(plan_prune(&dirs, now, MAX_BYTES, MAX_AGE_DAYS).is_empty());
    }

    #[test]
    fn age_is_applied_before_size() {
        let now = 1_800_000_000;
        // "old" sai por idade e já libera espaço: os outros ficam.
        let dirs = vec![dir("old", 9, 3 * GB, now), dir("b", 2, 2 * GB, now), dir("c", 1, 2 * GB, now)];
        let plan = plan_prune(&dirs, now, MAX_BYTES, MAX_AGE_DAYS);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].reason, "age");
    }
}
