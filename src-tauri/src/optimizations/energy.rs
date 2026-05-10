use serde_json::{json, Value};
use std::process::Command;

use super::ExecutionResult;

pub async fn set_high_performance(payload: Option<Value>) -> ExecutionResult {
    let result = tokio::task::spawn_blocking(|| {
        Command::new("powercfg")
            .args(["/setactive", "SCHEME_MIN"])
            .output()
    })
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => ExecutionResult::ok(
            "Plano de energia de alto desempenho ativado.",
            json!({
                "payload": payload,
                "implemented": true,
                "target_plan": "SCHEME_MIN",
                "requires_admin": false,
            }),
        ),
        Ok(Ok(output)) => ExecutionResult {
            success: false,
            message: "Nao foi possivel ativar o plano de alto desempenho.".to_string(),
            details: json!({
                "payload": payload,
                "implemented": true,
                "target_plan": "SCHEME_MIN",
                "status": output.status.code(),
                "stderr": String::from_utf8_lossy(&output.stderr).trim(),
            }),
        },
        Ok(Err(error)) => ExecutionResult {
            success: false,
            message: format!("Falha ao chamar powercfg: {error}"),
            details: json!({
                "payload": payload,
                "implemented": true,
                "target_plan": "SCHEME_MIN",
            }),
        },
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao executar powercfg: {error}"),
            details: json!({
                "payload": payload,
                "implemented": true,
                "target_plan": "SCHEME_MIN",
            }),
        },
    }
}
