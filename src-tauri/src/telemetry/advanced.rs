use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdvancedTelemetry {
    pub battery_percent: Option<f64>,
    pub battery_status: Option<String>,
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
    pub source: String,
    pub refreshed_at: Option<i64>,
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

pub fn collect_advanced_telemetry() -> AdvancedTelemetry {
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

    telemetry
}

fn collect_battery(telemetry: &mut AdvancedTelemetry) {
    let Some(value) = powershell_json(
        "Get-CimInstance Win32_Battery | Select-Object -First 1 EstimatedChargeRemaining,BatteryStatus | ConvertTo-Json -Compress",
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
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).to_string())
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
