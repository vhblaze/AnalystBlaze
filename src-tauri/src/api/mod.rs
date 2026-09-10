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
    #[serde(default, rename = "requiresConfirmation")]
    pub requires_confirmation: bool,
    #[serde(default, rename = "authorizationMode")]
    pub authorization_mode: Option<String>,
    #[serde(default, rename = "authorizationId")]
    pub authorization_id: Option<String>,
    #[serde(default, rename = "contextKey")]
    pub context_key: Option<String>,
    #[serde(default, rename = "riskLevel")]
    pub risk_level: Option<String>,
    #[serde(default, rename = "confirmationPrompt")]
    pub confirmation_prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommandAcknowledgement {
    pub command_id: Uuid,
    pub success: bool,
    pub details: Value,
    pub confirmed_locally: bool,
    pub authorization_id: Option<String>,
    pub context_key: Option<String>,
    pub execution_mode: Option<String>,
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
    #[serde(default)]
    pub protected_process_names: Vec<String>,
    #[serde(default)]
    pub optimizer_capabilities: AgentOptimizerCapabilities,
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
    #[serde(default)]
    pub foreground_burst_mode: bool,
    #[serde(default)]
    pub background_quiet_mode: bool,
    #[serde(default)]
    pub uplink_pressure_relief: bool,
    #[serde(default)]
    pub adaptive_optimization: bool,
    #[serde(default)]
    pub wifi_latency_guard: bool,
    #[serde(default)]
    pub hybrid_cpu_isolation: bool,
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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentOptimizerCapabilities {
    #[serde(default)]
    pub foreground_burst_mode: bool,
    #[serde(default)]
    pub background_quiet_mode: bool,
    #[serde(default)]
    pub uplink_pressure_relief_stage1: bool,
    #[serde(default)]
    pub adaptive_optimization: bool,
    #[serde(default)]
    pub uplink_pressure_relief_stage2: bool,
    #[serde(default)]
    pub wifi_latency_guard: Option<String>,
    #[serde(default)]
    pub hybrid_cpu_isolation: Option<String>,
    #[serde(default)]
    pub admin_helper_required_for: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandListResponse {
    pending: Vec<CommandResponse>,
    weekly_ai_usage: Option<WeeklyAiTelemetryUsage>,
    automatic_agent_available: Option<bool>,
    blocked_reason: Option<String>,
}

/// The server's weekly automation-command budget for the starter plan
/// (3600s / 60min today - `limit_seconds` is `None` for paid plans, meaning
/// unlimited). Enforced server-side regardless of what the desktop shows;
/// this is purely for surfacing the real, already-applied limit to the user
/// instead of leaving it silent (see api/mod.rs::next_commands).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyAiTelemetryUsage {
    pub used_seconds: i64,
    pub limit_seconds: Option<i64>,
    pub remaining_seconds: Option<i64>,
    pub is_currently_tracking: bool,
    pub limit_reached: bool,
}

/// An admin-authored broadcast message (see app/models/announcement.py on
/// the server), shown in the desktop notification bell. Dismissal is
/// tracked client-side only - see AppShell.tsx.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub tone: String,
    pub is_active: bool,
    pub expires_at: Option<String>,
    pub created_at: String,
}

/// The starter plan's weekly manual-Game-Mode budget for THIS machine (see
/// app/models/weekly_game_mode_usage.py - keyed by hw_id, not user_id, since
/// Game Mode runs on one PC at a time). `limit_seconds` is `None` for paid
/// plans (unlimited, and the desktop never calls these endpoints for them).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameModeUsage {
    pub used_seconds: i64,
    pub limit_seconds: Option<i64>,
    pub remaining_seconds: Option<i64>,
    pub is_currently_tracking: bool,
    pub limit_reached: bool,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            // Without an explicit timeout, a stalled TLS handshake (seen on
            // some Windows 10 machines depending on the local cert store)
            // hangs the underlying request forever instead of failing fast
            // into the resilient fallback paths that call this client.
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn register_hardware(
        &self,
        access_token: &str,
        profile: &HardwareProfile,
        accept_language: Option<&str>,
    ) -> Result<HardwareRegistration, String> {
        let body = serde_json::to_value(profile).map_err(|error| error.to_string())?;
        self.post_hardware_register(access_token, &body, accept_language)
            .await?
            .into_json::<HardwareRegistration>()
            .await
    }

    /// Same registration, but carrying proof that we hold this machine's
    /// current `hw_secret` - which authorizes the server to move the PC off
    /// whichever account it is linked to today and onto this one.
    ///
    /// Deliberately a separate call rather than an automatic retry inside
    /// `register_hardware`: moving a PC between accounts must be an explicit,
    /// confirmed action by whoever is physically at the keyboard. Retrying on
    /// our own would silently hand the PC (and its history) to the next person
    /// who happens to sign in on a shared computer.
    pub async fn register_hardware_with_transfer(
        &self,
        access_token: &str,
        profile: &HardwareProfile,
        accept_language: Option<&str>,
        prev_hw_id: Uuid,
        prev_hw_secret: &str,
    ) -> Result<HardwareRegistration, String> {
        let mut body = serde_json::to_value(profile).map_err(|error| error.to_string())?;
        if let Value::Object(map) = &mut body {
            map.insert(
                "transfer_proof".to_string(),
                build_transfer_proof(prev_hw_id, prev_hw_secret)?,
            );
        }
        self.post_hardware_register(access_token, &body, accept_language)
            .await?
            .into_json::<HardwareRegistration>()
            .await
    }

    async fn post_hardware_register(
        &self,
        access_token: &str,
        body: &Value,
        accept_language: Option<&str>,
    ) -> Result<RawResponse, String> {
        let mut request = self
            .http
            .post(self.url("/api/v1/hardware/register"))
            .bearer_auth(access_token)
            .json(body);
        if let Some(language) = accept_language.filter(|value| !value.trim().is_empty()) {
            request = request.header("Accept-Language", language);
        }
        let response = request.send().await.map_err(|error| error.to_string())?;
        Ok(RawResponse::read(response).await)
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

    pub async fn insights(
        &self,
        access_token: &str,
        hw_id: Uuid,
        accept_language: Option<&str>,
    ) -> Result<Value, String> {
        let mut request = self
            .http
            .get(self.url(&format!("/api/v1/insights?device_id={hw_id}")))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string());
        if let Some(language) = accept_language.filter(|value| !value.trim().is_empty()) {
            request = request.header("Accept-Language", language);
        }

        let response = request.send().await.map_err(|error| error.to_string())?;

        ok_json::<Value>(response).await
    }

    pub async fn post_performance_summary(
        &self,
        access_token: &str,
        hw_id: Uuid,
        summary: &Value,
    ) -> Result<(), String> {
        let response = self
            .http
            .post(self.url("/api/v1/performance/reports/summary"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .json(summary)
            .send()
            .await
            .map_err(|error| error.to_string())?;

        ok_empty(response).await
    }

    /// Ground-truth PresentMon capture stats for one Modo Gamer session -
    /// see app/models/performance.py's FrameCaptureSession docstring for why
    /// this exists as its own endpoint instead of folding into the
    /// performance summary above.
    pub async fn post_frame_capture_session(
        &self,
        access_token: &str,
        hw_id: Uuid,
        session: &Value,
    ) -> Result<(), String> {
        let response = self
            .http
            .post(self.url("/api/v1/performance/frame-capture/sessions"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .json(session)
            .send()
            .await
            .map_err(|error| error.to_string())?;

        ok_empty(response).await
    }

    pub async fn next_commands(
        &self,
        access_token: &str,
        hw_id: Uuid,
    ) -> Result<(Vec<CommandResponse>, Option<WeeklyAiTelemetryUsage>), String> {
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

        Ok((response.pending, response.weekly_ai_usage))
    }

    /// Admin-broadcast messages (see app/api/v1/announcements.py on the
    /// server) - short notices like "overlay changes coming next week",
    /// unrelated to a specific app release. Never mutates anything locally;
    /// the caller decides how/whether to show them.
    pub async fn active_announcements(&self, access_token: &str) -> Result<Vec<Announcement>, String> {
        let response = self
            .http
            .get(self.url("/api/v1/announcements/active"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        ok_json::<Vec<Announcement>>(response).await
    }

    pub async fn weekly_game_mode_usage(
        &self,
        access_token: &str,
        hw_id: Uuid,
    ) -> Result<GameModeUsage, String> {
        let response = self
            .http
            .get(self.url("/api/v1/game-mode-usage/weekly"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        ok_json::<GameModeUsage>(response).await
    }

    pub async fn start_game_mode_usage(
        &self,
        access_token: &str,
        hw_id: Uuid,
    ) -> Result<GameModeUsage, String> {
        let response = self
            .http
            .post(self.url("/api/v1/game-mode-usage/start"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        ok_json::<GameModeUsage>(response).await
    }

    pub async fn checkpoint_game_mode_usage(
        &self,
        access_token: &str,
        hw_id: Uuid,
    ) -> Result<GameModeUsage, String> {
        let response = self
            .http
            .post(self.url("/api/v1/game-mode-usage/checkpoint"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        ok_json::<GameModeUsage>(response).await
    }

    pub async fn stop_game_mode_usage(
        &self,
        access_token: &str,
        hw_id: Uuid,
    ) -> Result<GameModeUsage, String> {
        let response = self
            .http
            .post(self.url("/api/v1/game-mode-usage/stop"))
            .bearer_auth(access_token)
            .header("X-AnalystBlaze-Hardware-Id", hw_id.to_string())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        ok_json::<GameModeUsage>(response).await
    }

    /// Enqueues an insight's recommended action for the agent to apply on
    /// its own (see app/api/v1/dashboard.py::apply_insight_action) - it's
    /// picked up and executed the next time the background command poll
    /// runs, with the same local-confirmation rules as any other
    /// server-issued command until the server's approval-count reaches
    /// auto_allowed. Returns 403 (surfaced as an error string here) for
    /// plans without automatic-agent entitlement.
    pub async fn apply_insight_action(
        &self,
        access_token: &str,
        hw_id: Uuid,
        action_name: &str,
        title: Option<&str>,
        reason: Option<&str>,
    ) -> Result<(), String> {
        let response = self
            .http
            .post(self.url("/api/v1/insights/actions"))
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "deviceId": hw_id.to_string(),
                "actionName": action_name,
                "title": title,
                "reason": reason,
            }))
            .send()
            .await
            .map_err(|error| error.to_string())?;
        ok_empty(response).await
    }

    pub async fn acknowledge_command(
        &self,
        access_token: &str,
        ack: CommandAcknowledgement,
    ) -> Result<(), String> {
        let response = self
            .http
            .post(self.url(&format!(
                "/api/v1/telemetry/commands/{}/ack",
                ack.command_id
            )))
            .bearer_auth(access_token)
            .json(&json!({
                "success": ack.success,
                "details": ack.details,
                "confirmedLocally": ack.confirmed_locally,
                "authorizationId": ack.authorization_id,
                "contextKey": ack.context_key,
                "executionMode": ack.execution_mode,
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
    error_text(status, &text)
}

fn error_text(status: StatusCode, text: &str) -> String {
    if text.trim().is_empty() {
        format!("API retornou status {status}")
    } else if let Ok(value) = serde_json::from_str::<Value>(text) {
        value
            .get("detail")
            .and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
            .map(|message| format!("API retornou status {status}: {message}"))
            .unwrap_or_else(|| format!("API retornou status {status}: {text}"))
    } else {
        format!("API retornou status {status}: {text}")
    }
}

/// A fully-read HTTP response: status + body text, so the caller can inspect
/// the status and body and still decode or turn it into an error afterwards.
/// Used by hardware registration, which needs to peek for a 409
/// `HARDWARE_ALREADY_LINKED` before deciding whether to retry as a device
/// transfer.
struct RawResponse {
    status: StatusCode,
    body: String,
}

impl RawResponse {
    async fn read(response: reqwest::Response) -> Self {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Self { status, body }
    }

    async fn into_json<T: for<'de> Deserialize<'de>>(self) -> Result<T, String> {
        if !self.status.is_success() {
            return Err(self.into_error());
        }
        serde_json::from_str::<T>(&self.body).map_err(|error| error.to_string())
    }

    fn into_error(self) -> String {
        error_text(self.status, &self.body)
    }
}

fn build_transfer_proof(hw_id: Uuid, hw_secret: &str) -> Result<Value, String> {
    let nonce = Uuid::new_v4().simple().to_string();
    let signature = hmac::sign_json(
        &json!({ "nonce": nonce, "prev_hw_id": hw_id.to_string() }),
        hw_secret,
    )?;
    Ok(json!({
        "prev_hw_id": hw_id.to_string(),
        "nonce": nonce,
        "signature": signature,
    }))
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

#[cfg(test)]
mod transfer_proof_tests {
    use super::*;

    // Known-answer computed the way the server does it:
    //   json.dumps({"nonce":..,"prev_hw_id":..}, sort_keys=True, separators=(",",":"))
    //   then HMAC-SHA256 with the device hw_secret, hex.
    // Pins the desktop's canonical form to the server's without importing the
    // hmac crate here (its `Mac` trait collides with the local `hmac` module).
    const KAT_SECRET: &str = "device-hw-secret";
    const KAT_NONCE: &str = "fixednonce123456";
    const KAT_HW_ID: &str = "11111111-2222-3333-4444-555555555555";
    const KAT_SIGNATURE: &str =
        "b70c9ffbe594b676d87c9414ff6b0f1e2564e6d6ffbbf6f840e694e7d5d4db51";

    #[test]
    fn signature_matches_the_server_known_answer() {
        let signed = hmac::sign_json(
            &json!({ "nonce": KAT_NONCE, "prev_hw_id": KAT_HW_ID }),
            KAT_SECRET,
        )
        .unwrap();
        assert_eq!(signed, KAT_SIGNATURE);
    }

    #[test]
    fn build_transfer_proof_signs_exactly_nonce_and_prev_hw_id() {
        let hw_id = Uuid::parse_str(KAT_HW_ID).unwrap();
        let proof = build_transfer_proof(hw_id, KAT_SECRET).expect("proof");

        let nonce = proof["nonce"].as_str().unwrap();
        assert_eq!(proof["prev_hw_id"].as_str().unwrap(), KAT_HW_ID);
        assert!(nonce.len() >= 8 && nonce.len() <= 128);
        let mut keys = proof
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(keys, vec!["nonce", "prev_hw_id", "signature"]);

        let expected = hmac::sign_json(
            &json!({ "nonce": nonce, "prev_hw_id": hw_id.to_string() }),
            KAT_SECRET,
        )
        .unwrap();
        assert_eq!(proof["signature"].as_str().unwrap(), expected);
    }

    #[test]
    fn each_proof_carries_a_fresh_nonce() {
        let hw_id = Uuid::new_v4();
        let a = build_transfer_proof(hw_id, "s").unwrap();
        let b = build_transfer_proof(hw_id, "s").unwrap();
        assert_ne!(a["nonce"], b["nonce"]);
        assert_ne!(a["signature"], b["signature"]);
    }
}
