use std::time::Duration;
use url::Url;

const DEV_API_BASE_URL: &str = "http://127.0.0.1:8000";
// TEMPORARY: api.analystblaze.com's DNS points at a stuck/unactivated Railway
// custom domain (verified in DNS but Railway's edge still rejects the host
// header - see incident notes). Using Railway's own default domain directly
// unblocks login/telemetry/everything now; revert to the custom domain once
// Railway confirms it's actually routing.
const PROD_API_BASE_URL: &str = "https://analystblaze-server-production.up.railway.app";
const DEV_WEB_BASE_URL: &str = "http://localhost:3000";
const PROD_WEB_BASE_URL: &str = "https://analystblaze.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeEnvironment {
    Development,
    Production,
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub api_base_url: String,
    pub web_login_url: String,
    pub web_account_url: String,
    pub web_billing_url: String,
    pub web_insights_url: String,
    pub app_version: String,
    pub normal_sample_interval: Duration,
    pub batch_flush_interval: Duration,
    pub command_poll_interval: Duration,
    pub realtime_status_poll_interval: Duration,
    pub realtime_push_interval: Duration,
    pub dashboard_sample_interval: Duration,
    pub post_optimization_measurement_delay: Duration,
    pub policy_refresh_interval: Duration,
    pub telemetry_diagnostics_enabled: bool,
    pub telemetry_include_ssid: bool,
    pub telemetry_include_hostname: bool,
    pub telemetry_family_detail_consent: bool,
}

impl AgentConfig {
    pub fn from_env() -> Self {
        let environment = runtime_environment();
        let api_base_url = std::env::var("ANALYSTBLAZE_API_URL")
            .unwrap_or_else(|_| default_api_base_url(environment).to_string());
        let api_base_url =
            validate_endpoint_url("ANALYSTBLAZE_API_URL", &api_base_url, environment)
                .unwrap_or_else(|error| panic!("{error}"));
        let web_base_url = std::env::var("ANALYSTBLAZE_WEB_URL")
            .unwrap_or_else(|_| default_web_base_url(environment).to_string());
        let web_base_url =
            validate_endpoint_url("ANALYSTBLAZE_WEB_URL", &web_base_url, environment)
                .unwrap_or_else(|error| panic!("{error}"));
        let web_login_url = build_web_login_url(&web_base_url);
        let web_account_url = build_web_account_url(&web_base_url);
        let web_billing_url = build_web_billing_url(&web_base_url);
        let web_insights_url = build_web_insights_url(&web_base_url);

        Self {
            api_base_url,
            web_login_url,
            web_account_url,
            web_billing_url,
            web_insights_url,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            normal_sample_interval: Duration::from_secs(60),
            batch_flush_interval: Duration::from_secs(60 * 60),
            command_poll_interval: Duration::from_secs(30),
            realtime_status_poll_interval: Duration::from_secs(5),
            realtime_push_interval: Duration::from_secs(1),
            dashboard_sample_interval: Duration::from_secs(2),
            post_optimization_measurement_delay: Duration::from_secs(2),
            policy_refresh_interval: Duration::from_secs(15 * 60),
            telemetry_diagnostics_enabled: env_flag("ANALYSTBLAZE_DIAGNOSTIC_TELEMETRY"),
            telemetry_include_ssid: env_flag("ANALYSTBLAZE_TELEMETRY_INCLUDE_SSID"),
            telemetry_include_hostname: env_flag("ANALYSTBLAZE_TELEMETRY_INCLUDE_HOSTNAME"),
            telemetry_family_detail_consent: env_flag("ANALYSTBLAZE_FAMILY_DETAIL_CONSENT"),
        }
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn runtime_environment() -> RuntimeEnvironment {
    if !cfg!(debug_assertions) {
        return RuntimeEnvironment::Production;
    }

    ["ANALYSTBLAZE_ENV", "APP_ENV", "NODE_ENV"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .and_then(|value| environment_from_str(&value))
        .unwrap_or(RuntimeEnvironment::Development)
}

fn environment_from_str(value: &str) -> Option<RuntimeEnvironment> {
    match value.trim().to_ascii_lowercase().as_str() {
        "production" | "prod" => Some(RuntimeEnvironment::Production),
        "development" | "dev" | "local" | "test" => Some(RuntimeEnvironment::Development),
        _ => None,
    }
}

fn default_api_base_url(environment: RuntimeEnvironment) -> &'static str {
    match environment {
        RuntimeEnvironment::Development => DEV_API_BASE_URL,
        RuntimeEnvironment::Production => PROD_API_BASE_URL,
    }
}

fn default_web_base_url(environment: RuntimeEnvironment) -> &'static str {
    match environment {
        RuntimeEnvironment::Development => DEV_WEB_BASE_URL,
        RuntimeEnvironment::Production => PROD_WEB_BASE_URL,
    }
}

fn validate_endpoint_url(
    name: &str,
    raw_url: &str,
    environment: RuntimeEnvironment,
) -> Result<String, String> {
    let normalized = raw_url.trim().trim_end_matches('/').to_string();
    let url = Url::parse(&normalized)
        .map_err(|error| format!("{name} invalida: {error}. Valor recebido: {raw_url}"))?;

    match url.scheme() {
        "https" => Ok(normalized),
        "http" if environment == RuntimeEnvironment::Development && is_dev_loopback_url(&url) => {
            Ok(normalized)
        }
        "http" if environment == RuntimeEnvironment::Development => Err(format!(
            "{name} insegura: em desenvolvimento, http:// so e permitido para localhost ou 127.0.0.1."
        )),
        "http" => Err(format!(
            "{name} insegura: em production, use apenas https://. HTTP so e aceito para localhost/127.0.0.1 em modo dev."
        )),
        scheme => Err(format!(
            "{name} insegura: esquema '{scheme}' nao permitido. Use https:// em production."
        )),
    }
}

fn is_dev_loopback_url(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1")
}

fn build_web_account_url(web_base_url: &str) -> String {
    format!("{}/configuration", web_base_url.trim_end_matches('/'))
}

fn build_web_billing_url(web_base_url: &str) -> String {
    format!("{}/billing", web_base_url.trim_end_matches('/'))
}

fn build_web_insights_url(web_base_url: &str) -> String {
    format!("{}/insights", web_base_url.trim_end_matches('/'))
}

fn build_web_login_url(web_base_url: &str) -> String {
    let login_url = format!("{}/login", web_base_url.trim_end_matches('/'));

    match url::Url::parse(&login_url) {
        Ok(mut url) => {
            url.query_pairs_mut()
                .append_pair("desktop", "1")
                .append_pair("redirect_uri", "analystblaze://auth");
            url.to_string()
        }
        Err(_) => {
            let redirect_uri: String =
                url::form_urlencoded::byte_serialize(b"analystblaze://auth").collect();
            format!("{login_url}?desktop=1&redirect_uri={redirect_uri}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_web_account_url, build_web_billing_url, build_web_insights_url, build_web_login_url,
        default_api_base_url, validate_endpoint_url, RuntimeEnvironment,
    };

    #[test]
    fn builds_encoded_desktop_login_url() {
        assert_eq!(
            build_web_login_url("https://app.example.test"),
            "https://app.example.test/login?desktop=1&redirect_uri=analystblaze%3A%2F%2Fauth"
        );
    }

    #[test]
    fn trims_trailing_slash_from_web_url() {
        assert_eq!(
            build_web_login_url("http://localhost:3000/"),
            "http://localhost:3000/login?desktop=1&redirect_uri=analystblaze%3A%2F%2Fauth"
        );
    }

    #[test]
    fn builds_account_settings_url() {
        assert_eq!(
            build_web_account_url("https://app.example.test/"),
            "https://app.example.test/configuration"
        );
    }

    #[test]
    fn builds_billing_url() {
        assert_eq!(
            build_web_billing_url("https://app.example.test/"),
            "https://app.example.test/billing"
        );
    }

    #[test]
    fn builds_web_insights_url() {
        assert_eq!(
            build_web_insights_url("https://app.example.test/"),
            "https://app.example.test/insights"
        );
    }

    #[test]
    fn production_default_api_url_is_https() {
        assert_eq!(
            default_api_base_url(RuntimeEnvironment::Production),
            "https://analystblaze-server-production.up.railway.app"
        );
    }

    #[test]
    fn development_default_api_url_can_use_loopback_http() {
        assert_eq!(
            default_api_base_url(RuntimeEnvironment::Development),
            "http://127.0.0.1:8000"
        );
    }

    #[test]
    fn production_rejects_http_even_for_localhost() {
        let error = validate_endpoint_url(
            "ANALYSTBLAZE_API_URL",
            "http://localhost:8000",
            RuntimeEnvironment::Production,
        )
        .expect_err("production must reject all http urls");

        assert!(error.contains("production"));
    }

    #[test]
    fn production_accepts_https() {
        let value = validate_endpoint_url(
            "ANALYSTBLAZE_API_URL",
            "https://api.example.test/",
            RuntimeEnvironment::Production,
        )
        .expect("production should accept https");

        assert_eq!(value, "https://api.example.test");
    }

    #[test]
    fn development_allows_http_only_for_loopback() {
        assert!(validate_endpoint_url(
            "ANALYSTBLAZE_API_URL",
            "http://localhost:8000",
            RuntimeEnvironment::Development,
        )
        .is_ok());
        assert!(validate_endpoint_url(
            "ANALYSTBLAZE_API_URL",
            "http://127.0.0.1:8000",
            RuntimeEnvironment::Development,
        )
        .is_ok());

        let error = validate_endpoint_url(
            "ANALYSTBLAZE_API_URL",
            "http://api.example.test",
            RuntimeEnvironment::Development,
        )
        .expect_err("development should reject non-loopback http");

        assert!(error.contains("localhost"));
    }
}
