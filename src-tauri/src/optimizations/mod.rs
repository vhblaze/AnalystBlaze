pub mod cleanup;
pub mod detection;
pub mod energy;
pub mod focus;
pub mod latency;
pub mod memory;
pub mod processes;

use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub message: String,
    pub details: Value,
}

impl ExecutionResult {
    fn ok(message: impl Into<String>, details: Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            details,
        }
    }

    fn unsupported(action_name: &str) -> Self {
        Self {
            success: false,
            message: format!("Comando ainda nao implementado no agente: {action_name}"),
            details: json!({ "action_name": action_name }),
        }
    }
}

pub async fn execute_command(action_name: &str, payload: Option<Value>) -> ExecutionResult {
    match action_name {
        "SET_PROCESS_PRIORITY" => processes::set_process_priority(payload).await,
        "EMPTY_TEMP" => cleanup::empty_temp(payload).await,
        "CLEAR_STANDBY_LIST" => memory::clear_standby_list(payload).await,
        "SET_POWER_PLAN_HIGH_PERFORMANCE" => energy::set_high_performance(payload).await,
        "APPLY_LATENCY_TWEAKS" => latency::apply_latency_tweaks(payload).await,
        "ENTER_FOCUS_MODE" => focus::enter_focus_mode(payload).await,
        "DETECT_FOREGROUND_GAME" => detection::detect_foreground_game(payload).await,
        other => ExecutionResult::unsupported(other),
    }
}
