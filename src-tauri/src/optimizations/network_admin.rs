use serde_json::{json, Value};
use std::process::Command;

use super::{
    adaptive::is_safe_dns_literal,
    safety,
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};
use crate::audit;
use crate::process_ext::{decode_console_bytes, CommandExt};

pub async fn flush_dns_cache(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(flush_dns_cache_sync).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao limpar cache de DNS: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn set_dns_servers(payload: Option<Value>) -> ExecutionResult {
    let adapter_name = extract_payload_string(payload.as_ref(), &["adapterName", "adapter_name"]);
    let dns_servers = extract_dns_servers(payload.as_ref());
    let fallback_payload = payload.clone();

    let Some(adapter_name) = adapter_name else {
        return ExecutionResult {
            success: false,
            message: "Informe o adaptador de rede.".to_string(),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        };
    };

    if !safety::is_safe_network_target(&adapter_name) {
        return ExecutionResult {
            success: false,
            message: "Nome de adaptador invalido.".to_string(),
            details: json!({ "implemented": true, "adapter": adapter_name }),
        };
    }

    if dns_servers.is_empty() {
        return ExecutionResult {
            success: false,
            message: "Informe ao menos um servidor DNS valido.".to_string(),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        };
    }

    match tokio::task::spawn_blocking(move || set_dns_servers_sync(&adapter_name, &dns_servers))
        .await
    {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao alterar servidores DNS: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn reset_winsock_catalog(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(reset_winsock_catalog_sync).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao resetar catalogo Winsock: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

#[cfg(windows)]
fn flush_dns_cache_sync() -> ExecutionResult {
    let output = match Command::new("ipconfig").args(["/flushdns"]).no_window().output() {
        Ok(output) => output,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel chamar ipconfig.".to_string(),
                details: json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    };

    let stdout = decode_console_bytes(&output.stdout);
    let success = output.status.success();

    let _ = audit::record_event(
        if success { "info" } else { "warn" },
        "optimization.network.dns_flushed",
        "Cache de DNS local limpo.",
        json!({ "stdout": stdout.trim(), "success": success }),
    );

    if success {
        ExecutionResult::ok(
            "Cache de DNS limpo.",
            json!({ "implemented": true, "stdout": stdout.trim() }),
        )
    } else {
        ExecutionResult {
            success: false,
            message: "O Windows recusou limpar o cache de DNS.".to_string(),
            details: json!({ "implemented": true, "stdout": stdout.trim() }),
        }
    }
}

#[cfg(not(windows))]
fn flush_dns_cache_sync() -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Flush de DNS indisponivel nesta plataforma.".to_string(),
        details: json!({ "implemented": true }),
    }
}

#[cfg(windows)]
fn set_dns_servers_sync(adapter_name: &str, dns_servers: &[String]) -> ExecutionResult {
    let previous_dns_servers = match query_adapter_dns_servers(adapter_name) {
        Ok(servers) => servers,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel consultar a configuracao de DNS atual do adaptador."
                    .to_string(),
                details: json!({ "implemented": true, "adapter": adapter_name, "error": error }),
            };
        }
    };
    let was_dhcp = previous_dns_servers.is_empty();

    let snapshot = OptimizationSnapshot::new(
        "SET_DNS_SERVERS",
        vec![SnapshotEntry::DnsConfiguration {
            adapter_name: adapter_name.to_string(),
            previous_dns_servers: previous_dns_servers.clone(),
            was_dhcp,
        }],
        json!({
            "adapter": adapter_name,
            "previous_dns_servers": previous_dns_servers,
            "target_dns_servers": dns_servers,
            "was_dhcp": was_dhcp,
        }),
    );

    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        return ExecutionResult {
            success: false,
            message: "A alteracao foi bloqueada porque o snapshot nao pode ser salvo.".to_string(),
            details: json!({
                "implemented": true,
                "adapter": adapter_name,
                "snapshot_error": error,
            }),
        };
    }

    let servers_literal = dns_servers
        .iter()
        .map(|server| format!("'{}'", escape_powershell_literal(server)))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "Set-DnsClientServerAddress -InterfaceAlias '{}' -ServerAddresses @({})",
        escape_powershell_literal(adapter_name),
        servers_literal
    );

    match run_powershell(&script) {
        Ok(_) => {
            let _ = audit::record_event(
                "info",
                "optimization.network.dns_servers_changed",
                "Servidores DNS do adaptador alterados com snapshot reversivel.",
                json!({ "adapter": adapter_name, "dns_servers": dns_servers }),
            );
            ExecutionResult::ok(
                "Servidores DNS alterados com snapshot reversivel.",
                json!({
                    "implemented": true,
                    "adapter": adapter_name,
                    "dns_servers": dns_servers,
                    "snapshot": {
                        "id": snapshot.id,
                        "entries": snapshot.entries.len(),
                        "reversible": true,
                    },
                }),
            )
        }
        Err(error) => {
            let _ = snapshot::discard_snapshot(&snapshot.id);
            ExecutionResult {
                success: false,
                message: "O Windows recusou alterar os servidores DNS.".to_string(),
                details: json!({
                    "implemented": true,
                    "adapter": adapter_name,
                    "snapshot_discarded": true,
                    "error": error,
                }),
            }
        }
    }
}

#[cfg(not(windows))]
fn set_dns_servers_sync(adapter_name: &str, _dns_servers: &[String]) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Configuracao de DNS indisponivel nesta plataforma.".to_string(),
        details: json!({ "implemented": true, "adapter": adapter_name }),
    }
}

#[cfg(windows)]
fn reset_winsock_catalog_sync() -> ExecutionResult {
    let output = match Command::new("netsh").args(["winsock", "reset"]).no_window().output() {
        Ok(output) => output,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel chamar netsh.".to_string(),
                details: json!({ "implemented": true, "error": error.to_string() }),
            };
        }
    };

    let stdout = decode_console_bytes(&output.stdout);
    let stderr = decode_console_bytes(&output.stderr);
    let success = output.status.success();

    let _ = audit::record_event(
        if success { "info" } else { "warn" },
        "optimization.network.winsock_reset",
        "Catalogo Winsock resetado; acao irreversivel, requer reinicializacao.",
        json!({ "stdout": stdout.trim(), "stderr": stderr.trim(), "success": success }),
    );

    ExecutionResult {
        success,
        message: if success {
            "Catalogo Winsock resetado. Reinicie o computador para concluir.".to_string()
        } else {
            "O Windows recusou resetar o catalogo Winsock.".to_string()
        },
        details: json!({
            "implemented": true,
            "requiresReboot": true,
            "reversible": false,
            "stdout": stdout.trim(),
            "stderr": stderr.trim(),
        }),
    }
}

#[cfg(not(windows))]
fn reset_winsock_catalog_sync() -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Reset de Winsock indisponivel nesta plataforma.".to_string(),
        details: json!({ "implemented": true }),
    }
}

#[cfg(windows)]
fn query_adapter_dns_servers(adapter_name: &str) -> Result<Vec<String>, String> {
    let script = format!(
        "(Get-DnsClientServerAddress -InterfaceAlias '{}' -AddressFamily IPv4 -ErrorAction Stop).ServerAddresses -join ','",
        escape_powershell_literal(adapter_name)
    );
    let output = run_powershell(&script)?;
    Ok(output
        .trim()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect())
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<String, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .no_window()
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        Ok(decode_console_bytes(&output.stdout))
    } else {
        Err(decode_console_bytes(&output.stderr).trim().to_string())
    }
}

#[cfg(windows)]
fn escape_powershell_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn extract_dns_servers(payload: Option<&Value>) -> Vec<String> {
    payload
        .and_then(|value| {
            value
                .get("dnsServers")
                .or_else(|| value.get("dns_servers"))
        })
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| is_safe_dns_literal(item))
                .take(2)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn extract_payload_string(payload: Option<&Value>, keys: &[&str]) -> Option<String> {
    let payload = payload?;
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
