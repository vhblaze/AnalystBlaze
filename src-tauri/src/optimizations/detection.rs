use serde_json::{json, Value};

use super::ExecutionResult;

pub async fn detect_foreground_game(payload: Option<Value>) -> ExecutionResult {
    ExecutionResult::ok(
        "Modulo de deteccao de foreground preparado.",
        json!({
            "payload": payload,
            "implemented": false,
            "examples": ["csgo.exe", "cs2.exe", "valorant.exe"],
        }),
    )
}
