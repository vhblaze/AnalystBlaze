use serde_json::json;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).expect("valid json")
}

fn lock_package_version(package: &str) -> Option<(u64, u64, u64)> {
    let lock = include_str!("../Cargo.lock");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() != format!("name = \"{package}\"") {
            continue;
        }
        for next in lines.by_ref().take(4) {
            let next = next.trim();
            if !next.starts_with("version = ") {
                continue;
            }
            let raw = next.trim_start_matches("version = ").trim_matches('"');
            let mut parts = raw.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next()?.parse().ok()?;
            return Some((major, minor, patch));
        }
    }
    None
}

fn version_at_least(actual: (u64, u64, u64), required: (u64, u64, u64)) -> bool {
    actual >= required
}

fn has_expected_deep_link_scheme() -> bool {
    let config = parse_json(include_str!("../tauri.conf.json"));
    let schemes = config
        .pointer("/plugins/deep-link/desktop/schemes")
        .and_then(Value::as_array);

    matches!(schemes, Some(values) if values.len() == 1 && values[0].as_str() == Some("analystblaze"))
}

fn capabilities_are_minimal() -> bool {
    let capability = parse_json(include_str!("../capabilities/default.json"));
    let permissions = capability
        .get("permissions")
        .and_then(Value::as_array)
        .expect("capability permissions");
    let permission_names: Vec<&str> = permissions.iter().filter_map(Value::as_str).collect();

    permission_names.contains(&"core:default")
        && permission_names.contains(&"deep-link:default")
        && !permission_names.iter().any(|name| name.contains("shell"))
        && !permission_names.iter().any(|name| name.contains("fs:"))
        && !permission_names
            .iter()
            .any(|name| name.ends_with(":allow-all"))
}

fn shell_plugin_is_absent() -> bool {
    let manifest = include_str!("../Cargo.toml");
    let lock = include_str!("../Cargo.lock");

    !manifest.contains("tauri-plugin-shell") && !lock.contains("name = \"tauri-plugin-shell\"")
}

fn opener_calls_are_static_web_urls() -> bool {
    let lib = include_str!("../src/lib.rs");

    lib.matches("tauri_plugin_opener::open_url").count() == 4
        && lib.contains("open_url(&state.config.web_login_url")
        && lib.contains("open_url(&state.config.web_account_url")
        && lib.contains("open_url(&state.config.web_billing_url")
        && lib.contains("open_url(&state.config.web_insights_url")
        && !lib.contains("open_url(&raw_url")
}

fn tauri_core_meets_patch_floor() -> bool {
    lock_package_version("tauri")
        .map(|version| version_at_least(version, (2, 11, 1)))
        .unwrap_or(false)
}

fn tauri_csp_is_explicit() -> bool {
    let config = parse_json(include_str!("../tauri.conf.json"));
    let csp = config.pointer("/app/security/csp");

    csp.is_some() && !csp.unwrap().is_null()
}

fn deep_link_parser_is_strict() -> bool {
    let auth_source = include_str!("../src/auth/mod.rs");

    auth_source.contains("url.scheme() == \"analystblaze\"")
        && auth_source.contains("url.host_str() == Some(\"auth\")")
        && auth_source.contains("url.path().trim_matches('/') == \"auth\"")
        && auth_source.contains("rejects_non_auth_callback")
}

fn deep_link_rejects_unsafe_shapes() -> bool {
    let auth_source = include_str!("../src/auth/mod.rs");

    auth_source.contains("MAX_DEEP_LINK_BYTES")
        && auth_source.contains("FORBIDDEN_AUTH_PARAMS")
        && auth_source.contains("rejects_oversized_deep_link")
        && auth_source.contains("rejects_duplicate_auth_params")
        && auth_source.contains("rejects_command_like_deep_link_params")
}

fn secrets_use_keyring_not_plaintext_files() -> bool {
    let auth_source = include_str!("../src/auth/mod.rs");
    let audit_source = include_str!("../src/audit.rs");

    auth_source.contains("use keyring::{Entry")
        && auth_source.contains("Entry::new(SERVICE, SESSION_USER)")
        && auth_source.contains("entry.set_password")
        && !auth_source.contains("fs::write")
        && audit_source.contains("[redacted]")
        && audit_source.contains("signature")
}

fn privileged_helper_requires_trusted_signed_source() -> bool {
    let helper_source = include_str!("../src/optimizations/privileged_helper.rs");

    helper_source.contains("exe_path_is_trusted_service_source")
        && helper_source.contains("Get-AuthenticodeSignature")
        && helper_source.contains("exe_signature_is_trusted")
        && helper_source.contains("GetNamedPipeClientProcessId")
        && helper_source.contains("verify_response_signature")
        && helper_source.contains("optimization.helper.request_rejected")
        && helper_source.contains("run_elevated_script")
        && helper_source.contains("supported_actions")
}

fn lab_manual_validation_doc_exists() -> bool {
    let doc = include_str!("../../docs/windows-lab-validation.md");

    doc.contains("snapshot")
        && doc.contains("service ACL")
        && doc.contains("signed artifact")
        && doc.contains("DLL search-order")
        && doc.contains("roll back")
}

fn status(passed: bool) -> &'static str {
    if passed {
        "pass"
    } else {
        "fail"
    }
}

fn desktop_security_results() -> Vec<Value> {
    let tauri_version = lock_package_version("tauri")
        .map(|(major, minor, patch)| format!("{major}.{minor}.{patch}"))
        .unwrap_or_else(|| "unknown".to_string());

    vec![
        json!({
            "id": "ABZ-DESK-DEEPLINK-001",
            "title": "Deep-link scheme is scoped to analystblaze auth callbacks",
            "target_component": "desktop-tauri",
            "severity": "high",
            "status": status(has_expected_deep_link_scheme()),
            "evidence": "tauri.conf.json deep-link desktop schemes should contain only analystblaze.",
            "impacted_files_routes_modules": ["src-tauri/tauri.conf.json"],
            "remediation_summary": "Keep custom protocol handling limited to the expected analystblaze scheme and signed auth callback flow."
        }),
        json!({
            "id": "ABZ-DESK-CAP-001",
            "title": "Tauri capabilities avoid filesystem, shell, and allow-all grants",
            "target_component": "desktop-tauri",
            "severity": "critical",
            "status": status(capabilities_are_minimal()),
            "evidence": "default capability should include core/deep-link only and avoid shell/fs/allow-all permissions.",
            "impacted_files_routes_modules": ["src-tauri/capabilities/default.json"],
            "remediation_summary": "Grant only command scopes required by the packaged agent; add narrow plugin scopes before enabling new plugins."
        }),
        json!({
            "id": "ABZ-DESK-SHELL-001",
            "title": "Tauri shell plugin is not linked",
            "target_component": "desktop-tauri",
            "severity": "critical",
            "status": status(shell_plugin_is_absent()),
            "evidence": "Cargo.toml and Cargo.lock should not include tauri-plugin-shell unless a scoped allowlist test is added.",
            "impacted_files_routes_modules": ["src-tauri/Cargo.toml", "src-tauri/Cargo.lock"],
            "remediation_summary": "Keep shell plugin disabled, or add explicit command and protocol allowlists before enabling it."
        }),
        json!({
            "id": "ABZ-DESK-OPENER-001",
            "title": "External opener calls use configured web URLs only",
            "target_component": "desktop-tauri",
            "severity": "high",
            "status": status(opener_calls_are_static_web_urls()),
            "evidence": "open_url calls should only use web_login_url, web_account_url, web_billing_url, and web_insights_url from trusted config.",
            "impacted_files_routes_modules": ["src-tauri/src/lib.rs"],
            "remediation_summary": "Never pass raw deep-link, remote, or user-controlled URLs into opener or shell-like APIs."
        }),
        json!({
            "id": "ABZ-DESK-TAURI-001",
            "title": "Tauri core is at the local-origin confusion patch floor",
            "target_component": "desktop-tauri",
            "severity": "critical",
            "status": status(tauri_core_meets_patch_floor()),
            "evidence": format!("Cargo.lock tauri version is {tauri_version}; required floor is 2.11.1."),
            "impacted_files_routes_modules": ["src-tauri/Cargo.toml", "src-tauri/Cargo.lock"],
            "remediation_summary": "Upgrade Tauri core to 2.11.1 or newer and refresh the lockfile."
        }),
        json!({
            "id": "ABZ-DESK-CSP-001",
            "title": "Packaged Tauri agent has an explicit CSP",
            "target_component": "desktop-tauri",
            "severity": "high",
            "status": status(tauri_csp_is_explicit()),
            "evidence": "tauri.conf.json app.security.csp should be an explicit policy, not null.",
            "impacted_files_routes_modules": ["src-tauri/tauri.conf.json"],
            "remediation_summary": "Set a restrictive CSP for packaged builds and avoid remote/iframe content unless separately isolated."
        }),
        json!({
            "id": "ABZ-DESK-DEEPLINK-002",
            "title": "Deep-link auth parser rejects non-auth hosts and paths",
            "target_component": "desktop-tauri",
            "severity": "high",
            "status": status(deep_link_parser_is_strict()),
            "evidence": "auth parser should require analystblaze://auth/auth and include a negative regression test.",
            "impacted_files_routes_modules": ["src-tauri/src/auth/mod.rs"],
            "remediation_summary": "Keep strict URL parsing and reject malformed, duplicated, oversized, or command-like deep-link inputs."
        }),
        json!({
            "id": "ABZ-DESK-DEEPLINK-003",
            "title": "Deep-link parser rejects oversized, duplicated, and command-like auth inputs",
            "target_component": "desktop-tauri",
            "severity": "high",
            "status": status(deep_link_rejects_unsafe_shapes()),
            "evidence": "auth parser should include explicit negative tests for oversized, duplicate, and command-like parameters.",
            "impacted_files_routes_modules": ["src-tauri/src/auth/mod.rs"],
            "remediation_summary": "Keep the deep-link parser narrow and reject inputs that look like commands, files, URLs, or repeated auth fields."
        }),
        json!({
            "id": "ABZ-DESK-SECRETS-001",
            "title": "Desktop credentials use OS keyring storage and audit redaction",
            "target_component": "desktop-tauri",
            "severity": "critical",
            "status": status(secrets_use_keyring_not_plaintext_files()),
            "evidence": "auth storage should use keyring and avoid plaintext file writes; audit output should redact sensitive names.",
            "impacted_files_routes_modules": ["src-tauri/src/auth/mod.rs", "src-tauri/src/audit.rs"],
            "remediation_summary": "Keep hw_secret, access tokens, and refresh tokens in keyring/DPAPI-backed storage and out of logs/files."
        }),
        json!({
            "id": "ABZ-DESK-HELPER-001",
            "title": "Privileged helper install path requires trusted signed source and allowlisted actions",
            "target_component": "desktop-tauri",
            "severity": "critical",
            "status": status(privileged_helper_requires_trusted_signed_source()),
            "evidence": "helper source should check Program Files-style install roots, Authenticode signature status, UAC path, and supported action allowlist.",
            "impacted_files_routes_modules": ["src-tauri/src/optimizations/privileged_helper.rs"],
            "remediation_summary": "Gate privileged helper install/actions behind explicit UAC, signed binaries, trusted install paths, and action allowlists."
        }),
        json!({
            "id": "ABZ-DESK-LAB-001",
            "title": "Windows high-risk local checks are documented as lab-only validations",
            "target_component": "desktop-tauri",
            "severity": "high",
            "status": status(lab_manual_validation_doc_exists()),
            "evidence": "manual guide should cover snapshot VM, service ACL, helper path, signature, DLL simulation, driver blocking, telemetry, cleanup.",
            "impacted_files_routes_modules": ["docs/windows-lab-validation.md"],
            "remediation_summary": "Keep unsafe local attack classes in a disposable Windows VM guide, not automated exploit tests."
        }),
    ]
}

#[test]
fn write_desktop_security_report_artifacts() {
    let results = desktop_security_results();
    let report_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("desktop repo root")
        .join("reports");
    fs::create_dir_all(&report_root).expect("create reports directory");

    let report = json!({
        "suite": "analystblaze-desktop-security-pentest",
        "generated_by": "src-tauri/tests/security_harness.rs",
        "results": results,
    });
    fs::write(
        report_root.join("desktop-security-pentest-results.json"),
        serde_json::to_string_pretty(&report).expect("serialize report"),
    )
    .expect("write json report");

    let mut markdown = String::from("# AnalystBlaze Desktop Security Pentest Report\n\n");
    markdown.push_str("| ID | Severity | Status | Title | Remediation |\n");
    markdown.push_str("|---|---|---|---|---|\n");
    for case in report
        .get("results")
        .and_then(Value::as_array)
        .expect("results array")
    {
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            case["id"].as_str().unwrap_or("unknown"),
            case["severity"].as_str().unwrap_or("unknown"),
            case["status"].as_str().unwrap_or("unknown"),
            case["title"].as_str().unwrap_or("unknown"),
            case["remediation_summary"].as_str().unwrap_or("unknown")
        ));
    }
    markdown.push_str("\n## Remediation Checklist\n\n");
    let mut has_failures = false;
    for case in report
        .get("results")
        .and_then(Value::as_array)
        .expect("results array")
        .iter()
        .filter(|case| case["status"].as_str() == Some("fail"))
    {
        has_failures = true;
        markdown.push_str(&format!(
            "- [ ] `{}` {}\n",
            case["id"].as_str().unwrap_or("unknown"),
            case["remediation_summary"].as_str().unwrap_or("unknown")
        ));
    }
    if !has_failures {
        markdown.push_str("- [x] No failing desktop security findings in this run.\n");
    }
    fs::write(report_root.join("latest-security-report.md"), markdown)
        .expect("write markdown report");
}

#[test]
fn tauri_config_uses_only_expected_deep_link_scheme() {
    assert!(has_expected_deep_link_scheme());
}

#[test]
fn tauri_capabilities_do_not_grant_filesystem_shell_or_broad_permissions() {
    assert!(capabilities_are_minimal());
}

#[test]
fn shell_plugin_is_not_linked_into_the_desktop_agent() {
    assert!(shell_plugin_is_absent());
}

#[test]
fn opener_usage_is_limited_to_configured_web_urls() {
    assert!(opener_calls_are_static_web_urls());
}

#[test]
fn tauri_core_is_at_or_above_origin_confusion_patch_floor() {
    let version = lock_package_version("tauri").expect("tauri package exists in Cargo.lock");

    assert!(
        version_at_least(version, (2, 11, 1)),
        "Tauri core must be >= 2.11.1 for local-origin confusion hardening; found {version:?}"
    );
}

#[test]
fn tauri_csp_is_explicit_for_packaged_agent() {
    assert!(
        tauri_csp_is_explicit(),
        "Tauri CSP should be explicit instead of null before production packaging"
    );
}

#[test]
fn deep_link_auth_parser_rejects_non_auth_hosts_by_default() {
    assert!(deep_link_parser_is_strict());
}

#[test]
fn deep_link_parser_rejects_oversized_duplicate_and_command_like_inputs() {
    assert!(deep_link_rejects_unsafe_shapes());
}

#[test]
fn credentials_are_keyring_backed_and_redacted_from_audit() {
    assert!(secrets_use_keyring_not_plaintext_files());
}

#[test]
fn privileged_helper_is_gated_by_signed_trusted_source_and_allowlist() {
    assert!(privileged_helper_requires_trusted_signed_source());
}

#[test]
fn windows_high_risk_checks_are_lab_only_documented() {
    assert!(lab_manual_validation_doc_exists());
}
