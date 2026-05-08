use serde_json::{json, Value};

use super::ExecutionResult;

pub async fn enter_focus_mode(payload: Option<Value>) -> ExecutionResult {
    ExecutionResult::ok(
        "Modulo foco/jogo preparado.",
        json!({
            "payload": payload,
            "implemented": false,
            "policy": "Suspender apenas servicos permitidos pelo backend e reverter ao sair do fullscreen.",
        }),
    )
}
