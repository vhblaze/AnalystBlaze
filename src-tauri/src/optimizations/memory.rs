use serde_json::{json, Value};

use super::ExecutionResult;

pub async fn clear_standby_list(payload: Option<Value>) -> ExecutionResult {
    ExecutionResult::ok(
        "Modulo de limpeza de standby list preparado.",
        json!({
            "payload": payload,
            "implemented": false,
            "requires_admin": true,
            "next_step": "Usar NtSetSystemInformation/SystemMemoryListInformation com checagem de privilegios.",
        }),
    )
}
