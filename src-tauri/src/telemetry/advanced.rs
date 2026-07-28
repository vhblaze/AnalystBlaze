use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Command;

use crate::process_ext::{decode_console_bytes, CommandExt};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdvancedTelemetry {
    pub battery_percent: Option<f64>,
    pub battery_status: Option<String>,
    /// Instantaneous battery discharge rate in milliwatts, from
    /// Win32_Battery.DischargeRate. This is a real measurement of total
    /// system draw (not an estimate) whenever it's populated - but many
    /// systems report it as 0/absent even while genuinely discharging, so
    /// its absence doesn't mean the battery isn't discharging, only that
    /// this particular WMI property isn't implemented on this hardware.
    /// Only meaningful when battery_status is "discharging".
    pub battery_discharge_rate_mw: Option<f64>,
    pub disk_smart_status: Option<String>,
    pub disk_predict_failure: Option<bool>,
    pub disk_smart_devices: Vec<DiskSmartDevice>,
    pub defender_status: Option<String>,
    pub defender_realtime_enabled: Option<bool>,
    pub windows_update_reboot_pending: Option<bool>,
    pub event_log_critical_errors_24h: Option<u32>,
    pub latest_event_log_errors: Vec<EventLogIssue>,
    pub driver_inventory: Vec<DriverInfo>,
    pub thermal_throttling_suspected: Option<bool>,
    /// The GPU driver Windows itself considers "the" display adapter's
    /// driver (Win32_VideoController), not just any DISPLAY-class entry
    /// from driver_inventory above - picked by matching gpu_name_hint when
    /// available, otherwise the controller with the most VRAM (same
    /// heuristic as TelemetryCollector::primary_gpu). None whenever no
    /// video controller answers the query at all, never a guess.
    pub gpu_driver_status: Option<GpuDriverStatus>,
    pub source: String,
    pub refreshed_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuDriverStatus {
    pub device_name: Option<String>,
    pub driver_version: Option<String>,
    /// Raw DriverDate as PowerShell's ConvertTo-Json renders a [DateTime] -
    /// kept as an opaque string (same as DriverInfo.driver_date) rather than
    /// parsed in Rust; driver_age_days below is computed PowerShell-side
    /// instead, where real DateTime arithmetic is available.
    pub driver_date: Option<String>,
    pub driver_age_days: Option<i64>,
    /// True once driver_age_days crosses OUTDATED_DRIVER_AGE_DAYS. Age is
    /// only a proxy for "may be missing recent game-ready fixes/perf
    /// patches" - it is NOT a comparison against the vendor's actual latest
    /// release (that would need a live NVIDIA/AMD/Intel API call, out of
    /// scope here), so this can be wrong in either direction: a driver
    /// could be old but still the newest available for that card, or a
    /// fresh install could still be missing a same-week hotfix.
    pub possibly_outdated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiskSmartDevice {
    pub model: Option<String>,
    pub status: Option<String>,
    pub predict_failure: Option<bool>,
    pub media_type: Option<String>,
    pub size_gb: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventLogIssue {
    pub provider: Option<String>,
    pub event_id: Option<u32>,
    pub level: Option<u32>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriverInfo {
    pub device_name: Option<String>,
    pub device_class: Option<String>,
    pub driver_version: Option<String>,
    pub driver_date: Option<String>,
    pub manufacturer: Option<String>,
}

pub fn collect_advanced_telemetry(gpu_name_hint: Option<&str>) -> AdvancedTelemetry {
    let mut telemetry = AdvancedTelemetry {
        source: "windows_low_frequency".to_string(),
        refreshed_at: Some(chrono::Utc::now().timestamp()),
        ..AdvancedTelemetry::default()
    };

    collect_battery(&mut telemetry);
    collect_disk_smart(&mut telemetry);
    collect_defender(&mut telemetry);
    collect_windows_update(&mut telemetry);
    collect_event_log(&mut telemetry);
    collect_driver_inventory(&mut telemetry);
    telemetry.gpu_driver_status = collect_gpu_driver_status(gpu_name_hint);

    telemetry
}

/// Age past which a GPU driver is flagged as possibly outdated. Not a
/// vendor-sourced value - a conservative round number (most GPU vendors
/// ship at least a couple of driver updates a year) chosen so this only
/// fires on drivers that are genuinely stale, not merely a few months old.
const OUTDATED_DRIVER_AGE_DAYS: i64 = 365;

fn collect_gpu_driver_status(gpu_name_hint: Option<&str>) -> Option<GpuDriverStatus> {
    let values = powershell_json_array(
        r#"Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,DriverDate,AdapterRAM | ForEach-Object {
    $ageDays = if ($_.DriverDate) { [math]::Round(((Get-Date) - $_.DriverDate).TotalDays) } else { $null }
    [pscustomobject]@{ Name = $_.Name; DriverVersion = $_.DriverVersion; DriverDate = $_.DriverDate; DriverAgeDays = $ageDays; AdapterRAM = $_.AdapterRAM }
} | ConvertTo-Json -Compress"#,
    )?;

    let controller = select_primary_video_controller(&values, gpu_name_hint)?;
    let driver_age_days = controller.get("DriverAgeDays").and_then(Value::as_i64);

    Some(GpuDriverStatus {
        device_name: controller.get("Name").and_then(Value::as_str).map(clean_string),
        driver_version: controller
            .get("DriverVersion")
            .and_then(Value::as_str)
            .map(clean_string),
        driver_date: controller
            .get("DriverDate")
            .and_then(Value::as_str)
            .map(clean_string),
        driver_age_days,
        possibly_outdated: is_driver_possibly_outdated(driver_age_days),
    })
}

fn is_driver_possibly_outdated(driver_age_days: Option<i64>) -> bool {
    driver_age_days.is_some_and(|age| age >= OUTDATED_DRIVER_AGE_DAYS)
}

/// Prefers the entry whose Name matches gpu_name_hint (the GPU
/// TelemetryCollector::primary_gpu already picked, by max VRAM) so the
/// reported driver is for the same card the rest of telemetry talks about,
/// not just whichever WMI happened to return first. Falls back to the
/// controller with the most AdapterRAM - the same "biggest VRAM wins"
/// heuristic primary_gpu itself uses - when there's no hint or no name
/// match (e.g. WMI's Name string doesn't line up with the DXGI-sourced name
/// primary_gpu uses).
fn select_primary_video_controller(values: &[Value], gpu_name_hint: Option<&str>) -> Option<Value> {
    if let Some(hint) = gpu_name_hint {
        let hint_lower = hint.to_ascii_lowercase();
        if let Some(matched) = values.iter().find(|value| {
            value
                .get("Name")
                .and_then(Value::as_str)
                .map(|name| {
                    let name_lower = name.to_ascii_lowercase();
                    name_lower.contains(&hint_lower) || hint_lower.contains(&name_lower)
                })
                .unwrap_or(false)
        }) {
            return Some(matched.clone());
        }
    }

    values
        .iter()
        .max_by(|left, right| {
            let left_ram = left.get("AdapterRAM").and_then(Value::as_i64).unwrap_or(0);
            let right_ram = right.get("AdapterRAM").and_then(Value::as_i64).unwrap_or(0);
            left_ram.cmp(&right_ram)
        })
        .cloned()
}

fn collect_battery(telemetry: &mut AdvancedTelemetry) {
    let Some(value) = powershell_json(
        "Get-CimInstance Win32_Battery | Select-Object -First 1 EstimatedChargeRemaining,BatteryStatus,DischargeRate | ConvertTo-Json -Compress",
    ) else {
        return;
    };

    telemetry.battery_percent = value
        .get("EstimatedChargeRemaining")
        .and_then(Value::as_f64);
    telemetry.battery_status = value
        .get("BatteryStatus")
        .and_then(Value::as_i64)
        .map(battery_status_label);
    // DischargeRate is frequently 0/unpopulated even while genuinely
    // discharging (some OEMs never implement the WMI counter) - only trust
    // it as a real reading when it's positive, plausible for a laptop
    // battery (upper bound is a generous guess, not a spec value - "not
    // determined" beyond "clearly not a real discharge rate"), and we're
    // actually on battery, per the field doc on AdvancedTelemetry. A zero
    // here rolls back to None either way, which is what pushes
    // estimate_energy() down to the next tier instead of recording a
    // fabricated 0W "measurement".
    let raw_discharge_rate = value.get("DischargeRate").and_then(Value::as_f64);
    telemetry.battery_discharge_rate_mw = raw_discharge_rate
        .filter(|rate| is_plausible_discharge_rate_mw(*rate, telemetry.battery_status.as_deref()));
}

/// A generous upper bound, not a spec value - "not determined" beyond
/// "clearly not a real discharge rate" for a laptop battery. Zero and
/// negative readings are rejected the same way whether they come from an
/// OEM that never implemented the counter or genuinely aren't discharging;
/// callers only see a real, positive, discharging reading or None.
fn is_plausible_discharge_rate_mw(rate: f64, battery_status: Option<&str>) -> bool {
    (0.0..=300_000.0).contains(&rate) && rate > 0.0 && battery_status == Some("discharging")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discharge_rate_requires_a_positive_value_while_discharging() {
        assert!(is_plausible_discharge_rate_mw(45_000.0, Some("discharging")));
    }

    #[test]
    fn discharge_rate_rejects_zero_whether_charging_or_oem_unimplemented() {
        assert!(!is_plausible_discharge_rate_mw(0.0, Some("discharging")));
        assert!(!is_plausible_discharge_rate_mw(0.0, Some("charging")));
    }

    #[test]
    fn discharge_rate_is_ignored_while_plugged_in_even_if_wmi_reports_a_value() {
        // Some systems keep reporting a stale non-zero DischargeRate while charging.
        assert!(!is_plausible_discharge_rate_mw(15_000.0, Some("charging")));
        assert!(!is_plausible_discharge_rate_mw(15_000.0, Some("ac")));
    }

    #[test]
    fn discharge_rate_rejects_implausibly_large_values() {
        assert!(!is_plausible_discharge_rate_mw(2_000_000.0, Some("discharging")));
    }

    #[test]
    fn discharge_rate_rejects_negative_values() {
        assert!(!is_plausible_discharge_rate_mw(-500.0, Some("discharging")));
    }

    fn controller(name: &str, adapter_ram: i64) -> Value {
        json!({ "Name": name, "AdapterRAM": adapter_ram })
    }

    #[test]
    fn video_controller_selection_prefers_a_name_matching_the_hint() {
        let values = vec![
            controller("Intel(R) UHD Graphics", 1_073_741_824),
            controller("NVIDIA GeForce RTX 4070", 8_589_934_592),
        ];
        let selected = select_primary_video_controller(&values, Some("GeForce RTX 4070"))
            .expect("a match should be found");
        assert_eq!(
            selected.get("Name").and_then(Value::as_str),
            Some("NVIDIA GeForce RTX 4070")
        );
    }

    #[test]
    fn video_controller_selection_falls_back_to_most_vram_without_a_matching_hint() {
        let values = vec![
            controller("Intel(R) UHD Graphics", 1_073_741_824),
            controller("NVIDIA GeForce RTX 4070", 8_589_934_592),
        ];
        let selected = select_primary_video_controller(&values, Some("Some Unrelated Name"))
            .expect("should fall back instead of returning None");
        assert_eq!(
            selected.get("Name").and_then(Value::as_str),
            Some("NVIDIA GeForce RTX 4070")
        );

        let selected =
            select_primary_video_controller(&values, None).expect("no hint should still pick one");
        assert_eq!(
            selected.get("Name").and_then(Value::as_str),
            Some("NVIDIA GeForce RTX 4070")
        );
    }

    #[test]
    fn video_controller_selection_returns_none_for_an_empty_list() {
        assert!(select_primary_video_controller(&[], Some("anything")).is_none());
    }

    #[test]
    fn driver_age_below_threshold_is_not_flagged() {
        assert!(!is_driver_possibly_outdated(Some(OUTDATED_DRIVER_AGE_DAYS - 1)));
        assert!(!is_driver_possibly_outdated(None));
    }

    #[test]
    fn driver_age_at_or_above_threshold_is_flagged() {
        assert!(is_driver_possibly_outdated(Some(OUTDATED_DRIVER_AGE_DAYS)));
        assert!(is_driver_possibly_outdated(Some(OUTDATED_DRIVER_AGE_DAYS + 400)));
    }
}

fn collect_disk_smart(telemetry: &mut AdvancedTelemetry) {
    if let Some(values) = powershell_json_array(
        "Get-CimInstance Win32_DiskDrive | Select-Object Model,Status,MediaType,Size | ConvertTo-Json -Compress",
    ) {
        telemetry.disk_smart_devices = values
            .into_iter()
            .take(12)
            .map(|value| DiskSmartDevice {
                model: value.get("Model").and_then(Value::as_str).map(clean_string),
                status: value.get("Status").and_then(Value::as_str).map(clean_string),
                predict_failure: None,
                media_type: value.get("MediaType").and_then(Value::as_str).map(clean_string),
                size_gb: value.get("Size").and_then(Value::as_f64).map(bytes_to_gb),
            })
            .collect();
        telemetry.disk_smart_status = telemetry
            .disk_smart_devices
            .iter()
            .find_map(|device| device.status.clone())
            .map(|status| status.to_ascii_lowercase());
    }

    if let Some(values) = powershell_json_array(
        "Get-CimInstance -Namespace root\\wmi -Class MSStorageDriver_FailurePredictStatus | Select-Object PredictFailure | ConvertTo-Json -Compress",
    ) {
        let predict_failure = values
            .iter()
            .any(|value| value.get("PredictFailure").and_then(Value::as_bool) == Some(true));
        telemetry.disk_predict_failure = Some(predict_failure);
        if predict_failure {
            telemetry.disk_smart_status = Some("predict_failure".to_string());
        } else if telemetry.disk_smart_status.is_none() {
            telemetry.disk_smart_status = Some("ok".to_string());
        }

        for (index, value) in values.iter().enumerate() {
            if let Some(device) = telemetry.disk_smart_devices.get_mut(index) {
                device.predict_failure = value.get("PredictFailure").and_then(Value::as_bool);
            }
        }
    }
}

fn collect_defender(telemetry: &mut AdvancedTelemetry) {
    let Some(value) = powershell_json(
        "Get-MpComputerStatus | Select-Object AMServiceEnabled,AntivirusEnabled,RealTimeProtectionEnabled | ConvertTo-Json -Compress",
    ) else {
        return;
    };

    let service = value
        .get("AMServiceEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let antivirus = value
        .get("AntivirusEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let realtime = value
        .get("RealTimeProtectionEnabled")
        .and_then(Value::as_bool);

    telemetry.defender_realtime_enabled = realtime;
    telemetry.defender_status = Some(
        if service && antivirus && realtime.unwrap_or(false) {
            "healthy"
        } else if service || antivirus {
            "attention"
        } else {
            "disabled_or_unavailable"
        }
        .to_string(),
    );
}

fn collect_windows_update(telemetry: &mut AdvancedTelemetry) {
    let Some(value) = powershell_json(
        "$p1=Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired'; $p2=Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Component Based Servicing\\RebootPending'; [pscustomobject]@{RebootPending=($p1 -or $p2)} | ConvertTo-Json -Compress",
    ) else {
        return;
    };

    telemetry.windows_update_reboot_pending = value.get("RebootPending").and_then(Value::as_bool);
}

fn collect_event_log(telemetry: &mut AdvancedTelemetry) {
    let Some(output) = powershell_text(
        "$count=(Get-WinEvent -FilterHashtable @{LogName='System'; Level=1,2; StartTime=(Get-Date).AddHours(-24)} -MaxEvents 50 -ErrorAction SilentlyContinue | Measure-Object).Count; [string]$count",
    ) else {
        return;
    };

    telemetry.event_log_critical_errors_24h = output.trim().parse::<u32>().ok();

    let Some(values) = powershell_json_array(
        "Get-WinEvent -FilterHashtable @{LogName='System'; Level=1,2; StartTime=(Get-Date).AddHours(-24)} -MaxEvents 5 -ErrorAction SilentlyContinue | Select-Object ProviderName,Id,LevelDisplayName,Message | ConvertTo-Json -Compress",
    ) else {
        return;
    };

    telemetry.latest_event_log_errors = values
        .into_iter()
        .take(5)
        .map(|value| EventLogIssue {
            provider: value
                .get("ProviderName")
                .and_then(Value::as_str)
                .map(clean_string),
            event_id: value
                .get("Id")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
            level: None,
            message: value
                .get("Message")
                .and_then(Value::as_str)
                .map(|value| value.chars().take(220).collect::<String>()),
        })
        .collect();
}

fn collect_driver_inventory(telemetry: &mut AdvancedTelemetry) {
    let Some(values) = powershell_json_array(
        "Get-CimInstance Win32_PnPSignedDriver | Where-Object {$_.DeviceClass -in @('DISPLAY','NET','MEDIA')} | Select-Object -First 12 DeviceName,DeviceClass,DriverVersion,DriverDate,Manufacturer | ConvertTo-Json -Compress",
    ) else {
        return;
    };

    telemetry.driver_inventory = values
        .into_iter()
        .map(|value| DriverInfo {
            device_name: value
                .get("DeviceName")
                .and_then(Value::as_str)
                .map(clean_string),
            device_class: value
                .get("DeviceClass")
                .and_then(Value::as_str)
                .map(clean_string),
            driver_version: value
                .get("DriverVersion")
                .and_then(Value::as_str)
                .map(clean_string),
            driver_date: value
                .get("DriverDate")
                .and_then(Value::as_str)
                .map(clean_string),
            manufacturer: value
                .get("Manufacturer")
                .and_then(Value::as_str)
                .map(clean_string),
        })
        .collect();
}

fn powershell_json(script: &str) -> Option<Value> {
    let output = powershell_text(script)?;
    let output = output.trim();
    if output.is_empty() {
        return None;
    }
    serde_json::from_str(output)
        .ok()
        .or_else(|| Some(json!({})))
}

fn powershell_json_array(script: &str) -> Option<Vec<Value>> {
    match powershell_json(script)? {
        Value::Array(values) => Some(values),
        Value::Object(map) if map.is_empty() => None,
        value => Some(vec![value]),
    }
}

fn powershell_text(script: &str) -> Option<String> {
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

    Some(decode_console_bytes(&output.stdout))
}

fn clean_string(value: &str) -> String {
    value.trim().chars().take(180).collect()
}

fn bytes_to_gb(value: f64) -> f64 {
    value / 1024.0 / 1024.0 / 1024.0
}

fn battery_status_label(status: i64) -> String {
    match status {
        1 => "discharging",
        2 => "ac",
        3 => "fully_charged",
        4 => "low",
        5 => "critical",
        6 => "charging",
        7 => "charging_high",
        8 => "charging_low",
        9 => "charging_critical",
        10 => "undefined",
        11 => "partially_charged",
        _ => "unknown",
    }
    .to_string()
}
