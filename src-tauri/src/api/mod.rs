mod hmac;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::telemetry::collector::HardwareProfile;
use crate::telemetry::engine::{RealtimeTelemetryPayload, TelemetryBatch};

#[derive(Debug, Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HardwareRegistration {
    pub id: Uuid,
    pub status: String,
    pub hw_secret: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RealtimeStatus {
    pub active: bool,
    pub ttl_seconds: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandResponse {
    pub id: Uuid,
    pub hw_id: Uuid,
    pub action_name: String,
    pub action_payload: Option<Value>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandListResponse {
    pending: Vec<CommandResponse>,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn register_hardware(
        &self,
        access_token: &str,
        profile: &HardwareProfile,
    ) -> Result<HardwareRegistration, String> {
        let response = self
            .http
            .post(self.url("/api/v1/hardware/register"))
            .bearer_auth(access_token)
            .json(profile)
            .send()
            .await
            .map_err(|error| error.to_string())?;

        ok_json::<HardwareRegistration>(response).await
    }

    pub async fn account_profile(&self, access_token: &str) -> Result<Option<Value>, String> {
        for path in [
            "/api/v1/auth/me",
            "/api/v1/me",
            "/api/v1/account/me",
            "/api/v1/users/me",
        ] {
            let response = self
                .http
                .get(self.url(path))
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();

            if matches!(
                status,
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ) {
                continue;
            }

            if !status.is_success() {
                continue;
            }

            if let Ok(profile) = response.json::<Value>().await {
                return Ok(Some(profile));
            }
        }

        Ok(None)
    }

    pub async fn post_batch(
        &self,
        access_token: &str,
        hw_secret: &str,
        batch: &TelemetryBatch,
    ) -> Result<(), String> {
        let payload = serde_json::to_value(batch).map_err(|error| error.to_string())?;
        self.post_signed::<Value>(access_token, hw_secret, "/api/v1/telemetry/batch", &payload)
            .await?;
        Ok(())
    }

    pub async fn push_realtime(
        &self,
        access_token: &str,
        hw_secret: &str,
        payload: &RealtimeTelemetryPayload,
    ) -> Result<RealtimeStatus, String> {
        let payload = serde_json::to_value(payload).map_err(|error| error.to_string())?;
        self.post_signed(
            access_token,
            hw_secret,
            "/api/v1/telemetry/realtime/push",
            &payload,
        )
        .await
    }

    pub async fn realtime_status(
        &self,
        access_token: &str,
        hw_id: Uuid,
    ) -> Result<RealtimeStatus, String> {
        let response = self
            .http
            .get(self.url("/api/v1/telemetry/realtime/status"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .send()
            .await
            .map_err(|error| error.to_string())?;

        ok_json::<RealtimeStatus>(response).await
    }

    pub async fn next_commands(
        &self,
        access_token: &str,
        hw_id: Uuid,
    ) -> Result<Vec<CommandResponse>, String> {
        let response = self
            .http
            .get(self.url("/api/v1/telemetry/commands/next"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let response = ok_json::<CommandListResponse>(response).await?;

        Ok(response.pending)
    }

    pub async fn acknowledge_command(
        &self,
        access_token: &str,
        command_id: Uuid,
        success: bool,
        details: Value,
    ) -> Result<(), String> {
        let response = self
            .http
            .post(self.url(&format!("/api/v1/telemetry/commands/{command_id}/ack")))
            .bearer_auth(access_token)
            .json(&json!({
                "success": success,
                "details": details,
            }))
            .send()
            .await
            .map_err(|error| error.to_string())?;

        ok_empty(response).await
    }

    async fn post_signed<T: for<'de> Deserialize<'de>>(
        &self,
        access_token: &str,
        hw_secret: &str,
        path: &str,
        payload: &Value,
    ) -> Result<T, String> {
        let signature = hmac::sign_json(payload, hw_secret)?;
        let response = self
            .http
            .post(self.url(path))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Signature", signature)
            .json(payload)
            .send()
            .await
            .map_err(|error| error.to_string())?;

        ok_json::<T>(response).await
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

async fn ok_json<T: for<'de> Deserialize<'de>>(response: reqwest::Response) -> Result<T, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(error_body(status, response).await);
    }
    response
        .json::<T>()
        .await
        .map_err(|error| error.to_string())
}

async fn ok_empty(response: reqwest::Response) -> Result<(), String> {
    let status = response.status();
    if !status.is_success() {
        return Err(error_body(status, response).await);
    }
    Ok(())
}

async fn error_body(status: StatusCode, response: reqwest::Response) -> String {
    let text = response.text().await.unwrap_or_default();
    if text.trim().is_empty() {
        format!("API retornou status {status}")
    } else if let Ok(value) = serde_json::from_str::<Value>(&text) {
        value
            .get("detail")
            .and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
            .map(|message| message.to_string())
            .unwrap_or_else(|| format!("API retornou status {status}: {text}"))
    } else {
        format!("API retornou status {status}: {text}")
    }
}
