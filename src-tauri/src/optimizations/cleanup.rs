use serde_json::{json, Value};

use super::ExecutionResult;

pub async fn empty_temp(payload: Option<Value>) -> ExecutionResult {
    ExecutionResult::ok(
        "Modulo de limpeza TEMP preparado.",
        json!({
            "payload": payload,
            "implemented": false,
            "targets": ["TEMP", "TMP"],
        }),
    )
}
