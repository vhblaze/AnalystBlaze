use serde::Serialize;
use serde_json::{json, Value};

use super::protected_apps;

const MAX_TARGET_LEN: usize = 260;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    ManualUser,
    RemoteCommand,
    LocalPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Safe,
    Sensitive,
    Critical,
}

#[derive(Debug, Clone, Copy)]
pub struct CommandSafetyProfile {
    pub risk: RiskLevel,
    pub requires_local_confirmation: bool,
    pub requires_snapshot: bool,
    pub requires_privileged_helper: bool,
}

#[derive(Debug)]
pub struct SafetyContext<'a> {
    pub source: CommandSource,
    pub allowed_actions: Option<&'a [String]>,
    pub local_confirmation: bool,
    pub privileged_helper_available: bool,
}

#[derive(Debug, Clone)]
pub struct SafetyError {
    pub reason: String,
    pub details: Value,
}

pub fn validate_command(
    action_name: &str,
    payload: Option<&Value>,
    context: &SafetyContext<'_>,
) -> Result<CommandSafetyProfile, SafetyError> {
    let profile = command_profile(action_name).ok_or_else(|| {
        safety_error(
            "unknown_action",
            action_name,
            payload,
            context,
            None,
            json!({ "allowed_actions": supported_actions() }),
        )
    })?;

    if let Some(allowed_actions) = context.allowed_actions {
        if !allowed_actions
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(action_name))
        {
            return Err(safety_error(
                "action_not_allowed_by_policy",
                action_name,
                payload,
                context,
                Some(profile),
                json!({ "policy_allowed_actions": allowed_actions }),
            ));
        }
    }

    if profile.risk == RiskLevel::Critical {
        return Err(safety_error(
            "critical_command_blocked",
            action_name,
            payload,
            context,
            Some(profile),
            json!({ "required_flow": "mfa_plus_local_confirmation_plus_snapshot" }),
        ));
    }

    if profile.requires_privileged_helper && !context.privileged_helper_available {
        return Err(safety_error(
            "privileged_helper_unavailable",
            action_name,
            payload,
            context,
            Some(profile),
            json!({ "required_component": "admin_helper_with_uac" }),
        ));
    }

    if profile.requires_local_confirmation && !context.local_confirmation {
        return Err(safety_error(
            "local_confirmation_required",
            action_name,
            payload,
            context,
            Some(profile),
            json!({ "confirmation": "must_be_collected_on_this_desktop" }),
        ));
    }

    validate_action_payload(action_name, payload, context, profile)?;

    Ok(profile)
}

pub fn command_profile(action_name: &str) -> Option<CommandSafetyProfile> {
    match action_name {
        "DETECT_FOREGROUND_GAME" => Some(CommandSafetyProfile {
            risk: RiskLevel::Safe,
            requires_local_confirmation: false,
            requires_snapshot: false,
            requires_privileged_helper: false,
        }),
        "APPLY_GAME_MODE"
        | "DISABLE_STARTUP_APP"
        | "EMPTY_TEMP"
        | "ENTER_FOCUS_MODE"
        | "RESTORE_SERVICE"
        | "RESTORE_STARTUP_APP"
        | "SET_PROCESS_PRIORITY"
        | "SET_POWER_PLAN_BALANCED"
        | "SET_POWER_PLAN_HIGH_PERFORMANCE"
        | "SET_POWER_PLAN_POWER_SAVER"
        | "STOP_SERVICE" => Some(CommandSafetyProfile {
            risk: RiskLevel::Sensitive,
            requires_local_confirmation: true,
            requires_snapshot: matches!(
                action_name,
                "APPLY_GAME_MODE"
                    | "DISABLE_STARTUP_APP"
                    | "ENTER_FOCUS_MODE"
                    | "SET_POWER_PLAN_BALANCED"
                    | "SET_POWER_PLAN_HIGH_PERFORMANCE"
                    | "SET_POWER_PLAN_POWER_SAVER"
                    | "STOP_SERVICE"
            ),
            requires_privileged_helper: false,
        }),
        "CLEAR_STANDBY_LIST" => Some(CommandSafetyProfile {
            risk: RiskLevel::Sensitive,
            requires_local_confirmation: true,
            requires_snapshot: false,
            requires_privileged_helper: true,
        }),
        "APPLY_LATENCY_TWEAKS" => Some(CommandSafetyProfile {
            risk: RiskLevel::Critical,
            requires_local_confirmation: true,
            requires_snapshot: true,
            requires_privileged_helper: true,
        }),
        _ => None,
    }
}

pub fn supported_actions() -> &'static [&'static str] {
    &[
        "APPLY_GAME_MODE",
        "SET_PROCESS_PRIORITY",
        "EMPTY_TEMP",
        "CLEAR_STANDBY_LIST",
        "SET_POWER_PLAN_HIGH_PERFORMANCE",
        "SET_POWER_PLAN_BALANCED",
        "SET_POWER_PLAN_POWER_SAVER",
        "APPLY_LATENCY_TWEAKS",
        "ENTER_FOCUS_MODE",
        "DETECT_FOREGROUND_GAME",
        "DISABLE_STARTUP_APP",
        "RESTORE_STARTUP_APP",
        "STOP_SERVICE",
        "RESTORE_SERVICE",
    ]
}

pub fn is_critical_process(name: &str) -> bool {
    let normalized = normalize_target_name(name);
    matches!(
        normalized.as_str(),
        "system"
            | "registry"
            | "smss.exe"
            | "csrss.exe"
            | "wininit.exe"
            | "winlogon.exe"
            | "services.exe"
            | "lsass.exe"
            | "svchost.exe"
            | "dwm.exe"
            | "explorer.exe"
            | "fontdrvhost.exe"
            | "sihost.exe"
            | "taskhostw.exe"
            | "msmpeng.exe"
            | "securityhealthservice.exe"
            | "wudfhost.exe"
    ) || normalized.contains("antivirus")
        || normalized.contains("endpoint")
        || normalized.contains("defender")
}

#[allow(dead_code)]
pub fn is_critical_service(name: &str) -> bool {
    let normalized = normalize_target_name(name);
    matches!(
        normalized.as_str(),
        "windefend"
            | "securityhealthservice"
            | "wuauserv"
            | "cryptsvc"
            | "eventlog"
            | "rpcss"
            | "samss"
            | "mpssvc"
            | "nlasvc"
            | "dhcp"
            | "dnscache"
            | "winmgmt"
            | "trustedinstaller"
            | "bits"
            | "gpsvc"
            | "schedule"
            | "plugplay"
            | "profsvc"
    )
}

fn validate_action_payload(
    action_name: &str,
    payload: Option<&Value>,
    context: &SafetyContext<'_>,
    profile: CommandSafetyProfile,
) -> Result<(), SafetyError> {
    if let Some(target) = extract_target(payload) {
        validate_target_string(action_name, payload, context, profile, &target)?;
    }

    match action_name {
        "SET_PROCESS_PRIORITY" => {
            let target = extract_target(payload).ok_or_else(|| {
                safety_error(
                    "process_target_required",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "required_fields": ["target", "process_name", "exe"] }),
                )
            })?;

            if is_critical_process(&target) {
                return Err(safety_error(
                    "critical_process_protected",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "target": target }),
                ));
            }

            if protected_apps::is_protected_app(&target) {
                return Err(safety_error(
                    "protected_app_blocked",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "target": target }),
                ));
            }

            if requested_realtime_priority(payload) {
                return Err(safety_error(
                    "realtime_priority_blocked",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "policy": "never_set_realtime_priority_automatically" }),
                ));
            }
        }
        "EMPTY_TEMP" => {
            let min_age_hours = payload
                .and_then(|value| value.get("min_age_hours"))
                .and_then(Value::as_u64)
                .unwrap_or(24);

            if min_age_hours < 1 {
                return Err(safety_error(
                    "cleanup_min_age_too_low",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "min_age_hours": min_age_hours, "minimum_allowed": 1 }),
                ));
            }
        }
        "DISABLE_STARTUP_APP" => {
            let target = extract_target(payload).ok_or_else(|| {
                safety_error(
                    "startup_app_target_required",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "required_fields": ["target", "name"] }),
                )
            })?;

            if looks_like_security_component(&target) {
                return Err(safety_error(
                    "startup_app_protected",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "target": target }),
                ));
            }
        }
        "STOP_SERVICE" => {
            let target = extract_target(payload).ok_or_else(|| {
                safety_error(
                    "service_target_required",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "required_fields": ["target", "service", "service_name", "name"] }),
                )
            })?;

            if is_critical_service(&target) {
                return Err(safety_error(
                    "critical_service_protected",
                    action_name,
                    payload,
                    context,
                    Some(profile),
                    json!({ "target": target }),
                ));
            }
        }
        _ => {}
    }

    Ok(())
}

fn validate_target_string(
    action_name: &str,
    payload: Option<&Value>,
    context: &SafetyContext<'_>,
    profile: CommandSafetyProfile,
    target: &str,
) -> Result<(), SafetyError> {
    let trimmed = target.trim();
    let invalid = trimmed.is_empty()
        || trimmed.len() > MAX_TARGET_LEN
        || trimmed.contains('\0')
        || trimmed.contains("..")
        || trimmed.contains('*')
        || trimmed.contains('?')
        || trimmed.contains('|')
        || trimmed.contains('&')
        || trimmed.contains(';')
        || trimmed.contains('`');

    if invalid {
        return Err(safety_error(
            "invalid_target",
            action_name,
            payload,
            context,
            Some(profile),
            json!({ "target": target, "max_len": MAX_TARGET_LEN }),
        ));
    }

    Ok(())
}

fn requested_realtime_priority(payload: Option<&Value>) -> bool {
    let Some(payload) = payload else {
        return false;
    };

    ["priority", "priority_class", "class"]
        .iter()
        .filter_map(|key| payload.get(*key).and_then(Value::as_str))
        .any(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            normalized == "realtime" || normalized == "real_time" || normalized == "tempo_real"
        })
}

fn extract_target(payload: Option<&Value>) -> Option<String> {
    let payload = payload?;
    [
        "target",
        "process",
        "process_name",
        "processName",
        "exe",
        "service",
        "service_name",
        "serviceName",
        "name",
    ]
    .iter()
    .find_map(|key| payload.get(*key).and_then(Value::as_str))
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn normalize_target_name(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

fn looks_like_security_component(name: &str) -> bool {
    let normalized = normalize_target_name(name);
    normalized.contains("defender")
        || normalized.contains("security")
        || normalized.contains("antivirus")
        || normalized.contains("endpoint")
        || normalized.contains("vpn")
        || normalized.contains("driver")
}

fn safety_error(
    reason: &str,
    action_name: &str,
    payload: Option<&Value>,
    context: &SafetyContext<'_>,
    profile: Option<CommandSafetyProfile>,
    extra: Value,
) -> SafetyError {
    SafetyError {
        reason: reason.to_string(),
        details: json!({
            "reason": reason,
            "action_name": action_name,
            "source": context.source,
            "risk": profile.map(|profile| profile.risk),
            "requires_local_confirmation": profile.map(|profile| profile.requires_local_confirmation).unwrap_or(false),
            "requires_snapshot": profile.map(|profile| profile.requires_snapshot).unwrap_or(false),
            "requires_privileged_helper": profile.map(|profile| profile.requires_privileged_helper).unwrap_or(false),
            "local_confirmation": context.local_confirmation,
            "payload": sanitize_payload(payload),
            "extra": extra,
        }),
    }
}

fn sanitize_payload(payload: Option<&Value>) -> Value {
    let Some(payload) = payload else {
        return Value::Null;
    };

    match payload {
        Value::Object(map) => Value::Object(
            map.iter()
                .take(20)
                .map(|(key, value)| {
                    let sensitive = key.to_ascii_lowercase();
                    let value = if sensitive.contains("token")
                        || sensitive.contains("secret")
                        || sensitive.contains("password")
                        || sensitive.contains("signature")
                    {
                        json!("[redacted]")
                    } else {
                        sanitize_value(value, 0)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        _ => sanitize_value(payload, 0),
    }
}

fn sanitize_value(value: &Value, depth: usize) -> Value {
    if depth >= 2 {
        return json!("[nested]");
    }

    match value {
        Value::String(value) => json!(value.chars().take(180).collect::<String>()),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(10)
                .map(|value| sanitize_value(value, depth + 1))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .take(12)
                .map(|(key, value)| (key.clone(), sanitize_value(value, depth + 1)))
                .collect(),
        ),
        primitive => primitive.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        is_critical_process, is_critical_service, validate_command, CommandSource, SafetyContext,
    };

    fn context(
        source: CommandSource,
        allowed: Option<&[String]>,
        confirmation: bool,
    ) -> SafetyContext<'_> {
        SafetyContext {
            source,
            allowed_actions: allowed,
            local_confirmation: confirmation,
            privileged_helper_available: false,
        }
    }

    #[test]
    fn protects_critical_process_names() {
        assert!(is_critical_process("lsass.exe"));
        assert!(is_critical_process("C:\\Windows\\System32\\svchost.exe"));
        assert!(is_critical_process("Windows Defender Antivirus Service"));
        assert!(!is_critical_process("discord.exe"));
    }

    #[test]
    fn protects_critical_services() {
        assert!(is_critical_service("WinDefend"));
        assert!(is_critical_service("wuauserv"));
        assert!(!is_critical_service("SomeVendorUpdater"));
    }

    #[test]
    fn blocks_remote_sensitive_command_without_local_confirmation() {
        let allowed = vec!["SET_POWER_PLAN_HIGH_PERFORMANCE".to_string()];
        let result = validate_command(
            "SET_POWER_PLAN_HIGH_PERFORMANCE",
            None,
            &context(CommandSource::RemoteCommand, Some(&allowed), false),
        );

        assert_eq!(result.unwrap_err().reason, "local_confirmation_required");
    }

    #[test]
    fn allows_safe_remote_command_when_policy_allows_it() {
        let allowed = vec!["DETECT_FOREGROUND_GAME".to_string()];
        let result = validate_command(
            "DETECT_FOREGROUND_GAME",
            None,
            &context(CommandSource::RemoteCommand, Some(&allowed), false),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn blocks_action_missing_from_signed_policy() {
        let allowed = vec!["DETECT_FOREGROUND_GAME".to_string()];
        let result = validate_command(
            "EMPTY_TEMP",
            Some(&json!({ "min_age_hours": 24 })),
            &context(CommandSource::RemoteCommand, Some(&allowed), true),
        );

        assert_eq!(result.unwrap_err().reason, "action_not_allowed_by_policy");
    }

    #[test]
    fn blocks_realtime_process_priority() {
        let result = validate_command(
            "SET_PROCESS_PRIORITY",
            Some(&json!({ "process_name": "game.exe", "priority": "realtime" })),
            &context(CommandSource::ManualUser, None, true),
        );

        assert_eq!(result.unwrap_err().reason, "realtime_priority_blocked");
    }

    #[test]
    fn blocks_critical_process_target() {
        let result = validate_command(
            "SET_PROCESS_PRIORITY",
            Some(&json!({ "process_name": "lsass.exe", "priority": "below_normal" })),
            &context(CommandSource::ManualUser, None, true),
        );

        assert_eq!(result.unwrap_err().reason, "critical_process_protected");
    }

    #[test]
    fn blocks_critical_service_target() {
        let result = validate_command(
            "STOP_SERVICE",
            Some(&json!({ "service_name": "WinDefend" })),
            &context(CommandSource::ManualUser, None, true),
        );

        assert_eq!(result.unwrap_err().reason, "critical_service_protected");
    }

    #[test]
    fn blocks_sensitive_startup_apps() {
        let result = validate_command(
            "DISABLE_STARTUP_APP",
            Some(&json!({ "name": "Windows Defender" })),
            &context(CommandSource::ManualUser, None, true),
        );

        assert_eq!(result.unwrap_err().reason, "startup_app_protected");
    }

    #[test]
    fn blocks_critical_latency_tweaks() {
        let result = validate_command(
            "APPLY_LATENCY_TWEAKS",
            None,
            &context(CommandSource::ManualUser, None, true),
        );

        assert_eq!(result.unwrap_err().reason, "critical_command_blocked");
    }
}
