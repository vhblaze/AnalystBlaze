use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::UdpSocket;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::process_ext::{decode_console_bytes, CommandExt};

static LAST_AUTO_DNS_CHECK_AT: OnceLock<Mutex<i64>> = OnceLock::new();

// The dashboard's network telemetry sample is cached for 30s (see
// TelemetryCollector::network_diagnostics) so the 2s telemetry loop doesn't
// re-probe adapters/gateway/DNS on every tick. That's fine for passive
// display, but it means the summary card can show stale info for up to 30s
// after the user actively changes something (enabling/disabling an
// adapter, DNS servers, TCP tuning, DHCP renewal). Network-changing actions
// call invalidate_network_cache() on success so the very next telemetry
// tick re-probes instead of waiting out the cache window.
static NETWORK_CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);

pub fn invalidate_network_cache() {
    NETWORK_CACHE_INVALIDATED.store(true, Ordering::SeqCst);
}

pub fn take_network_cache_invalidated() -> bool {
    NETWORK_CACHE_INVALIDATED.swap(false, Ordering::SeqCst)
}

/// Cheap in-memory throttle so the "best DNS route" local-policy decision
/// doesn't re-run the live benchmark (real UDP queries) on every tick -
/// only once the configured cooldown has elapsed since the last attempt.
pub fn should_run_auto_dns_check(min_interval_seconds: i64) -> bool {
    let cell = LAST_AUTO_DNS_CHECK_AT.get_or_init(|| Mutex::new(0));
    let Ok(mut last) = cell.lock() else {
        return false;
    };
    let now = chrono::Utc::now().timestamp();
    if now.saturating_sub(*last) < min_interval_seconds.max(0) {
        return false;
    }
    *last = now;
    true
}

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
    /// This host's own current send+receive throughput on the active
    /// adapter, sampled the same way as the fields below (two
    /// Get-NetAdapterStatistics reads 500ms apart). Only meaningful as "is
    /// this PC busy on the network right now" - see
    /// possible_external_congestion in network_recommendations for how it's
    /// used, and its doc comment for why this can't identify *which* other
    /// device (if any) is responsible.
    pub adapter_throughput_kbps: Option<f64>,
    /// New ReceivedPacketErrors+OutboundPacketErrors counted *during* the
    /// same 500ms sample window above - not the adapter's lifetime total
    /// (which could be old and no longer relevant). A nonzero value here
    /// means the NIC/cable/driver is actively erroring right now, the
    /// closest software-only proxy for "bad cable or bad port" available
    /// without a link-layer diagnostic tool.
    pub adapter_error_rate_per_sec: Option<f64>,
    /// Same idea as adapter_error_rate_per_sec but for discarded packets -
    /// more often a sign of buffer/driver pressure (which overlaps with
    /// congestion) than physical damage, so it's surfaced separately rather
    /// than folded into the error count.
    pub adapter_discard_rate_per_sec: Option<f64>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAdapterSummary {
    pub name: String,
    pub description: Option<String>,
    pub status: Option<String>,
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
    let health = adapter.name.as_deref().and_then(measure_adapter_health);
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
        adapter_throughput_kbps: health.as_ref().map(|health| health.throughput_kbps),
        adapter_error_rate_per_sec: health.as_ref().map(|health| health.error_rate_per_sec),
        adapter_discard_rate_per_sec: health.as_ref().map(|health| health.discard_rate_per_sec),
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
    // Previously picked the *first* Get-NetIPConfiguration entry that had
    // any non-null IPv4DefaultGateway object and Status "Up" - but
    // Get-NetIPConfiguration's enumeration order has nothing to do with
    // which adapter Windows actually routes internet traffic through, and
    // some virtual adapters (VPN/LAN-tunnel software) can carry a non-null
    // but effectively empty IPv4DefaultGateway object, passing that filter
    // without being a real gateway. That produced misattributed diagnostics
    // - e.g. reporting "Radmin VPN" as the active adapter with Gateway "--"
    // while probes to 1.1.1.1/8.8.8.8 were actually leaving through
    // Ethernet the whole time. Querying the real IPv4 default route
    // (lowest combined route+interface metric, exactly how Windows itself
    // picks the outbound adapter) and resolving *that* route's interface
    // is the correct source of truth instead.
    let Some(value) = powershell_json(
        r#"$route = Get-NetRoute -AddressFamily IPv4 -ErrorAction SilentlyContinue |
    Where-Object { $_.DestinationPrefix -eq '0.0.0.0/0' } |
    Sort-Object -Property @{Expression = { $_.RouteMetric + $_.InterfaceMetric }} |
    Select-Object -First 1;
$result = if ($null -eq $route) { [pscustomobject]@{} } else {
    $adapter = Get-NetAdapter -InterfaceIndex $route.ifIndex -ErrorAction SilentlyContinue;
    $cfg = Get-NetIPConfiguration -InterfaceIndex $route.ifIndex -ErrorAction SilentlyContinue;
    [pscustomobject]@{
        Name = $adapter.Name; InterfaceDescription = $adapter.InterfaceDescription; Status = $adapter.Status;
        MediaType = $adapter.MediaType; PhysicalMediaType = $adapter.PhysicalMediaType; LinkSpeed = $adapter.LinkSpeed;
        Gateway = $route.NextHop; DnsServers = ($cfg.DNSServer.ServerAddresses -join ',')
    }
};
$result | ConvertTo-Json -Compress"#,
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

struct AdapterHealthSample {
    throughput_kbps: f64,
    error_rate_per_sec: f64,
    discard_rate_per_sec: f64,
}

/// Two Get-NetAdapterStatistics samples half a second apart - same
/// call+wait+call pattern as optimizations::network_admin's
/// measure_adapter_traffic_kbps, just extended to also diff the
/// error/discard counters instead of only bytes, so a single PowerShell
/// round trip covers both throughput and health. New*/500ms rather than the
/// adapter's lifetime totals: a NIC can carry a handful of errors from
/// months ago that say nothing about right now, so only errors that show up
/// *during this live sample* count.
fn measure_adapter_health(adapter_name: &str) -> Option<AdapterHealthSample> {
    let script = format!(
        r#"$a1 = Get-NetAdapterStatistics -Name '{name}' -ErrorAction SilentlyContinue;
if (-not $a1) {{ exit 1 }}
$bytes1 = $a1.ReceivedBytes + $a1.SentBytes
$errors1 = $a1.ReceivedPacketErrors + $a1.OutboundPacketErrors
$discards1 = $a1.ReceivedDiscardedPackets + $a1.OutboundDiscardedPackets
Start-Sleep -Milliseconds 500
$a2 = Get-NetAdapterStatistics -Name '{name}' -ErrorAction SilentlyContinue;
if (-not $a2) {{ exit 1 }}
$bytes2 = $a2.ReceivedBytes + $a2.SentBytes
$errors2 = $a2.ReceivedPacketErrors + $a2.OutboundPacketErrors
$discards2 = $a2.ReceivedDiscardedPackets + $a2.OutboundDiscardedPackets
[pscustomobject]@{{
    ThroughputBytes = [Math]::Max(0, $bytes2 - $bytes1)
    NewErrors = [Math]::Max(0, $errors2 - $errors1)
    NewDiscards = [Math]::Max(0, $discards2 - $discards1)
}} | ConvertTo-Json -Compress"#,
        name = escape_powershell_literal(adapter_name)
    );
    let value = powershell_json(&script)?;
    let throughput_bytes = value.get("ThroughputBytes").and_then(Value::as_f64)?;
    let new_errors = value.get("NewErrors").and_then(Value::as_f64).unwrap_or(0.0);
    let new_discards = value.get("NewDiscards").and_then(Value::as_f64).unwrap_or(0.0);

    Some(AdapterHealthSample {
        throughput_kbps: (throughput_bytes * 8.0 / 1000.0) / 0.5,
        error_rate_per_sec: new_errors / 0.5,
        discard_rate_per_sec: new_discards / 0.5,
    })
}

/// Mirrors optimizations::network_admin::escape_powershell_literal - kept as
/// a local copy rather than a cross-module dependency since telemetry
/// (read-only observation) and optimizations (privileged actions) are
/// deliberately kept separate in this codebase.
fn escape_powershell_literal(value: &str) -> String {
    value.replace('\'', "''")
}

pub fn list_network_adapters() -> Vec<NetworkAdapterSummary> {
    let Some(value) = powershell_json(
        "Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | Select-Object Name,InterfaceDescription,Status | ConvertTo-Json -Compress",
    ) else {
        return Vec::new();
    };

    let items: Vec<Value> = match value {
        Value::Array(items) => items,
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    };

    items
        .iter()
        .filter_map(|item| {
            let name = text_field(item, "Name")?;
            Some(NetworkAdapterSummary {
                name,
                description: text_field(item, "InterfaceDescription"),
                status: text_field(item, "Status"),
            })
        })
        .collect()
}

pub fn list_all_network_adapters() -> Vec<NetworkAdapterSummary> {
    // Get-NetAdapter alone also returns "ghost" virtual mobile-broadband
    // adapters Windows' WWAN stack leaves behind (a long-standing Windows
    // quirk, unrelated to this app) - dozens of them can accumulate over
    // time. They report Status "Not Present" and can't be toggled since
    // there's no real device backing them, so exclude them here to match
    // what Settings > Network adapters shows.
    let Some(value) = powershell_json(
        "Get-NetAdapter | Where-Object { $_.Status -ne 'Not Present' } | \
         Select-Object Name,InterfaceDescription,Status | ConvertTo-Json -Compress",
    ) else {
        return Vec::new();
    };

    let items: Vec<Value> = match value {
        Value::Array(items) => items,
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    };

    items
        .iter()
        .filter_map(|item| {
            let name = text_field(item, "Name")?;
            Some(NetworkAdapterSummary {
                name,
                description: text_field(item, "InterfaceDescription"),
                status: text_field(item, "Status"),
            })
        })
        .collect()
}

fn collect_wifi_info() -> WifiInfo {
    let output = Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .no_window()
        .output()
        .ok();
    let Some(output) = output.filter(|output| output.status.success()) else {
        return WifiInfo::default();
    };
    let text = decode_console_bytes(&output.stdout);
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
        .no_window()
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
    let stdout = decode_console_bytes(&output.stdout);
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
    if diagnostics
        .adapter_error_rate_per_sec
        .is_some_and(|rate| rate > 0.0)
    {
        recommendations.push("network_adapter_errors_detected".to_string());
    }
    if possible_external_congestion(diagnostics) {
        recommendations.push("possible_external_network_congestion".to_string());
    }
    if recommendations.is_empty() {
        recommendations.push("network_stable".to_string());
    }

    recommendations
}

/// This host has no visibility into what other devices on the LAN are
/// doing (no router/ARP/UPnP integration - see measure_adapter_health's
/// doc comment) and can't distinguish "another device is saturating the
/// link" from "the ISP's own route is degraded" - both look the same from
/// here. This is only a proxy: jitter/loss is already elevated (the same
/// thresholds as jitter_high/packet_loss_detected above) while this PC's
/// *own* measured throughput is close to idle, meaning whatever is causing
/// the degradation almost certainly isn't this machine's own traffic.
const LOW_OWN_THROUGHPUT_KBPS_THRESHOLD: f64 = 50.0;

fn possible_external_congestion(diagnostics: &NetworkDiagnostics) -> bool {
    let degraded = diagnostics.packet_loss_percent.unwrap_or_default() >= 2.0
        || diagnostics.jitter_ms.unwrap_or_default() >= 20.0;
    degraded
        && diagnostics
            .adapter_throughput_kbps
            .is_some_and(|kbps| kbps < LOW_OWN_THROUGHPUT_KBPS_THRESHOLD)
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
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = decode_console_bytes(&output.stdout).trim().to_string();
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

// ============================================================
// Traceroute - hop-by-hop route diagnostics (D "Rede" round 2).
// Shells out to Windows' own tracert.exe (-d skips reverse DNS,
// which is both faster and one less thing to sanitize) rather than
// hand-rolling raw ICMP with TTL manipulation - same call+parse
// pattern as probe_latency above, just a different command.
// ============================================================

const TRACEROUTE_MAX_HOPS: u32 = 20;
const TRACEROUTE_TIMEOUT_MS: u32 = 800;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracerouteHop {
    pub hop: u32,
    pub ip: Option<String>,
    pub rtt_ms: [Option<f64>; 3],
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracerouteResult {
    pub target: String,
    pub hops: Vec<TracerouteHop>,
    pub reached_target: bool,
}

pub fn run_traceroute(raw_target: &str) -> Result<TracerouteResult, String> {
    let target = sanitize_target(raw_target).ok_or_else(|| "alvo_invalido".to_string())?;

    let output = Command::new("tracert")
        .args([
            "-d",
            "-h",
            &TRACEROUTE_MAX_HOPS.to_string(),
            "-w",
            &TRACEROUTE_TIMEOUT_MS.to_string(),
            &target,
        ])
        .no_window()
        .output()
        .map_err(|error| error.to_string())?;

    let stdout = decode_console_bytes(&output.stdout);
    let hops: Vec<TracerouteHop> = stdout.lines().filter_map(parse_traceroute_line).collect();
    let reached_target = hops
        .last()
        .and_then(|hop| hop.ip.as_deref())
        .map(|ip| ip == target)
        .unwrap_or(false);

    Ok(TracerouteResult {
        target,
        hops,
        reached_target,
    })
}

fn parse_traceroute_line(line: &str) -> Option<TracerouteHop> {
    let trimmed = line.trim();
    let mut tokens = trimmed.split_whitespace().peekable();
    let hop: u32 = tokens.next()?.parse().ok()?;

    let mut rtts: Vec<Option<f64>> = Vec::with_capacity(3);
    while rtts.len() < 3 {
        let Some(&token) = tokens.peek() else { break };
        if token == "*" {
            rtts.push(None);
            tokens.next();
            continue;
        }
        if let Some(stripped) = token.strip_suffix("ms") {
            if !stripped.is_empty() {
                rtts.push(stripped.trim_start_matches('<').parse::<f64>().ok());
                tokens.next();
                continue;
            }
        }
        let numeric = token.trim_start_matches('<');
        if !numeric.is_empty() && numeric.chars().all(|ch| ch.is_ascii_digit()) {
            rtts.push(numeric.parse::<f64>().ok());
            tokens.next();
            // consume a separate "ms" token if present
            if tokens.peek() == Some(&"ms") {
                tokens.next();
            }
            continue;
        }
        break;
    }
    while rtts.len() < 3 {
        rtts.push(None);
    }

    let remainder: String = tokens.collect::<Vec<_>>().join(" ");
    let lower = remainder.to_ascii_lowercase();
    let timed_out = lower.contains("timed out")
        || lower.contains("esgotado")
        || lower.contains("tempo limite")
        || remainder.trim().is_empty();
    let ip = if timed_out {
        None
    } else {
        clean_string(remainder.trim())
    };

    Some(TracerouteHop {
        hop,
        ip,
        rtt_ms: [rtts[0], rtts[1], rtts[2]],
        timed_out,
    })
}

// ============================================================
// DNS benchmark - times a real DNS query (not just a ping) against
// a handful of candidate resolvers plus whatever the adapter is
// using today, so "which DNS is actually fastest for this network
// right now" is a measured answer instead of a guess. Hand-rolled
// minimal DNS-over-UDP query (12-byte header + one question) - the
// wire format is simple enough that pulling in a full DNS client
// crate for this one timing probe isn't worth it.
// ============================================================

const DNS_BENCHMARK_TIMEOUT_MS: u64 = 1500;
const DNS_BENCHMARK_PROBE_DOMAIN: &str = "www.cloudflare.com";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsBenchmarkResult {
    pub label: String,
    pub server: String,
    pub latency_ms: Option<f64>,
    pub is_current: bool,
}

pub fn benchmark_dns_servers(current_dns: &[String]) -> Vec<DnsBenchmarkResult> {
    let mut candidates: Vec<(String, String)> = vec![
        ("Cloudflare".to_string(), "1.1.1.1".to_string()),
        ("Google".to_string(), "8.8.8.8".to_string()),
        ("Quad9".to_string(), "9.9.9.9".to_string()),
        ("OpenDNS".to_string(), "208.67.222.222".to_string()),
    ];
    for server in current_dns {
        if sanitize_target(server).is_some()
            && !candidates.iter().any(|(_, ip)| ip == server)
        {
            candidates.push(("Atual (DHCP/manual)".to_string(), server.clone()));
        }
    }

    candidates
        .into_iter()
        .map(|(label, server)| {
            let latency_ms = benchmark_one_dns(&server).map(round2);
            let is_current = current_dns.iter().any(|dns| dns == &server);
            DnsBenchmarkResult {
                label,
                server,
                latency_ms,
                is_current,
            }
        })
        .collect()
}

fn benchmark_one_dns(server_ip: &str) -> Option<f64> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket
        .set_read_timeout(Some(Duration::from_millis(DNS_BENCHMARK_TIMEOUT_MS)))
        .ok()?;
    let id: u16 = 0x4241; // "AB" - arbitrary, only used to match the reply
    let query = build_dns_query(id, DNS_BENCHMARK_PROBE_DOMAIN);
    let addr = format!("{server_ip}:53");

    let started = Instant::now();
    socket.send_to(&query, &addr).ok()?;
    let mut buf = [0u8; 512];
    let (len, _from) = socket.recv_from(&mut buf).ok()?;
    let elapsed = started.elapsed();

    if len < 2 || buf[0] != (id >> 8) as u8 || buf[1] != (id & 0xff) as u8 {
        return None;
    }
    Some(elapsed.as_secs_f64() * 1000.0)
}

// ============================================================
// Gateway identity - best-effort, read-only. Just an HTTP GET to the
// router's own admin page on the LAN to grab whatever it's willing to
// announce (HTTP Server header or <title>) - no login, no credentials,
// nothing is ever sent to it besides the request itself. Purely
// informational; if it doesn't answer (most routers use HTTPS with a
// self-signed cert, or nothing at all on 80), this just returns None.
// ============================================================

pub async fn probe_gateway_identity(gateway_ip: &str) -> Option<String> {
    let target = sanitize_target(gateway_ip)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1200))
        .build()
        .ok()?;
    let response = client.get(format!("http://{target}/")).send().await.ok()?;

    let server_header = response
        .headers()
        .get(reqwest::header::SERVER)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let body = response.text().await.unwrap_or_default();
    let title = extract_html_title(&body);

    title.or(server_header)
}

fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    clean_string(html.get(start..end)?.trim())
}

fn build_dns_query(id: u16, qname: &str) -> Vec<u8> {
    let mut packet = Vec::with_capacity(32 + qname.len());
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&[0x01, 0x00]); // flags: standard query, recursion desired
    packet.extend_from_slice(&[0x00, 0x01]); // QDCOUNT=1
    packet.extend_from_slice(&[0x00, 0x00]); // ANCOUNT=0
    packet.extend_from_slice(&[0x00, 0x00]); // NSCOUNT=0
    packet.extend_from_slice(&[0x00, 0x00]); // ARCOUNT=0
    for label in qname.split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0); // root label
    packet.extend_from_slice(&[0x00, 0x01]); // QTYPE=A
    packet.extend_from_slice(&[0x00, 0x01]); // QCLASS=IN
    packet
}

#[cfg(test)]
mod tests {
    use super::{
        network_recommendations, parse_ping_latencies, parse_traceroute_line, NetworkDiagnostics,
    };

    #[test]
    fn parses_traceroute_hop_with_three_replies() {
        let hop = parse_traceroute_line("  1     2 ms     1 ms     1 ms  192.168.1.1").unwrap();
        assert_eq!(hop.hop, 1);
        assert_eq!(hop.rtt_ms, [Some(2.0), Some(1.0), Some(1.0)]);
        assert_eq!(hop.ip.as_deref(), Some("192.168.1.1"));
        assert!(!hop.timed_out);
    }

    #[test]
    fn parses_traceroute_hop_with_sub_millisecond_reply() {
        let hop = parse_traceroute_line("  2    <1 ms    <1 ms    <1 ms  10.0.0.1").unwrap();
        assert_eq!(hop.rtt_ms, [Some(1.0), Some(1.0), Some(1.0)]);
        assert_eq!(hop.ip.as_deref(), Some("10.0.0.1"));
    }

    #[test]
    fn parses_timed_out_hop_in_english() {
        let hop = parse_traceroute_line("  3     *        *        *     Request timed out.").unwrap();
        assert_eq!(hop.rtt_ms, [None, None, None]);
        assert!(hop.timed_out);
        assert!(hop.ip.is_none());
    }

    #[test]
    fn parses_timed_out_hop_in_portuguese() {
        let hop = parse_traceroute_line("  4     *        *        *     Esgotado o tempo limite do pedido.").unwrap();
        assert!(hop.timed_out);
        assert!(hop.ip.is_none());
    }

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

    fn stable_diagnostics() -> NetworkDiagnostics {
        NetworkDiagnostics {
            connected: true,
            external_latency_ms: Some(20.0),
            ..NetworkDiagnostics::default()
        }
    }

    #[test]
    fn flags_adapter_errors_only_when_a_live_error_rate_was_measured() {
        let mut diagnostics = stable_diagnostics();
        assert!(!network_recommendations(&diagnostics)
            .contains(&"network_adapter_errors_detected".to_string()));

        diagnostics.adapter_error_rate_per_sec = Some(0.0);
        assert!(!network_recommendations(&diagnostics)
            .contains(&"network_adapter_errors_detected".to_string()));

        diagnostics.adapter_error_rate_per_sec = Some(2.0);
        assert!(network_recommendations(&diagnostics)
            .contains(&"network_adapter_errors_detected".to_string()));
    }

    #[test]
    fn flags_external_congestion_only_when_degraded_and_this_host_is_idle() {
        let mut diagnostics = stable_diagnostics();
        diagnostics.jitter_ms = Some(35.0);
        diagnostics.adapter_throughput_kbps = Some(20_000.0); // this PC is busy itself
        assert!(!network_recommendations(&diagnostics)
            .contains(&"possible_external_network_congestion".to_string()));

        diagnostics.adapter_throughput_kbps = Some(5.0); // this PC is basically idle
        assert!(network_recommendations(&diagnostics)
            .contains(&"possible_external_network_congestion".to_string()));

        // Idle but network quality is fine - no false positive.
        let mut healthy_idle = stable_diagnostics();
        healthy_idle.adapter_throughput_kbps = Some(5.0);
        assert!(!network_recommendations(&healthy_idle)
            .contains(&"possible_external_network_congestion".to_string()));
    }
}
