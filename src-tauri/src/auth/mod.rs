use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const SERVICE: &str = "AnalystBlaze";
const SESSION_USER: &str = "session";
const MAX_DEEP_LINK_BYTES: usize = 2048;
const SINGLE_VALUE_AUTH_PARAMS: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "code",
    "pairing_code",
];
const FORBIDDEN_AUTH_PARAMS: &[&str] = &[
    "cmd", "command", "exec", "shell", "program", "path", "file", "url", "open",
];
type JsonObject = serde_json::Map<String, Value>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredCredentials {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub hw_id: Option<Uuid>,
    pub hw_secret: Option<String>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub plan: Option<String>,
    pub has_paid_plan: Option<bool>,
    /// Unix seconds of the last time `plan`/`has_paid_plan` were confirmed
    /// against the server (not just decoded from a locally-cached JWT).
    /// `None` means it has never been actively confirmed since pairing -
    /// see sync_account_plan in lib.rs.
    pub plan_synced_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub profile: AuthProfile,
}

#[derive(Debug, Clone)]
pub enum AuthCallback {
    Tokens(AuthTokens),
    PairingCode(String),
}

#[derive(Debug, Clone, Default)]
pub struct AuthProfile {
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub plan: Option<String>,
    pub has_paid_plan: Option<bool>,
}

impl AuthProfile {
    pub fn merge(self, fallback: AuthProfile) -> Self {
        let plan = self.plan.or(fallback.plan);
        let has_paid_plan = self
            .has_paid_plan
            .or(fallback.has_paid_plan)
            .or_else(|| plan.as_ref().map(|plan| is_paid_plan(plan)));

        Self {
            user_name: self.user_name.or(fallback.user_name),
            user_email: self.user_email.or(fallback.user_email),
            plan,
            has_paid_plan,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SecureStore;

impl SecureStore {
    pub fn new() -> Result<Self, String> {
        Ok(Self)
    }

    pub fn load(&self) -> Result<StoredCredentials, String> {
        let entry = session_entry()?;
        match entry.get_password() {
            Ok(raw) => serde_json::from_str(&raw).map_err(|error| error.to_string()),
            Err(KeyringError::NoEntry) => Ok(StoredCredentials::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn save(&self, credentials: &StoredCredentials) -> Result<(), String> {
        let entry = session_entry()?;
        let raw = serde_json::to_string(credentials).map_err(|error| error.to_string())?;
        entry.set_password(&raw).map_err(|error| error.to_string())
    }

    pub fn clear(&self) -> Result<(), String> {
        let entry = session_entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(test)]
pub fn tokens_from_deep_link(raw_url: &str) -> Result<AuthTokens, String> {
    match auth_callback_from_deep_link(raw_url)? {
        AuthCallback::Tokens(tokens) => Ok(tokens),
        AuthCallback::PairingCode(_) => {
            Err("Deep link trouxe codigo de pareamento, nao um token JWT.".to_string())
        }
    }
}

pub fn auth_callback_from_deep_link(raw_url: &str) -> Result<AuthCallback, String> {
    if raw_url.len() > MAX_DEEP_LINK_BYTES {
        return Err("Deep link de autenticacao excede o tamanho permitido.".to_string());
    }

    let url = url::Url::parse(raw_url).map_err(|error| error.to_string())?;
    if !is_auth_deep_link(&url) {
        return Err("Deep link invalido para autenticacao do AnalystBlaze.".to_string());
    }

    let params = auth_params(&url);
    validate_auth_params(&params)?;
    let access_token = find_param(&params, "token")
        .or_else(|| find_param(&params, "access_token"))
        .filter(|token| !token.trim().is_empty())
        .map(|token| {
            let refresh_token =
                find_param(&params, "refresh_token").filter(|token| !token.trim().is_empty());
            let profile = auth_profile(&params, &token);

            AuthCallback::Tokens(AuthTokens {
                access_token: token,
                refresh_token,
                profile,
            })
        });

    if let Some(callback) = access_token {
        return Ok(callback);
    }

    let pairing_code = find_param(&params, "code")
        .or_else(|| find_param(&params, "pairing_code"))
        .filter(|code| !code.trim().is_empty())
        .ok_or_else(|| "Deep link nao trouxe token JWT nem codigo de pareamento.".to_string())?;

    Ok(AuthCallback::PairingCode(pairing_code))
}

pub fn profile_from_token(access_token: &str) -> AuthProfile {
    auth_profile(&[], access_token)
}

pub fn profile_from_value(value: &Value) -> AuthProfile {
    profile_from_claims(&[], &Some(value.clone()))
}

pub fn profile_from_credentials(credentials: &StoredCredentials) -> AuthProfile {
    let plan = credentials.plan.as_deref().map(normalize_plan);
    let has_paid_plan = credentials
        .has_paid_plan
        .or_else(|| plan.as_ref().map(|plan| is_paid_plan(plan)));

    AuthProfile {
        user_name: credentials.user_name.as_deref().and_then(clean_user_name),
        user_email: credentials.user_email.as_deref().and_then(non_empty),
        plan,
        has_paid_plan,
    }
}

fn is_auth_deep_link(url: &url::Url) -> bool {
    url.scheme() == "analystblaze"
        && (url.host_str() == Some("auth") || url.path().trim_matches('/') == "auth")
}

fn auth_params(url: &url::Url) -> Vec<(String, String)> {
    let mut params: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    if let Some(fragment) = url.fragment() {
        params.extend(
            url::form_urlencoded::parse(fragment.as_bytes())
                .map(|(key, value)| (key.into_owned(), value.into_owned())),
        );
    }

    params
}

fn validate_auth_params(params: &[(String, String)]) -> Result<(), String> {
    for forbidden in FORBIDDEN_AUTH_PARAMS {
        if params
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case(forbidden))
        {
            return Err("Deep link contem parametro nao permitido para autenticacao.".to_string());
        }
    }

    for single in SINGLE_VALUE_AUTH_PARAMS {
        let count = params
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(single))
            .count();
        if count > 1 {
            return Err("Deep link contem parametros de autenticacao duplicados.".to_string());
        }
    }

    Ok(())
}

fn find_param(params: &[(String, String)], name: &str) -> Option<String> {
    params
        .iter()
        .find_map(|(key, value)| (key == name).then(|| value.clone()))
}

fn auth_profile(params: &[(String, String)], access_token: &str) -> AuthProfile {
    let jwt_claims = jwt_claims(access_token);
    profile_from_claims(params, &jwt_claims)
}

fn profile_from_claims(params: &[(String, String)], claims: &Option<Value>) -> AuthProfile {
    let root_claims = claims.as_ref().and_then(Value::as_object);
    let data_claims = first_object(root_claims, &["data"]);
    let user_claims =
        first_object(root_claims, &["user"]).or_else(|| first_object(data_claims, &["user"]));
    let account_claims =
        first_object(root_claims, &["account"]).or_else(|| first_object(data_claims, &["account"]));
    let profile_claims = first_object(root_claims, &["profile"])
        .or_else(|| first_object(data_claims, &["profile"]))
        .or_else(|| first_object(user_claims, &["profile"]))
        .or_else(|| first_object(account_claims, &["profile"]));
    let user_metadata_claims = first_object(root_claims, &["user_metadata", "raw_user_meta_data"])
        .or_else(|| first_object(data_claims, &["user_metadata", "raw_user_meta_data"]))
        .or_else(|| {
            first_object(
                user_claims,
                &["user_metadata", "raw_user_meta_data", "metadata"],
            )
        });
    let metadata_claims = first_object(root_claims, &["app_metadata", "metadata"])
        .or_else(|| first_object(data_claims, &["metadata"]))
        .or_else(|| first_object(account_claims, &["metadata"]));
    let subscription_claims = first_object(root_claims, &["subscription", "billing"])
        .or_else(|| first_object(data_claims, &["subscription", "billing"]))
        .or_else(|| first_object(account_claims, &["subscription", "billing"]));
    let text_sources = [
        root_claims,
        data_claims,
        user_claims,
        account_claims,
        profile_claims,
        user_metadata_claims,
        metadata_claims,
        subscription_claims,
    ];

    let name_keys = [
        "name",
        "user_name",
        "username",
        "display_name",
        "displayName",
        "full_name",
        "fullName",
        "preferred_username",
        "nickname",
        "first_name",
        "given_name",
    ];
    let user_name = first_name_param(params, &name_keys)
        .or_else(|| first_name_claim(&text_sources, &name_keys))
        .or_else(|| joined_name_from_sources(&text_sources));
    let user_email = first_param(params, &["email", "user_email"])
        .or_else(|| first_claim_from_sources(&text_sources, &["email", "user_email"]));
    let plan = first_param(
        params,
        &[
            "plan",
            "plan_tier",
            "account_plan",
            "subscription_plan",
            "subscription_tier",
            "tier",
        ],
    )
    .or_else(|| {
        first_claim_from_sources(
            &text_sources,
            &[
                "plan",
                "plan_tier",
                "account_plan",
                "subscription_plan",
                "subscription_tier",
                "tier",
            ],
        )
    })
    .map(|plan| normalize_plan(&plan));
    let has_paid_plan = first_bool_param(
        params,
        &[
            "has_paid_plan",
            "paid",
            "is_paid",
            "is_pro",
            "has_pro",
            "active_subscription",
            "subscription_active",
            "is_subscribed",
        ],
    )
    .or_else(|| {
        first_bool_claim_from_sources(
            &text_sources,
            &[
                "has_paid_plan",
                "paid",
                "is_paid",
                "is_pro",
                "has_pro",
                "active_subscription",
                "subscription_active",
                "is_subscribed",
            ],
        )
    })
    .or_else(|| subscription_active(params, &text_sources, subscription_claims))
    .or_else(|| plan.as_ref().map(|plan| is_paid_plan(plan)));

    AuthProfile {
        user_name,
        user_email,
        plan,
        has_paid_plan,
    }
}

fn first_object<'a>(object: Option<&'a JsonObject>, names: &[&str]) -> Option<&'a JsonObject> {
    object.and_then(|object| {
        names
            .iter()
            .find_map(|name| object.get(*name).and_then(Value::as_object))
    })
}

fn first_param(params: &[(String, String)], names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| find_param(params, name))
        .and_then(non_empty)
}

fn first_name_param(params: &[(String, String)], names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| find_param(params, name).and_then(clean_user_name))
}

fn first_bool_param(params: &[(String, String)], names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| find_param(params, name))
        .and_then(|value| parse_bool(&value))
}

fn first_claim_from_sources(sources: &[Option<&JsonObject>], names: &[&str]) -> Option<String> {
    sources
        .iter()
        .find_map(|object| first_nested_claim(*object, names))
}

fn first_name_claim(sources: &[Option<&JsonObject>], names: &[&str]) -> Option<String> {
    sources.iter().find_map(|object| {
        object.and_then(|object| {
            names.iter().find_map(|name| {
                object
                    .get(*name)
                    .and_then(Value::as_str)
                    .and_then(clean_user_name)
            })
        })
    })
}

fn first_nested_claim(object: Option<&JsonObject>, names: &[&str]) -> Option<String> {
    object.and_then(|object| {
        names.iter().find_map(|name| {
            object
                .get(*name)
                .and_then(Value::as_str)
                .and_then(non_empty)
        })
    })
}

fn joined_name_from_sources(sources: &[Option<&JsonObject>]) -> Option<String> {
    sources.iter().find_map(|object| joined_name(*object))
}

fn joined_name(object: Option<&JsonObject>) -> Option<String> {
    let object = object?;
    let first = object
        .get("first_name")
        .or_else(|| object.get("given_name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let last = object
        .get("last_name")
        .or_else(|| object.get("family_name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    clean_user_name(format!("{first} {last}"))
}

fn first_bool_claim_from_sources(sources: &[Option<&JsonObject>], names: &[&str]) -> Option<bool> {
    sources
        .iter()
        .find_map(|object| first_nested_bool_claim(*object, names))
}

fn first_nested_bool_claim(object: Option<&JsonObject>, names: &[&str]) -> Option<bool> {
    object.and_then(|object| {
        names.iter().find_map(|name| match object.get(*name) {
            Some(Value::Bool(value)) => Some(*value),
            Some(Value::String(value)) => parse_bool(value),
            Some(Value::Number(value)) => value.as_i64().map(|value| value > 0),
            _ => None,
        })
    })
}

fn subscription_active(
    params: &[(String, String)],
    sources: &[Option<&JsonObject>],
    subscription_claims: Option<&JsonObject>,
) -> Option<bool> {
    let status = first_param(params, &["subscription_status", "billing_status"])
        .or_else(|| first_claim_from_sources(sources, &["subscription_status", "billing_status"]))
        .or_else(|| {
            first_nested_claim(subscription_claims, &["status", "state", "billing_status"])
        })?;
    Some(matches!(
        status.to_ascii_lowercase().as_str(),
        "active" | "trialing" | "paid"
    ))
}

fn jwt_claims(access_token: &str) -> Option<Value> {
    let payload = access_token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice::<Value>(&decoded).ok()
}

fn normalize_plan(plan: &str) -> String {
    let plan = plan.trim().to_ascii_lowercase();
    match plan.as_str() {
        "" | "free" | "trial" | "starter" | "basic" => "starter".to_string(),
        "premium" | "paid" => "pro".to_string(),
        _ => plan,
    }
}

fn is_paid_plan(plan: &str) -> bool {
    !matches!(normalize_plan(plan).as_str(), "starter")
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" | "paid" | "active" => Some(true),
        "0" | "false" | "no" | "n" | "free" | "starter" | "inactive" | "none" => Some(false),
        _ => None,
    }
}

fn non_empty(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn clean_user_name(value: impl AsRef<str>) -> Option<String> {
    let value = non_empty(value)?;
    let normalized = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    if value.contains('@')
        || matches!(
            normalized.as_str(),
            "analystblaze" | "analystblazedesktop" | "analystblazeagent" | "desktopagent"
        )
    {
        return None;
    }

    Some(value)
}

fn session_entry() -> Result<Entry, String> {
    Entry::new(SERVICE, SESSION_USER).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        auth_callback_from_deep_link, profile_from_credentials, profile_from_value,
        tokens_from_deep_link, AuthCallback, StoredCredentials,
    };

    #[test]
    fn parses_query_token_callback() {
        let tokens = tokens_from_deep_link(
            "analystblaze://auth?token=access&refresh_token=refresh&name=Ana&plan=pro",
        )
        .expect("valid callback");

        assert_eq!(tokens.access_token, "access");
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh"));
        assert_eq!(tokens.profile.user_name.as_deref(), Some("Ana"));
        assert_eq!(tokens.profile.plan.as_deref(), Some("pro"));
        assert_eq!(tokens.profile.has_paid_plan, Some(true));
    }

    #[test]
    fn parses_fragment_access_token_callback() {
        let tokens =
            tokens_from_deep_link("analystblaze://auth#access_token=access&refresh_token=refresh")
                .expect("valid callback");

        assert_eq!(tokens.access_token, "access");
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh"));
    }

    #[test]
    fn accepts_path_based_auth_callback() {
        let tokens = tokens_from_deep_link("analystblaze:/auth?access_token=access")
            .expect("valid callback");

        assert_eq!(tokens.access_token, "access");
    }

    #[test]
    fn parses_pairing_code_callback() {
        let callback = auth_callback_from_deep_link("analystblaze://auth?code=pair_123")
            .expect("valid callback");

        match callback {
            AuthCallback::PairingCode(code) => assert_eq!(code, "pair_123"),
            AuthCallback::Tokens(_) => panic!("expected pairing code callback"),
        }
    }

    #[test]
    fn rejects_non_auth_callback() {
        assert!(tokens_from_deep_link("analystblaze://billing?token=access").is_err());
    }

    #[test]
    fn rejects_oversized_deep_link() {
        let long_code = "a".repeat(2100);
        assert!(
            auth_callback_from_deep_link(&format!("analystblaze://auth?code={long_code}")).is_err()
        );
    }

    #[test]
    fn rejects_duplicate_auth_params() {
        assert!(auth_callback_from_deep_link("analystblaze://auth?code=one&code=two").is_err());
        assert!(auth_callback_from_deep_link(
            "analystblaze://auth?access_token=one&access_token=two"
        )
        .is_err());
    }

    #[test]
    fn rejects_command_like_deep_link_params() {
        assert!(auth_callback_from_deep_link("analystblaze://auth?code=pair&cmd=calc").is_err());
        assert!(auth_callback_from_deep_link(
            "analystblaze://auth?code=pair&path=C:/Windows/System32"
        )
        .is_err());
    }

    #[test]
    fn parses_free_plan_as_unpaid() {
        let tokens = tokens_from_deep_link("analystblaze://auth?token=access&plan=free")
            .expect("valid callback");

        assert_eq!(tokens.profile.plan.as_deref(), Some("starter"));
        assert_eq!(tokens.profile.has_paid_plan, Some(false));
    }

    #[test]
    fn parses_starter_plan_as_unpaid() {
        let tokens = tokens_from_deep_link("analystblaze://auth?token=access&plan=starter")
            .expect("valid callback");

        assert_eq!(tokens.profile.plan.as_deref(), Some("starter"));
        assert_eq!(tokens.profile.has_paid_plan, Some(false));
    }

    #[test]
    fn parses_username_from_callback() {
        let tokens =
            tokens_from_deep_link("analystblaze://auth?token=access&username=Vitor%20Hugo")
                .expect("valid callback");

        assert_eq!(tokens.profile.user_name.as_deref(), Some("Vitor Hugo"));
    }

    #[test]
    fn ignores_product_name_as_user_name() {
        let tokens = tokens_from_deep_link("analystblaze://auth?token=access&name=AnalystBlaze")
            .expect("valid callback");

        assert_eq!(tokens.profile.user_name, None);
    }

    #[test]
    fn skips_product_name_and_uses_real_username() {
        let tokens = tokens_from_deep_link(
            "analystblaze://auth?token=access&name=AnalystBlaze&username=Vitor%20Hugo",
        )
        .expect("valid callback");

        assert_eq!(tokens.profile.user_name.as_deref(), Some("Vitor Hugo"));
    }

    #[test]
    fn parses_account_profile_response() {
        let profile = profile_from_value(&json!({
            "data": {
                "user": {
                    "profile": {
                        "display_name": "Vitor Hugo"
                    },
                    "email": "vitor@example.com",
                    "plan_tier": "pro",
                    "has_paid_plan": true
                }
            }
        }));

        assert_eq!(profile.user_name.as_deref(), Some("Vitor Hugo"));
        assert_eq!(profile.user_email.as_deref(), Some("vitor@example.com"));
        assert_eq!(profile.plan.as_deref(), Some("pro"));
        assert_eq!(profile.has_paid_plan, Some(true));
    }

    #[test]
    fn sanitizes_stored_product_name() {
        let profile = profile_from_credentials(&StoredCredentials {
            user_name: Some("AnalystBlaze Desktop".to_string()),
            user_email: Some("vitor@example.com".to_string()),
            plan: Some("free".to_string()),
            ..StoredCredentials::default()
        });

        assert_eq!(profile.user_name, None);
        assert_eq!(profile.user_email.as_deref(), Some("vitor@example.com"));
        assert_eq!(profile.plan.as_deref(), Some("starter"));
    }
}
