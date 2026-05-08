use serde_json::{json, Value};

use super::ExecutionResult;

pub async fn set_process_priority(payload: Option<Value>) -> ExecutionResult {
    ExecutionResult::ok(
        "Modulo de prioridade de processos preparado para Win32.",
        json!({
            "payload": payload,
            "implemented": false,
            "requires_admin": false,
            "next_step": "Abrir processo via OpenProcess e aplicar SetPriorityClass.",
        }),
    )
}
