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

pub async fn set_interface_metric(payload: Option<Value>) -> ExecutionResult {
    let adapter_name = extract_payload_string(payload.as_ref(), &["adapterName", "adapter_name"]);
    let metric = payload
        .as_ref()
        .and_then(|value| value.get("metric"))
        .and_then(Value::as_u64);
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

    let Some(metric) = metric.filter(|value| (1..=9999).contains(value)) else {
        return ExecutionResult {
            success: false,
            message: "Informe uma prioridade de rede valida (1 a 9999).".to_string(),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        };
    };

    match tokio::task::spawn_blocking(move || set_interface_metric_sync(&adapter_name, metric as u32))
        .await
    {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao alterar prioridade de rede: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn set_adapter_enabled(payload: Option<Value>) -> ExecutionResult {
    let adapter_name = extract_payload_string(payload.as_ref(), &["adapterName", "adapter_name"]);
    let enabled = payload
        .as_ref()
        .and_then(|value| value.get("enabled"))
        .and_then(Value::as_bool);
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

    let Some(enabled) = enabled else {
        return ExecutionResult {
            success: false,
            message: "Informe se o adaptador deve ser ativado ou desativado.".to_string(),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        };
    };

    match tokio::task::spawn_blocking(move || set_adapter_enabled_sync(&adapter_name, enabled))
        .await
    {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao alterar estado do adaptador: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

/// Checked before offering to disable an adapter from the network
/// diagnostics card - adapters like Radmin VPN are routinely used as a
/// virtual LAN for games in progress, so disabling one out from under an
/// active session would drop it. Neither signal blocks the action outright;
/// the frontend uses them to decide whether a second, explicit
/// confirmation is warranted.
pub async fn check_adapter_disable_guard(payload: Option<Value>) -> ExecutionResult {
    let adapter_name = extract_payload_string(payload.as_ref(), &["adapterName", "adapter_name"]);
    let Some(adapter_name) = adapter_name else {
        return ExecutionResult {
            success: false,
            message: "Informe o adaptador de rede.".to_string(),
            details: json!({ "implemented": true }),
        };
    };
    if !safety::is_safe_network_target(&adapter_name) {
        return ExecutionResult {
            success: false,
            message: "Nome de adaptador invalido.".to_string(),
            details: json!({ "implemented": true, "adapter": adapter_name }),
        };
    }

    let game = super::detection::detect_game_process();
    let traffic = tokio::task::spawn_blocking(move || measure_adapter_traffic_kbps(&adapter_name))
        .await
        .unwrap_or(None);

    let active_traffic_kbps = traffic.unwrap_or(0.0);
    // Idle virtual adapters still carry a trickle of keepalive/heartbeat
    // traffic - this threshold is well above that but well below anything
    // resembling real game traffic.
    const ACTIVE_TRAFFIC_THRESHOLD_KBPS: f64 = 15.0;

    ExecutionResult::ok(
        "Verificacao de contexto concluida.",
        json!({
            "implemented": true,
            "gameInForeground": game.detected,
            "gameProcessName": game.process_name,
            "adapterHasActiveTraffic": active_traffic_kbps >= ACTIVE_TRAFFIC_THRESHOLD_KBPS,
            "adapterTrafficKbps": active_traffic_kbps,
        }),
    )
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
            crate::telemetry::network::invalidate_network_cache();
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
fn set_interface_metric_sync(adapter_name: &str, metric: u32) -> ExecutionResult {
    let (previous_metric, previous_automatic) = match query_adapter_interface_metric(adapter_name) {
        Ok(value) => value,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel consultar a prioridade de rede atual do adaptador."
                    .to_string(),
                details: json!({ "implemented": true, "adapter": adapter_name, "error": error }),
            };
        }
    };

    let snapshot = OptimizationSnapshot::new(
        "SET_INTERFACE_METRIC",
        vec![SnapshotEntry::InterfaceMetric {
            adapter_name: adapter_name.to_string(),
            previous_metric,
            previous_automatic,
        }],
        json!({
            "adapter": adapter_name,
            "previous_metric": previous_metric,
            "previous_automatic": previous_automatic,
            "target_metric": metric,
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

    let script = format!(
        "Set-NetIPInterface -InterfaceAlias '{}' -AddressFamily IPv4 -AutomaticMetric Disabled -InterfaceMetric {}",
        escape_powershell_literal(adapter_name),
        metric
    );

    match run_powershell(&script) {
        Ok(_) => {
            crate::telemetry::network::invalidate_network_cache();
            let _ = audit::record_event(
                "info",
                "optimization.network.interface_metric_changed",
                "Prioridade de rede do adaptador alterada com snapshot reversivel.",
                json!({ "adapter": adapter_name, "metric": metric }),
            );
            ExecutionResult::ok(
                "Adaptador priorizado com snapshot reversivel.",
                json!({
                    "implemented": true,
                    "adapter": adapter_name,
                    "metric": metric,
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
                message: "O Windows recusou alterar a prioridade de rede.".to_string(),
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
fn set_interface_metric_sync(adapter_name: &str, _metric: u32) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Prioridade de rede indisponivel nesta plataforma.".to_string(),
        details: json!({ "implemented": true, "adapter": adapter_name }),
    }
}

#[cfg(windows)]
fn set_adapter_enabled_sync(adapter_name: &str, enabled: bool) -> ExecutionResult {
    let previous_enabled = match query_adapter_enabled(adapter_name) {
        Ok(value) => value,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel consultar o estado atual do adaptador.".to_string(),
                details: json!({ "implemented": true, "adapter": adapter_name, "error": error }),
            };
        }
    };

    if previous_enabled == enabled {
        return ExecutionResult::ok(
            if enabled {
                "O adaptador ja estava ativado."
            } else {
                "O adaptador ja estava desativado."
            },
            json!({ "implemented": true, "adapter": adapter_name, "enabled": enabled, "unchanged": true }),
        );
    }

    let snapshot = OptimizationSnapshot::new(
        "SET_ADAPTER_ENABLED",
        vec![SnapshotEntry::AdapterEnabled {
            adapter_name: adapter_name.to_string(),
            previous_enabled,
        }],
        json!({
            "adapter": adapter_name,
            "previous_enabled": previous_enabled,
            "target_enabled": enabled,
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

    let verb = if enabled { "Enable-NetAdapter" } else { "Disable-NetAdapter" };
    let script = format!(
        "{verb} -Name '{}' -Confirm:$false -ErrorAction Stop",
        escape_powershell_literal(adapter_name)
    );

    match run_powershell(&script) {
        Ok(_) => {
            crate::telemetry::network::invalidate_network_cache();
            let _ = audit::record_event(
                "info",
                "optimization.network.adapter_enabled_changed",
                "Estado do adaptador de rede alterado com snapshot reversivel.",
                json!({ "adapter": adapter_name, "enabled": enabled }),
            );
            ExecutionResult::ok(
                if enabled {
                    "Adaptador ativado com snapshot reversivel."
                } else {
                    "Adaptador desativado com snapshot reversivel."
                },
                json!({
                    "implemented": true,
                    "adapter": adapter_name,
                    "enabled": enabled,
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
                message: "O Windows recusou alterar o estado do adaptador.".to_string(),
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
fn set_adapter_enabled_sync(adapter_name: &str, _enabled: bool) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Estado de adaptador indisponivel nesta plataforma.".to_string(),
        details: json!({ "implemented": true, "adapter": adapter_name }),
    }
}

/// Two Get-NetAdapterStatistics samples half a second apart, converted to
/// an approximate combined send+receive kbps. Good enough to distinguish
/// "idle" from "actively carrying traffic right now" - not meant to be a
/// precise bandwidth reading.
#[cfg(windows)]
fn measure_adapter_traffic_kbps(adapter_name: &str) -> Option<f64> {
    let script = format!(
        r#"$a = Get-NetAdapterStatistics -Name '{name}' -ErrorAction SilentlyContinue;
if (-not $a) {{ exit 1 }}
$b1 = $a.ReceivedBytes + $a.SentBytes
Start-Sleep -Milliseconds 500
$a2 = Get-NetAdapterStatistics -Name '{name}' -ErrorAction SilentlyContinue;
if (-not $a2) {{ exit 1 }}
$b2 = $a2.ReceivedBytes + $a2.SentBytes
[Math]::Max(0, $b2 - $b1)"#,
        name = escape_powershell_literal(adapter_name)
    );
    let output = run_powershell(&script).ok()?;
    let delta_bytes: f64 = output.trim().parse().ok()?;
    Some((delta_bytes * 8.0 / 1000.0) / 0.5)
}

#[cfg(not(windows))]
fn measure_adapter_traffic_kbps(_adapter_name: &str) -> Option<f64> {
    None
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
fn query_adapter_interface_metric(adapter_name: &str) -> Result<(u32, bool), String> {
    let script = format!(
        "$i=Get-NetIPInterface -InterfaceAlias '{}' -AddressFamily IPv4 -ErrorAction Stop; [pscustomobject]@{{ Metric=$i.InterfaceMetric; Automatic=($i.AutomaticMetric -eq 'Enabled') }} | ConvertTo-Json -Compress",
        escape_powershell_literal(adapter_name)
    );
    let output = run_powershell(&script)?;
    let parsed: Value = serde_json::from_str(output.trim())
        .map_err(|error| format!("resposta inesperada do Windows: {error}"))?;
    let metric = parsed
        .get("Metric")
        .and_then(Value::as_u64)
        .ok_or_else(|| "metrica de interface ausente na resposta do Windows".to_string())?
        as u32;
    let automatic = parsed
        .get("Automatic")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok((metric, automatic))
}

#[cfg(windows)]
fn query_adapter_enabled(adapter_name: &str) -> Result<bool, String> {
    let script = format!(
        "(Get-NetAdapter -Name '{}' -ErrorAction Stop).Status",
        escape_powershell_literal(adapter_name)
    );
    let output = run_powershell(&script)?;
    // Status is "Up", "Disconnected", "Degraded" etc. for an administratively
    // enabled adapter that just has no active link - only "Disabled" means
    // the adapter itself is off. Treating anything but "Up" as disabled would
    // misreport e.g. Wi-Fi with no network in range as already off.
    Ok(!output.trim().eq_ignore_ascii_case("Disabled"))
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
