//! Limites da quarentena: até 5 GB e até 7 dias.
//!
//! A quarentena guarda o que a limpeza e o Disco tiram, uma pasta por
//! snapshot (`quarantine/<snapshot_id>/`). Este módulo apaga, arquivo por
//! arquivo:
//! - tudo o que está lá há mais de `MAX_AGE_DAYS` (pela data da pasta);
//! - depois, se o total ainda passar de `MAX_BYTES`, os arquivos que
//!   chegaram antes, um a um, até caber. Uma limpeza com milhares de arquivos
//!   pequenos não vira um bloco: só o que passa de 5 GB sai.
//! "Chegou antes" = pasta mais antiga primeiro; dentro da mesma pasta, o
//! arquivo de data de modificação mais antiga primeiro (o Windows não guarda
//! a ordem exata em que a limpeza moveu os arquivos).
//! Roda logo depois de cada ação que enche a quarentena e a cada 30 min.
//! Cada snapshot afetado é marcado: "sem volta" se a pasta saiu inteira,
//! "parcial" se sobraram arquivos (o que sobrou ainda volta). Nunca abre janela.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::snapshot::{self, QuarantinePurge};
use crate::audit;

pub const MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub const MAX_AGE_DAYS: i64 = 7;

/// Só uma poda por vez: a de depois da limpeza e a do loop não se atropelam.
static PRUNE_LOCK: Mutex<()> = Mutex::new(());

/// Um arquivo da quarentena, só com o que o plano precisa.
#[derive(Debug, Clone, PartialEq)]
pub struct QuarantineFile {
    pub snapshot_id: String,
    pub bytes: u64,
    /// Unix (s) de quando a pasta do snapshot nasceu.
    pub snapshot_at: i64,
    /// Unix (s) da modificação do arquivo (desempate dentro da pasta).
    pub modified_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Age,
    Size,
}

/// Decide quais arquivos apagar (índices de `files`). Puro.
pub fn plan_prune(files: &[QuarantineFile], now: i64, max_bytes: u64, max_age_days: i64) -> Vec<(usize, Reason)> {
    let cutoff = now - max_age_days * 24 * 60 * 60;
    let mut decisions = Vec::new();
    let mut kept: Vec<usize> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if file.snapshot_at < cutoff {
            decisions.push((index, Reason::Age));
        } else {
            kept.push(index);
        }
    }
    let mut total: u64 = kept.iter().map(|&index| files[index].bytes).sum();
    if total > max_bytes {
        kept.sort_by_key(|&index| (files[index].snapshot_at, files[index].modified_at, index));
        for index in kept {
            if total <= max_bytes {
                break;
            }
            total = total.saturating_sub(files[index].bytes);
            decisions.push((index, Reason::Size));
        }
    }
    decisions
}

fn unix_secs(time: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    let secs = time.ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

/// Percorre a quarentena e devolve cada arquivo com o caminho. Não segue links.
fn scan(root: &Path) -> (Vec<QuarantineFile>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(root) else { return (files, paths) };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(meta) = fs::symlink_metadata(&dir) else { continue };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        let snapshot_id = entry.file_name().to_string_lossy().to_string();
        let snapshot_at = unix_secs(meta.created()).or_else(|| unix_secs(meta.modified())).unwrap_or(0);
        walk(&dir, &snapshot_id, snapshot_at, &mut files, &mut paths);
    }
    (files, paths)
}

fn walk(dir: &Path, snapshot_id: &str, snapshot_at: i64, files: &mut Vec<QuarantineFile>, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(&path, snapshot_id, snapshot_at, files, paths);
        } else if meta.is_file() {
            files.push(QuarantineFile {
                snapshot_id: snapshot_id.to_string(),
                bytes: meta.len(),
                snapshot_at,
                modified_at: unix_secs(meta.modified()).unwrap_or(0),
            });
            paths.push(path);
        }
    }
}

/// Apaga pastas vazias de baixo para cima (só entram as que já ficaram vazias).
fn remove_empty_dirs(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink()) {
                remove_empty_dirs(&entry.path());
            }
        }
    }
    let _ = fs::remove_dir(dir);
}

fn remove_file(path: &Path) -> bool {
    if fs::remove_file(path).is_ok() {
        return true;
    }
    // Arquivo somente leitura: tira o atributo e tenta de novo.
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_readonly(false);
        let _ = fs::set_permissions(path, perms);
    }
    fs::remove_file(path).is_ok()
}

/// Resultado de uma poda.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PruneReport {
    pub files_removed: usize,
    pub bytes_freed: u64,
    pub snapshots_whole: usize,
    pub snapshots_partial: usize,
}

/// Aplica os limites agora, na quarentena do app.
pub fn prune_quarantine() -> PruneReport {
    let Ok(_guard) = PRUNE_LOCK.lock() else { return PruneReport::default() };
    prune_root(&snapshot::cleanup_quarantine_root(), chrono::Utc::now().timestamp(), MAX_BYTES, MAX_AGE_DAYS, true)
}

/// A poda em si, numa pasta qualquer (os testes usam uma pasta temporária e
/// `record = false`, para não mexer no arquivo de snapshots nem no log reais).
fn prune_root(root: &Path, now: i64, max_bytes: u64, max_age_days: i64, record: bool) -> PruneReport {
    let root = root.to_path_buf();
    let (files, paths) = scan(&root);
    let decisions = plan_prune(&files, now, max_bytes, max_age_days);
    if decisions.is_empty() {
        return PruneReport::default();
    }

    let mut report = PruneReport::default();
    let mut by_age = 0usize;
    let mut touched: Vec<String> = Vec::new();
    for (index, reason) in &decisions {
        let path = &paths[*index];
        // Nunca sai da raiz da quarentena.
        if !path.starts_with(&root) {
            continue;
        }
        if remove_file(path) {
            report.files_removed += 1;
            report.bytes_freed += files[*index].bytes;
            if *reason == Reason::Age {
                by_age += 1;
            }
            let id = &files[*index].snapshot_id;
            if !touched.contains(id) {
                touched.push(id.clone());
            }
        }
    }

    let mut marks: Vec<(String, QuarantinePurge)> = Vec::new();
    for id in touched {
        let dir = root.join(&id);
        remove_empty_dirs(&dir);
        let kind = if dir.exists() { QuarantinePurge::Partial } else { QuarantinePurge::Whole };
        match kind {
            QuarantinePurge::Whole => report.snapshots_whole += 1,
            QuarantinePurge::Partial => report.snapshots_partial += 1,
        }
        marks.push((id, kind));
    }
    if record {
        let _ = snapshot::mark_snapshots_pruned(&marks);
    }

    if record && report.files_removed > 0 {
        let _ = audit::record_event(
            "info",
            "quarantine.auto_pruned",
            "Quarentena: arquivos mais antigos apagados para respeitar o limite de 5 GB e 7 dias.",
            json!({
                "removed": report.files_removed,
                "freed_bytes": report.bytes_freed,
                "by_age": by_age,
                "by_size": report.files_removed - by_age,
                "snapshots_whole": report.snapshots_whole,
                "snapshots_partial": report.snapshots_partial,
                "max_bytes": max_bytes,
                "max_age_days": max_age_days,
            }),
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60;
    const GB: u64 = 1024 * 1024 * 1024;
    const NOW: i64 = 1_800_000_000;

    fn file(snapshot: &str, age_days: i64, bytes: u64, modified_days: i64) -> QuarantineFile {
        QuarantineFile {
            snapshot_id: snapshot.to_string(),
            bytes,
            snapshot_at: NOW - age_days * DAY,
            modified_at: NOW - modified_days * DAY,
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ab-quarantine-test-{name}-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_files(dir: &Path, count: usize, size: usize) {
        fs::create_dir_all(dir).unwrap();
        for n in 0..count {
            fs::write(dir.join(format!("f{n:03}.bin")), vec![0u8; size]).unwrap();
        }
    }

    fn dir_total(dir: &Path) -> u64 {
        let (files, _) = scan(dir);
        files.iter().map(|f| f.bytes).sum()
    }

    #[test]
    fn real_files_one_big_snapshot_is_trimmed_to_the_limit_and_the_folder_stays() {
        let root = temp_root("trim");
        make_files(&root.join("snapA").join("target-0").join("deep"), 40, 1000); // 40 KB em arquivos pequenos
        let now = chrono::Utc::now().timestamp();
        let report = prune_root(&root, now, 10_000, 7, false);
        assert_eq!(dir_total(&root), 10_000, "{report:?}");
        assert_eq!(report.files_removed, 30);
        assert_eq!(report.snapshots_partial, 1);
        assert_eq!(report.snapshots_whole, 0);
        assert!(root.join("snapA").exists(), "o que sobrou continua lá");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn real_files_fully_emptied_snapshot_folder_is_removed() {
        let root = temp_root("whole");
        make_files(&root.join("old").join("target-0"), 3, 1000);
        make_files(&root.join("new").join("target-0"), 3, 1000);
        // Um limite de 3 KB deixa só 3 arquivos; os de "old" e "new" têm a mesma data aqui,
        // então forçamos "old" como mais velho pelo tempo de modificação.
        let old_time = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400);
        for entry in fs::read_dir(root.join("old").join("target-0")).unwrap().flatten() {
            let file = fs::File::options().write(true).open(entry.path()).unwrap();
            file.set_modified(old_time).unwrap();
        }
        let now = chrono::Utc::now().timestamp();
        let report = prune_root(&root, now, 3_000, 7, false);
        assert_eq!(dir_total(&root), 3_000, "{report:?}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn real_files_under_the_limit_nothing_is_deleted() {
        let root = temp_root("under");
        make_files(&root.join("snapA"), 5, 1000);
        let report = prune_root(&root, chrono::Utc::now().timestamp(), 10_000, 7, false);
        assert_eq!(report, PruneReport::default());
        assert_eq!(dir_total(&root), 5_000);
        fs::remove_dir_all(&root).unwrap();
    }

    fn removed(plan: &[(usize, Reason)]) -> Vec<usize> {
        let mut indices: Vec<usize> = plan.iter().map(|(index, _)| *index).collect();
        indices.sort_unstable();
        indices
    }

    #[test]
    fn older_than_seven_days_goes_regardless_of_size() {
        let files = vec![file("old", 8, 10, 8), file("fresh", 1, 10, 1)];
        let plan = plan_prune(&files, NOW, MAX_BYTES, MAX_AGE_DAYS);
        assert_eq!(plan, vec![(0, Reason::Age)]);
    }

    #[test]
    fn thousands_of_small_files_in_one_snapshot_are_trimmed_not_dropped_whole() {
        // O caso do dono: uma limpeza, 32 GB em arquivos pequenos de 1 MB.
        let files: Vec<QuarantineFile> = (0..32 * 1024).map(|n| file("one-cleanup", 0, 1024 * 1024, 1000 - (n as i64 % 1000))).collect();
        let plan = plan_prune(&files, NOW, MAX_BYTES, MAX_AGE_DAYS);
        let freed: u64 = plan.iter().map(|(index, _)| files[*index].bytes).sum();
        let left: u64 = files.iter().map(|f| f.bytes).sum::<u64>() - freed;
        assert!(left <= MAX_BYTES, "sobraram {left} bytes");
        assert!(left > MAX_BYTES - 2 * 1024 * 1024, "apagou demais: sobraram só {left} bytes");
        assert!(plan.iter().all(|(_, reason)| *reason == Reason::Size));
    }

    #[test]
    fn oldest_snapshot_goes_first_then_oldest_file_inside() {
        let files = vec![
            file("newer", 1, 2 * GB, 1), // 0
            file("older", 3, 2 * GB, 3), // 1
            file("older", 3, 2 * GB, 5), // 2: mesma pasta, arquivo mais antigo
            file("newer", 1, 2 * GB, 1), // 3
        ];
        // Total 8 GB, teto 5 GB: precisa sair 3 GB => 2 arquivos de 2 GB.
        let plan = plan_prune(&files, NOW, MAX_BYTES, MAX_AGE_DAYS);
        assert_eq!(removed(&plan), vec![1, 2], "{plan:?}");
    }

    #[test]
    fn under_the_limits_nothing_is_touched() {
        let files = vec![file("a", 6, GB, 6), file("b", 0, GB, 0)];
        assert!(plan_prune(&files, NOW, MAX_BYTES, MAX_AGE_DAYS).is_empty());
    }

    #[test]
    fn age_is_applied_before_size() {
        // "old" sai por idade e já libera espaço: os outros ficam.
        let files = vec![file("old", 9, 3 * GB, 9), file("b", 2, 2 * GB, 2), file("c", 1, 2 * GB, 1)];
        let plan = plan_prune(&files, NOW, MAX_BYTES, MAX_AGE_DAYS);
        assert_eq!(plan, vec![(0, Reason::Age)]);
    }
}
