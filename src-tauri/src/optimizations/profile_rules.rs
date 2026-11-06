//! Regras automáticas do perfil (etapa 3).
//!
//! Pega a lista de "pesa sem você usar" (`idle_weight`) e aplica, sozinho,
//! a mesma ação reversível que o botão em Insights aplicaria, mas só quando
//! é seguro:
//! - `profile_rules_enabled` ligado (nasce ligado) e a automação geral ligada;
//! - PC ocioso há `MIN_IDLE_SECS` (sem teclado nem mouse);
//! - nenhum jogo aberto;
//! - serviço crítico nunca (`safety::is_critical_service`);
//! - no máximo `MAX_PER_DAY` itens por dia;
//! - cada item só uma vez a cada `RETRY_DAYS` dias (se o usuário desfez,
//!   não insiste).
//! Tudo passa por `execute_command_checked` com `CommandSource::LocalPolicy`:
//! snapshot, Histórico ("automático") e Desfazer, como qualquer ação.
//! Nunca abre janela.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::sync::Mutex;

use super::idle_weight::{self, IdleWeightItem};
use super::safety::{self, CommandSource};
use super::{adaptive, detection, local_ai_policy, service_usage, snapshot};
use crate::audit;

pub const MIN_IDLE_SECS: u64 = 10 * 60;
pub const MAX_PER_DAY: usize = 2;
pub const RETRY_DAYS: i64 = 14;
pub const LOOP_SECS: u64 = 30 * 60;

static FILE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppliedRule {
    pub key: String,
    pub action: String,
    pub applied_at: i64,
    pub success: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RulesState {
    pub applied: Vec<AppliedRule>,
}

fn state_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join("profile-rules.json")
}

fn read_state() -> RulesState {
    fs::read_to_string(state_path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn write_state(state: &RulesState) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(state) {
        let _ = fs::write(path, text);
    }
}

pub fn item_key(item: &IdleWeightItem) -> String {
    format!("{}:{}", item.kind, item.name.to_lowercase())
}

/// Os freios, em um lugar só. Puro. Devolve o motivo de não agir, ou None.
pub fn blocker(enabled: bool, idle_secs: u64, game_open: bool, applied_today: usize) -> Option<&'static str> {
    if !enabled {
        return Some("disabled");
    }
    if game_open {
        return Some("game_open");
    }
    if idle_secs < MIN_IDLE_SECS {
        return Some("not_idle");
    }
    if applied_today >= MAX_PER_DAY {
        return Some("daily_limit");
    }
    None
}

/// Quais itens aplicar agora. Puro: respeita o limite diário, a repetição
/// e nunca escolhe serviço crítico.
pub fn plan<'a>(items: &'a [IdleWeightItem], state: &RulesState, now: i64, applied_today: usize) -> Vec<&'a IdleWeightItem> {
    let budget = MAX_PER_DAY.saturating_sub(applied_today);
    items
        .iter()
        .filter(|item| {
            let key = item_key(item);
            !state
                .applied
                .iter()
                .any(|entry| entry.key == key && now - entry.applied_at < RETRY_DAYS * 24 * 60 * 60)
        })
        .filter(|item| item.kind != "service" || !safety::is_critical_service(&item.name))
        .take(budget)
        .collect()
}

fn applied_today(state: &RulesState, now: i64) -> usize {
    let day_start = chrono::Local::now().date_naive().and_hms_opt(0, 0, 0).map(|d| d.and_utc().timestamp()).unwrap_or(now - 86_400);
    state.applied.iter().filter(|entry| entry.success && entry.applied_at >= day_start).count()
}

/// Uma passada: decide e aplica. Chamada pelo loop em lib.rs.
pub async fn run_once() {
    let policy = local_ai_policy::load_local_ai_policy();
    let enabled = policy.enabled && policy.profile_rules_enabled;
    let now = chrono::Utc::now().timestamp();
    let state = {
        let _guard = FILE_LOCK.lock();
        read_state()
    };
    let today = applied_today(&state, now);
    let game_open = detection::detect_game_process().detected;
    if blocker(enabled, adaptive::idle_seconds(), game_open, today).is_some() {
        return;
    }
    let Ok(items) = tokio::task::spawn_blocking(idle_weight::candidates).await else { return };
    let chosen: Vec<IdleWeightItem> = plan(&items, &state, now, today).into_iter().cloned().collect();
    if chosen.is_empty() {
        return;
    }

    let mut state = state;
    for item in chosen {
        // Freio conferido de novo logo antes de cada ação: o usuário pode ter voltado.
        if blocker(true, adaptive::idle_seconds(), detection::detect_game_process().detected, 0).is_some() {
            break;
        }
        let (action, payload) = match item.action.as_str() {
            "delay_startup" => ("DELAY_STARTUP_APP", json!({ "name": item.name, "location": item.location, "delaySeconds": 120 })),
            "stop_service" => ("STOP_SERVICE", json!({ "service_name": item.name })),
            _ => continue,
        };
        let result = super::execute_command_checked(CommandSource::LocalPolicy, action, Some(payload), None, true).await;
        if result.success && action == "STOP_SERVICE" {
            service_usage::mark_paused_by_analystblaze(&item.name);
        }
        let _ = audit::record_event(
            if result.success { "info" } else { "warn" },
            "profile_rule.applied",
            if result.success {
                "Regra do perfil: item sem uso tratado no horário ocioso."
            } else {
                "Regra do perfil: não deu para aplicar."
            },
            json!({
                "key": item_key(&item),
                "kind": item.kind,
                "display_name": item.display_name,
                "action": action,
                "memory_mb": item.memory_mb,
                "cpu_percent": item.cpu_percent,
                "last_foreground_days_ago": item.last_foreground_days_ago,
                "success": result.success,
                "message": result.message,
            }),
        );
        state.applied.push(AppliedRule { key: item_key(&item), action: action.to_string(), applied_at: now, success: result.success });
    }
    state.applied.retain(|entry| now - entry.applied_at < 90 * 24 * 60 * 60);
    let _guard = FILE_LOCK.lock();
    write_state(&state);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: &str, name: &str, action: &str) -> IdleWeightItem {
        IdleWeightItem {
            kind: kind.to_string(),
            name: name.to_string(),
            display_name: name.to_string(),
            location: None,
            memory_mb: 300.0,
            cpu_percent: 0.0,
            last_foreground_days_ago: None,
            days_tracked: 10,
            action: action.to_string(),
        }
    }

    #[test]
    fn brakes_in_order() {
        assert_eq!(blocker(false, 9999, false, 0), Some("disabled"));
        assert_eq!(blocker(true, 9999, true, 0), Some("game_open"));
        assert_eq!(blocker(true, 599, false, 0), Some("not_idle"));
        assert_eq!(blocker(true, 600, false, MAX_PER_DAY), Some("daily_limit"));
        assert_eq!(blocker(true, 600, false, 0), None);
    }

    #[test]
    fn at_most_two_per_day() {
        let items = vec![item("startup", "A", "delay_startup"), item("startup", "B", "delay_startup"), item("startup", "C", "delay_startup")];
        assert_eq!(plan(&items, &RulesState::default(), 1_800_000_000, 0).len(), 2);
        assert_eq!(plan(&items, &RulesState::default(), 1_800_000_000, 1).len(), 1);
    }

    #[test]
    fn does_not_insist_on_an_item_for_fourteen_days() {
        let items = vec![item("startup", "A", "delay_startup")];
        let now = 1_800_000_000;
        let state = RulesState { applied: vec![AppliedRule { key: "startup:a".into(), action: "DELAY_STARTUP_APP".into(), applied_at: now - 5 * 86_400, success: true }] };
        assert!(plan(&items, &state, now, 0).is_empty());
        let old = RulesState { applied: vec![AppliedRule { key: "startup:a".into(), action: "DELAY_STARTUP_APP".into(), applied_at: now - 20 * 86_400, success: true }] };
        assert_eq!(plan(&items, &old, now, 0).len(), 1);
    }

    #[test]
    fn critical_service_is_never_chosen() {
        let items = vec![item("service", "WinDefend", "stop_service"), item("service", "bthserv", "stop_service")];
        let chosen = plan(&items, &RulesState::default(), 1_800_000_000, 0);
        assert!(chosen.iter().all(|item| item.name != "WinDefend"), "{chosen:?}");
    }
}
