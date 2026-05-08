use serde_json::{json, Value};

use super::ExecutionResult;

pub async fn set_high_performance(payload: Option<Value>) -> ExecutionResult {
    ExecutionResult::ok(
        "Modulo de plano de energia preparado.",
        json!({
            "payload": payload,
            "implemented": false,
            "target_plan": "SCHEME_MIN",
            "next_step": "Invocar PowerSetActiveScheme ou powercfg com rollback controlado.",
        }),
    )
}
