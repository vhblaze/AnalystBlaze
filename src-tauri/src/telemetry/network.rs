use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkDiagnostics {
    pub connected: bool,
    pub adapter_name: Option<String>,
    pub adapter_description: Option<String>,
    pub adapter_status: Option<String>,
    pub adapter_type: Option<String>,
    pub link_speed: Option<String>,
    pub gateway: Option<String>,
    pub dns_servers: Vec<String>,
    pub wifi_ssid: Option<String>,
    pub wifi_signal_percent: Option<f64>,
    pub wifi_radio_type: Option<String>,
    pub wifi_channel: Option<String>,
    pub gateway_latency_ms: Option<f64>,
    pub dns_latency_ms: Option<f64>,
    pub external_latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_percent: Option<f64>,
    pub probes: Vec<NetworkProbe>,
    pub recommendations: Vec<String>,
    pub refreshed_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkProbe {
    pub label: String,
    pub target: String,
    pub sent: u32,
    pub received: u32,
    pub packet_loss_percent: f64,
    pub avg_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
}

#[derive(Debug, Clone, Default)]
struct AdapterInfo {
    name: Option<String>,
    description: Option<String>,
    status: Option<String>,
    adapter_type: Option<String>,
    link_speed: Option<String>,
    gateway: Option<String>,
    dns_servers: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct WifiInfo {
    ssid: Option<String>,
    signal_percent: Option<f64>,
    radio_type: Option<String>,
    channel: Option<String>,
}

pub fn collect_network_diagnostics() -> NetworkDiagnostics {
    let adapter = collect_active_adapter();
    let wifi = collect_wifi_info();
    let mut probes = Vec::new();

    if let Some(gateway) = adapter.gateway.as_deref().and_then(sanitize_target) {
        probes.push(probe_latency("gateway", &gateway, 3));
    }

    probes.push(probe_latency("dns_cloudflare", "1.1.1.1", 4));
    probes.push(probe_latency("dns_google", "8.8.8.8", 4));

    let gateway_probe = probes.iter().find(|probe| probe.label == "gateway");
    let dns_probe = probes.iter().find(|probe| probe.label == "dns_cloudflare");
    let external_probe = probes
        .iter()
        .find(|probe| probe.label != "gateway" && probe.received > 0 && probe.avg_ms.is_some())
        .or(dns_probe);
    let packet_loss_percent = probes
        .iter()
        .filter(|probe| probe.sent > 0)
        .map(|probe| probe.packet_loss_percent)
        .fold(None, |worst: Option<f64>, value| {
            Some(worst.map_or(value, |current| current.max(value)))
        });
    let jitter_ms = external_probe.and_then(|probe| probe.jitter_ms);
    let connected = adapter
        .status
        .as_deref()
        .map(|status| status.eq_ignore_ascii_case("up"))
        .unwrap_or(false)
        || probes.iter().any(|probe| probe.received > 0);

    let mut diagnostics = NetworkDiagnostics {
        connected,
        adapter_name: adapter.name,
        adapter_description: adapter.description,
        adapter_status: adapter.status,
        adapter_type: adapter.adapter_type,
        link_speed: adapter.link_speed,
        gateway: adapter.gateway,
        dns_servers: adapter.dns_servers,
        wifi_ssid: wifi.ssid,
        wifi_signal_percent: wifi.signal_percent,
        wifi_radio_type: wifi.radio_type,
        wifi_channel: wifi.channel,
        gateway_latency_ms: gateway_probe.and_then(|probe| probe.avg_ms),
        dns_latency_ms: dns_probe.and_then(|probe| probe.avg_ms),
        external_latency_ms: external_probe.and_then(|probe| probe.avg_ms),
        jitter_ms,
        packet_loss_percent,
        probes,
        recommendations: Vec::new(),
        refreshed_at: chrono::Utc::now().timestamp(),
    };
    diagnostics.recommendations = network_recommendations(&diagnostics);
    diagnostics
}

pub fn collect_network_sample() -> NetworkDiagnostics {
    let probes = vec![
        probe_latency("dns_cloudflare", "1.1.1.1", 2),
        probe_latency("dns_google", "8.8.8.8", 2),
    ];
    diagnostics_from_probes(probes)
}

fn diagnostics_from_probes(probes: Vec<NetworkProbe>) -> NetworkDiagnostics {
    let dns_probe = probes.iter().find(|probe| probe.label == "dns_cloudflare");
    let external_probe = probes
        .iter()
        .find(|probe| probe.received > 0 && probe.avg_ms.is_some())
        .or(dns_probe);
    let packet_loss_percent = probes
        .iter()
        .filter(|probe| probe.sent > 0)
        .map(|probe| probe.packet_loss_percent)
        .fold(None, |worst: Option<f64>, value| {
            Some(worst.map_or(value, |current| current.max(value)))
        });
    let mut diagnostics = NetworkDiagnostics {
        connected: probes.iter().any(|probe| probe.received > 0),
        dns_latency_ms: dns_probe.and_then(|probe| probe.avg_ms),
        external_latency_ms: external_probe.and_then(|probe| probe.avg_ms),
        jitter_ms: external_probe.and_then(|probe| probe.jitter_ms),
        packet_loss_percent,
        probes,
        refreshed_at: chrono::Utc::now().timestamp(),
        ..NetworkDiagnostics::default()
    };
    diagnostics.recommendations = network_recommendations(&diagnostics);
    diagnostics
}

fn collect_active_adapter() -> AdapterInfo {
    let Some(value) = powershell_json(
        "$cfg=Get-NetIPConfiguration | Where-Object {$_.IPv4DefaultGateway -ne $null -and $_.NetAdapter.Status -eq 'Up'} | Select-Object -First 1; $result=if ($null -eq $cfg) { [pscustomobject]@{} } else { $adapter=Get-NetAdapter -InterfaceIndex $cfg.InterfaceIndex -ErrorAction SilentlyContinue; [pscustomobject]@{ Name=$adapter.Name; InterfaceDescription=$adapter.InterfaceDescription; Status=$adapter.Status; MediaType=$adapter.MediaType; PhysicalMediaType=$adapter.PhysicalMediaType; LinkSpeed=$adapter.LinkSpeed; Gateway=($cfg.IPv4DefaultGateway.NextHop | Select-Object -First 1); DnsServers=($cfg.DNSServer.ServerAddresses -join ',') } }; $result | ConvertTo-Json -Compress",
    ) else {
        return AdapterInfo::default();
    };

    let adapter_type = first_text(&value, &["PhysicalMediaType", "MediaType"]);

    AdapterInfo {
        name: text_field(&value, "Name"),
        description: text_field(&value, "InterfaceDescription"),
        status: text_field(&value, "Status"),
        adapter_type,
        link_speed: text_field(&value, "LinkSpeed"),
        gateway: text_field(&value, "Gateway"),
        dns_servers: text_field(&value, "DnsServers")
            .map(|raw| {
                raw.split(',')
                    .filter_map(clean_string)
                    .take(6)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    }
}

fn collect_wifi_info() -> WifiInfo {
    let output = Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .output()
        .ok();
    let Some(output) = output.filter(|output| output.status.success()) else {
        return WifiInfo::default();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut info = WifiInfo::default();

    for line in text.lines() {
        let Some((raw_key, raw_value)) = line.split_once(':') else {
            continue;
        };
        let key = raw_key.trim().to_ascii_lowercase();
        let value = raw_value.trim();
        if value.is_empty() {
            continue;
        }

        if key == "ssid" {
            info.ssid = clean_string(value);
        } else if key.contains("signal") || key.contains("sinal") {
            info.signal_percent = value
                .trim_end_matches('%')
                .trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok();
        } else if key.contains("radio") || key.starts_with("tipo de r") {
            info.radio_type = clean_string(value);
        } else if key.contains("channel") || key.contains("canal") {
            info.channel = clean_string(value);
        }
    }

    info
}

fn probe_latency(label: &str, target: &str, count: u32) -> NetworkProbe {
    let Some(target) = sanitize_target(target) else {
        return NetworkProbe {
            label: label.to_string(),
            target: target.to_string(),
            sent: count,
            packet_loss_percent: 100.0,
            ..NetworkProbe::default()
        };
    };

    let output = Command::new("ping")
        .args(["-n", &count.to_string(), &target])
        .output()
        .ok();
    let Some(output) = output else {
        return NetworkProbe {
            label: label.to_string(),
            target,
            sent: count,
            packet_loss_percent: 100.0,
            ..NetworkProbe::default()
        };
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let latencies = parse_ping_latencies(&stdout);
    let received = latencies.len() as u32;
    let packet_loss_percent = if count > 0 {
        (((count.saturating_sub(received)) as f64 / count as f64) * 100.0).round()
    } else {
        100.0
    };
    let avg_ms = average(&latencies).map(round2);
    let min_ms = latencies.iter().copied().reduce(f64::min).map(round2);
    let max_ms = latencies.iter().copied().reduce(f64::max).map(round2);
    let jitter_ms = jitter(&latencies).map(round2);

    NetworkProbe {
        label: label.to_string(),
        target,
        sent: count,
        received,
        packet_loss_percent,
        avg_ms,
        min_ms,
        max_ms,
        jitter_ms,
    }
}

fn network_recommendations(diagnostics: &NetworkDiagnostics) -> Vec<String> {
    let mut recommendations = Vec::new();

    if !diagnostics.connected {
        recommendations.push("network_offline".to_string());
    }
    if diagnostics.packet_loss_percent.unwrap_or_default() >= 2.0 {
        recommendations.push("packet_loss_detected".to_string());
    }
    if diagnostics.jitter_ms.unwrap_or_default() >= 20.0 {
        recommendations.push("jitter_high".to_string());
    }
    if diagnostics.external_latency_ms.unwrap_or_default() >= 90.0 {
        recommendations.push("latency_high".to_string());
    }
    if diagnostics.wifi_signal_percent.unwrap_or(100.0) < 55.0 {
        recommendations.push("wifi_signal_low".to_string());
    }
    if diagnostics.wifi_ssid.is_some()
        && diagnostics
            .wifi_radio_type
            .as_deref()
            .map(|radio| radio.contains("802.11b") || radio.contains("802.11g"))
            .unwrap_or(false)
    {
        recommendations.push("wifi_legacy_radio".to_string());
    }
    let adapter_text = format!(
        "{} {}",
        diagnostics.adapter_name.as_deref().unwrap_or_default(),
        diagnostics
            .adapter_description
            .as_deref()
            .unwrap_or_default()
    )
    .to_ascii_lowercase();
    if adapter_text.contains("vpn") || adapter_text.contains("virtual") {
        recommendations.push("vpn_or_virtual_adapter_active".to_string());
    }
    if recommendations.is_empty() {
        recommendations.push("network_stable".to_string());
    }

    recommendations
}

fn powershell_json(script: &str) -> Option<Value> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(&text).ok()
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(clean_string)
}

fn first_text(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| text_field(value, key))
}

fn parse_ping_latencies(output: &str) -> Vec<f64> {
    output
        .lines()
        .filter_map(|line| {
            let normalized = line.to_ascii_lowercase();
            if !(normalized.contains("time") || normalized.contains("tempo")) {
                return None;
            }
            latency_before_ms(line)
        })
        .collect()
}

fn latency_before_ms(line: &str) -> Option<f64> {
    let lower = line.to_ascii_lowercase();
    let ms_index = lower.find("ms")?;
    let before = &line[..ms_index];
    let digits = before
        .chars()
        .rev()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == ',')
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.replace(',', ".").parse::<f64>().ok()
}

fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn jitter(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let deltas = values
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .collect::<Vec<_>>();
    average(&deltas)
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn clean_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.chars().take(180).collect())
    }
}

fn sanitize_target(target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() || target.len() > 253 {
        return None;
    }
    if !target
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '_'))
    {
        return None;
    }
    Some(target.to_string())
}

pub fn best_latency_ms(diagnostics: &NetworkDiagnostics) -> f64 {
    diagnostics
        .external_latency_ms
        .or(diagnostics.dns_latency_ms)
        .or(diagnostics.gateway_latency_ms)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::parse_ping_latencies;

    #[test]
    fn parses_english_ping_latencies() {
        let values = parse_ping_latencies(
            "Reply from 1.1.1.1: bytes=32 time=14ms TTL=57\nReply from 1.1.1.1: bytes=32 time<1ms TTL=57",
        );

        assert_eq!(values, vec![14.0, 1.0]);
    }

    #[test]
    fn parses_portuguese_ping_latencies() {
        let values = parse_ping_latencies(
            "Resposta de 1.1.1.1: bytes=32 tempo=13ms TTL=57\nResposta de 1.1.1.1: bytes=32 tempo=15ms TTL=57",
        );

        assert_eq!(values, vec![13.0, 15.0]);
    }
}
