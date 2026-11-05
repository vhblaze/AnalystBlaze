//! L2 (redesign D3): tempo de boot medido pelo próprio Windows.
//!
//! O Windows registra em Microsoft-Windows-Diagnostics-Performance/Operational
//! o evento 100 (quanto o boot levou) e o 101 (um app de inicialização que
//! atrasou o boot, e em quanto). O log só pode ser lido por administrador,
//! então a leitura roda no componente de sistema (READ_BOOT_PERFORMANCE).
//! Só lê; não muda nada.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::ExecutionResult;

/// Janela olhada no log: os boots e atrasos dos últimos 30 dias.
const LOOKBACK_DAYS: u32 = 30;
/// Quantos boots recentes a tela recebe.
const RECENT_BOOTS: usize = 5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootRecord {
    /// Do ligar ao Windows pronto para usar (BootTime), em ms.
    pub boot_ms: u64,
    /// Até a área de trabalho aparecer (MainPathBootTime), em ms.
    pub main_path_ms: Option<u64>,
    /// Da área de trabalho até o PC ficar livre (BootPostBootTime), em ms.
    pub post_boot_ms: Option<u64>,
    /// Quando o Windows registrou (ISO 8601).
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlowStartupApp {
    /// Executável ("Teams.exe").
    pub name: String,
    /// Nome amigável que o Windows guardou ("Microsoft Teams").
    pub friendly_name: Option<String>,
    /// Quanto o app atrasou o boot, em ms (o maior visto no período).
    pub degradation_ms: u64,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPerformance {
    /// "ok", "no_events" (o Windows não registrou boots no período) ou
    /// "unavailable" (não deu para ler o log).
    pub status: String,
    pub last_boot: Option<BootRecord>,
    pub recent_boots: Vec<BootRecord>,
    pub slow_apps: Vec<SlowStartupApp>,
}

fn data_u64(data: &Value, key: &str) -> Option<u64> {
    let value = data.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse::<u64>().ok()))
}

fn data_string(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

/// Monta o resultado a partir dos eventos, do mais novo para o mais velho,
/// no formato `{ id, time, data: { Nome: valor } }` que o script devolve.
pub fn parse_boot_events(events: &[Value]) -> BootPerformance {
    let mut boots: Vec<BootRecord> = Vec::new();
    let mut slow: Vec<SlowStartupApp> = Vec::new();

    for event in events {
        let id = event.get("id").and_then(Value::as_u64).unwrap_or_default();
        let time = event.get("time").and_then(Value::as_str).unwrap_or_default().to_string();
        let empty = json!({});
        let data = event.get("data").unwrap_or(&empty);
        match id {
            100 => {
                if let Some(boot_ms) = data_u64(data, "BootTime").filter(|ms| *ms > 0) {
                    boots.push(BootRecord {
                        boot_ms,
                        main_path_ms: data_u64(data, "MainPathBootTime"),
                        post_boot_ms: data_u64(data, "BootPostBootTime"),
                        recorded_at: time,
                    });
                }
            }
            101 => {
                let Some(name) = data_string(data, "Name") else { continue };
                let degradation_ms = data_u64(data, "DegradationTime").unwrap_or_default();
                if degradation_ms == 0 {
                    continue;
                }
                match slow.iter_mut().find(|app| app.name.eq_ignore_ascii_case(&name)) {
                    Some(existing) if existing.degradation_ms >= degradation_ms => {}
                    Some(existing) => {
                        existing.degradation_ms = degradation_ms;
                        existing.recorded_at = time;
                    }
                    None => slow.push(SlowStartupApp {
                        name,
                        friendly_name: data_string(data, "FriendlyName"),
                        degradation_ms,
                        recorded_at: time,
                    }),
                }
            }
            _ => {}
        }
    }

    slow.sort_by(|a, b| b.degradation_ms.cmp(&a.degradation_ms));
    boots.truncate(RECENT_BOOTS);
    BootPerformance {
        status: if boots.is_empty() && slow.is_empty() { "no_events" } else { "ok" }.to_string(),
        last_boot: boots.first().cloned(),
        recent_boots: boots,
        slow_apps: slow,
    }
}

#[cfg(windows)]
fn read_events() -> Result<Vec<Value>, String> {
    use crate::process_ext::CommandExt;
    use std::process::Command;

    // "No events were found" vira lista vazia; qualquer outro erro (acesso
    // negado, log desligado) volta como erro para a tela dizer o motivo.
    let script = format!(
        "$ErrorActionPreference='Stop'; try {{ \
           $events = Get-WinEvent -FilterHashtable @{{LogName='Microsoft-Windows-Diagnostics-Performance/Operational'; Id=100,101; StartTime=(Get-Date).AddDays(-{LOOKBACK_DAYS})}} -MaxEvents 200; \
           $out = foreach ($e in $events) {{ $x = [xml]$e.ToXml(); $d = @{{}}; foreach ($n in $x.Event.EventData.Data) {{ $d[$n.Name] = $n.'#text' }}; [pscustomobject]@{{ id = $e.Id; time = $e.TimeCreated.ToUniversalTime().ToString('o'); data = $d }} }}; \
           ConvertTo-Json -InputObject @($out) -Depth 4 -Compress \
         }} catch {{ if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') {{ '[]' }} else {{ throw }} }}"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .no_window()
        .output()
        .map_err(|error| format!("Nao foi possivel ler o log de boot: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Nao foi possivel ler o log de boot: {}", stderr.trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(values)) => Ok(values),
        Ok(value) => Ok(vec![value]),
        Err(error) => Err(format!("Resposta do log de boot invalida: {error}")),
    }
}

#[cfg(not(windows))]
fn read_events() -> Result<Vec<Value>, String> {
    Err("Tempo de boot disponivel apenas no Windows.".to_string())
}

pub async fn read_boot_performance(_payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(read_events).await {
        Ok(Ok(events)) => {
            let performance = parse_boot_events(&events);
            ExecutionResult::ok(
                "Tempo de boot lido do Windows.",
                serde_json::to_value(&performance).unwrap_or(Value::Null),
            )
        }
        Ok(Err(error)) => ExecutionResult {
            success: false,
            message: error,
            details: json!({ "implemented": true, "status": "unavailable" }),
        },
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao ler o tempo de boot: {error}"),
            details: json!({ "implemented": true, "status": "unavailable" }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: u64, time: &str, data: Value) -> Value {
        json!({ "id": id, "time": time, "data": data })
    }

    #[test]
    fn reads_boot_times_newest_first() {
        let result = parse_boot_events(&[
            event(100, "2026-11-04T08:00:00Z", json!({ "BootTime": "34120", "MainPathBootTime": "21000", "BootPostBootTime": "13120" })),
            event(100, "2026-11-03T08:00:00Z", json!({ "BootTime": "29000" })),
        ]);
        assert_eq!(result.status, "ok");
        let last = result.last_boot.expect("last boot");
        assert_eq!(last.boot_ms, 34120);
        assert_eq!(last.main_path_ms, Some(21000));
        assert_eq!(last.post_boot_ms, Some(13120));
        assert_eq!(result.recent_boots.len(), 2);
    }

    #[test]
    fn keeps_the_worst_delay_per_app_sorted_by_delay() {
        let result = parse_boot_events(&[
            event(101, "2026-11-04T08:00:00Z", json!({ "Name": "Teams.exe", "FriendlyName": "Microsoft Teams", "DegradationTime": "3200" })),
            event(101, "2026-11-03T08:00:00Z", json!({ "Name": "teams.exe", "DegradationTime": "4100" })),
            event(101, "2026-11-02T08:00:00Z", json!({ "Name": "Spotify.exe", "DegradationTime": "1900" })),
            event(101, "2026-11-02T08:00:00Z", json!({ "Name": "Zero.exe", "DegradationTime": "0" })),
        ]);
        assert_eq!(result.slow_apps.len(), 2);
        assert_eq!(result.slow_apps[0].name, "Teams.exe");
        assert_eq!(result.slow_apps[0].degradation_ms, 4100);
        assert_eq!(result.slow_apps[0].friendly_name.as_deref(), Some("Microsoft Teams"));
        assert_eq!(result.slow_apps[1].name, "Spotify.exe");
    }

    #[test]
    fn no_events_is_reported_as_such() {
        let result = parse_boot_events(&[]);
        assert_eq!(result.status, "no_events");
        assert!(result.last_boot.is_none());
    }

    #[test]
    fn boot_read_goes_through_the_helper_and_changes_nothing() {
        let profile = super::super::safety::command_profile("READ_BOOT_PERFORMANCE").expect("profile");
        assert!(profile.requires_privileged_helper);
        assert!(!profile.requires_snapshot);
        assert_eq!(profile.risk, super::super::safety::RiskLevel::Safe);
    }
}
