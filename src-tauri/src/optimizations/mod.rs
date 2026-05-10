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
        "APPLY_GAME_MODE" => apply_game_mode(payload).await,
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

async fn apply_game_mode(payload: Option<Value>) -> ExecutionResult {
    let power = energy::set_high_performance(payload.clone()).await;
    let cleanup = cleanup::empty_temp(payload.clone()).await;
    let focus = focus::enter_focus_mode(payload.clone()).await;
    let foreground = detection::detect_foreground_game(payload).await;

    let success = power.success || cleanup.success || focus.success || foreground.success;
    ExecutionResult {
        success,
        message: if success {
            "Modo gamer aplicado com otimizacoes seguras locais.".to_string()
        } else {
            "Modo gamer nao conseguiu aplicar otimizacoes locais.".to_string()
        },
        details: json!({
            "profile": "game_mode",
            "manual": true,
            "steps": {
                "power": {
                    "success": power.success,
                    "message": power.message,
                    "details": power.details,
                },
                "cleanup": {
                    "success": cleanup.success,
                    "message": cleanup.message,
                    "details": cleanup.details,
                },
                "focus": {
                    "success": focus.success,
                    "message": focus.message,
                    "details": focus.details,
                },
                "foreground": {
                    "success": foreground.success,
                    "message": foreground.message,
                    "details": foreground.details,
                },
            },
            "pro_agent_note": "Planos Pro poderao aplicar ajustes adaptativos automaticamente por orquestracao.",
        }),
    }
}
