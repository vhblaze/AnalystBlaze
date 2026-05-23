use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub api_base_url: String,
    pub web_login_url: String,
    pub web_account_url: String,
    pub app_version: String,
    pub normal_sample_interval: Duration,
    pub batch_flush_interval: Duration,
    pub command_poll_interval: Duration,
    pub realtime_status_poll_interval: Duration,
    pub realtime_push_interval: Duration,
    pub dashboard_sample_interval: Duration,
    pub post_optimization_measurement_delay: Duration,
    pub policy_refresh_interval: Duration,
}

impl AgentConfig {
    pub fn from_env() -> Self {
        let api_base_url = std::env::var("ANALYSTBLAZE_API_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
        let web_base_url = std::env::var("ANALYSTBLAZE_WEB_URL")
            .unwrap_or_else(|_| "http://localhost:3000".to_string());
        let web_login_url = build_web_login_url(&web_base_url);
        let web_account_url = build_web_account_url(&web_base_url);

        Self {
            api_base_url,
            web_login_url,
            web_account_url,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            normal_sample_interval: Duration::from_secs(60),
            batch_flush_interval: Duration::from_secs(60 * 60),
            command_poll_interval: Duration::from_secs(30),
            realtime_status_poll_interval: Duration::from_secs(5),
            realtime_push_interval: Duration::from_secs(1),
            dashboard_sample_interval: Duration::from_secs(2),
            post_optimization_measurement_delay: Duration::from_secs(2),
            policy_refresh_interval: Duration::from_secs(15 * 60),
        }
    }
}

fn build_web_account_url(web_base_url: &str) -> String {
    format!("{}/configuration", web_base_url.trim_end_matches('/'))
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
    use super::{build_web_account_url, build_web_login_url};

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
}
