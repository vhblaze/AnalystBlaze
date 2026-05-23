mod hmac;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{profile_from_value, AuthTokens};
use crate::telemetry::collector::HardwareProfile;
use crate::telemetry::engine::{
    AgentOptimizationEventPayload, RealtimeTelemetryPayload, TelemetryBatch,
};

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPolicyEnvelope {
    pub bundle: AgentPolicyBundle,
    pub signature: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPolicyBundle {
    pub user_id: Uuid,
    pub hw_id: Uuid,
    pub plan: String,
    pub model_version: String,
    pub policy_version: String,
    pub issued_at: String,
    pub expires_at: String,
    pub permissions: AgentPolicyPermissions,
    pub allowed_actions: Vec<String>,
    pub thresholds: AgentPolicyThresholds,
    pub cooldowns: AgentPolicyCooldowns,
    pub user_weights: AgentPolicyWeights,
    pub server_authority: bool,
    pub notes: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPolicyPermissions {
    pub manual_gamer_mode: bool,
    pub automatic_agent_optimization: bool,
    pub local_inference: bool,
    pub energy_optimization: bool,
    pub process_optimization: bool,
    pub weekly_ai_telemetry_seconds: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPolicyThresholds {
    pub high_cpu: f64,
    pub high_gpu: f64,
    pub high_ram_percent: f64,
    pub high_cpu_temp: f64,
    pub high_gpu_temp: f64,
    pub idle_seconds: u64,
    pub min_confidence: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPolicyCooldowns {
    pub local_decision_seconds: u64,
    pub game_mode_seconds: u64,
    pub cleanup_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPolicyWeights {
    pub gaming_priority: f64,
    pub energy_saving_priority: f64,
    pub silence_notifications_priority: f64,
    pub thermal_protection_priority: f64,
    pub background_cleanup_priority: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandListResponse {
    pending: Vec<CommandResponse>,
    weekly_ai_usage: Option<WeeklyAiTelemetryUsage>,
    automatic_agent_available: Option<bool>,
    blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyAiTelemetryUsage {
    used_seconds: i64,
    limit_seconds: Option<i64>,
    remaining_seconds: Option<i64>,
    is_currently_tracking: bool,
    limit_reached: bool,
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

    pub async fn exchange_desktop_pairing_code(&self, code: &str) -> Result<AuthTokens, String> {
        let code = code.trim();
        if code.is_empty() {
            return Err("Codigo de pareamento vazio.".to_string());
        }

        let response = self
            .http
            .post(self.url("/api/v1/auth/desktop-pairing/exchange"))
            .json(&json!({ "code": code }))
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let payload = ok_json::<Value>(response).await?;
        let access_token = payload
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| "Pareamento nao retornou token de acesso.".to_string())?
            .to_string();
        let refresh_token = payload
            .get("refresh_token")
            .and_then(Value::as_str)
            .filter(|token| !token.trim().is_empty())
            .map(ToString::to_string);
        let response_profile = profile_from_value(&payload);
        let user_profile = payload
            .get("user")
            .map(profile_from_value)
            .unwrap_or_default();

        Ok(AuthTokens {
            access_token,
            refresh_token,
            profile: user_profile.merge(response_profile),
        })
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

    pub async fn post_agent_event(
        &self,
        access_token: &str,
        hw_secret: &str,
        payload: &AgentOptimizationEventPayload,
    ) -> Result<(), String> {
        let payload = serde_json::to_value(payload).map_err(|error| error.to_string())?;
        self.post_signed::<Value>(
            access_token,
            hw_secret,
            "/api/v1/telemetry/agent-events",
            &payload,
        )
        .await?;
        Ok(())
    }

    pub async fn agent_policy(
        &self,
        access_token: &str,
        hw_id: Uuid,
        hw_secret: &str,
    ) -> Result<AgentPolicyBundle, String> {
        let response = self
            .http
            .get(self.url("/api/v1/telemetry/agent-policy"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let envelope = ok_json::<AgentPolicyEnvelope>(response).await?;
        if envelope.bundle.hw_id != hw_id {
            return Err("Policy bundle recebido para outro hardware.".to_string());
        }

        let bundle_payload =
            serde_json::to_value(&envelope.bundle).map_err(|error| error.to_string())?;
        let expected_signature = hmac::sign_json(&bundle_payload, hw_secret)?;
        if !constant_time_eq(expected_signature.as_bytes(), envelope.signature.as_bytes()) {
            return Err("Assinatura do policy bundle invalida.".to_string());
        }

        Ok(envelope.bundle)
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
        if response.automatic_agent_available == Some(false) {
            let reason = response
                .blocked_reason
                .as_deref()
                .unwrap_or("AUTOMATIC_AGENT_UNAVAILABLE");
            eprintln!("Agente automatico indisponivel pelo servidor: {reason}");
        }
        if let Some(usage) = response.weekly_ai_usage.as_ref() {
            if usage.limit_reached {
                eprintln!(
                    "Limite semanal de IA do Starter atingido: {}/{}s usados",
                    usage.used_seconds,
                    usage.limit_seconds.unwrap_or_default()
                );
            } else if usage.is_currently_tracking {
                if let Some(remaining) = usage.remaining_seconds {
                    eprintln!("IA automatica Starter em uso: {remaining}s restantes nesta semana");
                }
            }
        }

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

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    left.iter()
        .zip(right.iter())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}
