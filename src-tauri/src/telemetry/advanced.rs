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
    #[serde(default)]
    pub shell_crashes: Vec<ShellCrash>,
    pub driver_inventory: Vec<DriverInfo>,
    pub thermal_throttling_suspected: Option<bool>,
    /// The GPU driver Windows itself considers "the" display adapter's
    /// driver (Win32_VideoController), not just any DISPLAY-class entry
    /// from driver_inventory above - picked by matching gpu_name_hint when
    /// available, otherwise the controller with the most VRAM (same
    /// heuristic as TelemetryCollector::primary_gpu). None whenever no
    /// video controller answers the query at all, never a guess.
    pub gpu_driver_status: Option<GpuDriverStatus>,
    /// Per-candidate "does this person ever actually use this Windows
    /// service" signal (see `optimizations::service_usage`) - a plain
    /// snapshot with no local decision-making, so the server can decide
    /// when/how to turn it into a pause-suggestion insight, the same way
    /// every other insight rule works off uploaded signals.
    #[serde(default)]
    pub service_usage_signals: Vec<crate::optimizations::service_usage::ServiceUsageStatus>,
    /// Whether Xbox Game Bar's background Game DVR recording
    /// (HKCU\System\GameConfigStore\GameDVR_Enabled) is currently on -
    /// found via a cross-user production audit correlating with
    /// Microsoft-Windows-DistributedCOM event 10010 timeouts naming Game
    /// Bar/BcastDVR components (Windows' own broadcast/capture service
    /// failing to start in time). `None` when the value has never been
    /// explicitly set (key/value absent - Windows applies its own
    /// platform default in that case, which this deliberately doesn't
    /// guess at) or on non-Windows.
    pub game_dvr_enabled: Option<bool>,
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
    /// How many times this exact (provider, event_id) fired in the window.
    /// The sample used to be the newest N raw events, which a single chatty
    /// error drowns out - one real machine had 82 of its 95 daily errors
    /// coming from one repeating Game DVR DCOM timeout, so five raw slots
    /// showed that timeout five times and nothing else. Grouping keeps one
    /// row per kind and puts the repetition in the count instead.
    #[serde(default)]
    pub count: Option<u32>,
}

/// A crash of the Windows shell, or of AnalystBlaze itself.
///
/// Deliberately not "recent crashes on this PC". The Application log names
/// every application that crashes, which paints a far more intimate picture
/// than the System log we already read - on one real machine it would have
/// reported the user's browser, CAD software, Java runtime and RGB utility.
/// None of that helps answer the only question worth asking here ("is the
/// Windows shell broken?"), so `SHELL_CRASH_ALLOWLIST` decides what may
/// leave the machine, the same curated-allowlist principle used for
/// pausable services and protected apps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShellCrash {
    /// Always one of `SHELL_CRASH_ALLOWLIST` - never an arbitrary process.
    pub process: String,
    /// 1000 = Application Error (crash), 1002 = Application Hang.
    pub event_id: Option<u32>,
    /// Faulting module, and only for our own binary: for a Microsoft shell
    /// process this is often a third-party shell extension, which would say
    /// what the user has installed. For AnalystBlaze it is the whole point -
    /// it is what identified nvml.dll as the module our own crashes land in.
    pub module: Option<String>,
    pub count: u32,
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
    collect_shell_crashes(&mut telemetry);
    collect_driver_inventory(&mut telemetry);
    telemetry.gpu_driver_status = collect_gpu_driver_status(gpu_name_hint);
    // Cheap registry reads (no WMI/PowerShell child process) - safe to run
    // on every refresh of this already-throttled (300s) block rather than
    // needing its own cache.
    telemetry.service_usage_signals = crate::optimizations::service_usage::current_signals();
    telemetry.game_dvr_enabled = game_dvr_enabled();
    crate::optimizations::shadow_storage::set_volsnap_detected(
        crate::optimizations::shadow_storage::volsnap_storage_limited(
            &telemetry.latest_event_log_errors,
        ),
    );

    telemetry
}

#[cfg(windows)]
fn game_dvr_enabled() -> Option<bool> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("System\\GameConfigStore").ok()?;
    let value: u32 = key.get_value("GameDVR_Enabled").ok()?;
    Some(value != 0)
}

#[cfg(not(windows))]
fn game_dvr_enabled() -> Option<bool> {
    None
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

/// Windows Update failures that are loud, harmless and self-resolving. They
/// are matched by hex result code because the surrounding message is
/// localized - the same failure reads differently on a pt-BR and an en-US
/// machine, so matching prose would silently stop working per locale.
///
/// - `0x80073D02`: a Store app (WhatsApp, Office Hub) could not update
///   because it was running. By far the loudest signal across the fleet -
///   over six thousand occurrences - and it means nothing is wrong.
/// - `0x8024200B`: OEM driver update (Lenovo's, in practice) failed.
/// - `0x80240016`: a Defender definition update collided with another
///   install already in progress.
///
/// Everything else is kept, including the CBS/servicing codes (`0x800F....`,
/// `0x800704C7`) that indicate a genuinely broken Windows update.
const EVENT_LOG_NOISE_CODES: &[&str] = &["0x80073D02", "0x8024200B", "0x80240016"];

/// Ceiling for the 24h critical-error count. The old value was 50, which
/// this metric reached constantly: 11623 stored samples sat at exactly 50
/// and none above, because `-MaxEvents 50` capped the count itself rather
/// than the machine having exactly 50 errors. A machine with 50 problems and
/// one with 5000 were indistinguishable in the data. Raising it costs
/// nothing measurable (116ms vs 118ms on a machine with 95) and the cap only
/// exists so a pathological log cannot stall the collector - there is no
/// timeout around the PowerShell call.
const EVENT_LOG_COUNT_CAP: u32 = 5000;

fn collect_event_log(telemetry: &mut AdvancedTelemetry) {
    let Some(output) = powershell_text(&format!(
        "$count=(Get-WinEvent -FilterHashtable @{{LogName='System'; Level=1,2; StartTime=(Get-Date).AddHours(-24)}} -MaxEvents {EVENT_LOG_COUNT_CAP} -ErrorAction SilentlyContinue | Measure-Object).Count; [string]$count",
    )) else {
        return;
    };

    telemetry.event_log_critical_errors_24h = output.trim().parse::<u32>().ok();

    let noise = EVENT_LOG_NOISE_CODES
        .iter()
        .map(|code| format!("'{code}'"))
        .collect::<Vec<_>>()
        .join(",");
    let Some(values) = powershell_json_array(&format!(
        "$noise=@({noise}); \
         Get-WinEvent -FilterHashtable @{{LogName='System'; Level=1,2; StartTime=(Get-Date).AddHours(-24)}} -MaxEvents 500 -ErrorAction SilentlyContinue \
         | Where-Object {{ $m=$_.Message; -not ($noise | Where-Object {{ $m -like \"*$_*\" }}) }} \
         | Group-Object ProviderName,Id | Sort-Object Count -Descending | Select-Object -First 8 \
         | ForEach-Object {{ [pscustomobject]@{{ ProviderName=$_.Group[0].ProviderName; Id=$_.Group[0].Id; Count=$_.Count; Message=$_.Group[0].Message }} }} \
         | ConvertTo-Json -Compress",
    )) else {
        return;
    };

    telemetry.latest_event_log_errors = values
        .into_iter()
        .take(8)
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
            // 160 chars: every hex result code in the fleet's messages ends
            // by char 139 and every KB number by char 153, so this keeps the
            // part the server actually parses while trimming the localized
            // boilerplate that follows. Going from 5 raw rows to 8 grouped
            // rows plus this trim leaves the `details` payload slightly
            // smaller than before, which matters - the server rejects the
            // whole batch if `details` exceeds 8000 chars.
            message: value
                .get("Message")
                .and_then(Value::as_str)
                .map(|value| value.chars().take(160).collect::<String>()),
            count: value
                .get("Count")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
        })
        .collect();
}

/// The only processes whose crashes may be reported. Every entry is a
/// Microsoft-shipped piece of the Windows shell - the taskbar, Start menu,
/// search, the compositor - plus AnalystBlaze itself. A crash here is what a
/// user experiences as "the taskbar broke"; a crash in anything else is the
/// user's business, not ours.
const SHELL_CRASH_ALLOWLIST: &[&str] = &[
    "explorer.exe",
    "ShellExperienceHost.exe",
    "StartMenuExperienceHost.exe",
    "SearchHost.exe",
    "sihost.exe",
    "dwm.exe",
    "RuntimeBroker.exe",
    "TextInputHost.exe",
    "ShellHost.exe",
];

/// Our own binary. Its crashes are ours to diagnose, so unlike the shell
/// entries above it also reports the faulting module.
const SELF_PROCESS_NAME: &str = "analystblaze-desktop.exe";

fn collect_shell_crashes(telemetry: &mut AdvancedTelemetry) {
    let allow = SHELL_CRASH_ALLOWLIST
        .iter()
        .chain(std::iter::once(&SELF_PROCESS_NAME))
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(",");

    // Properties[0] is the faulting application for both Application Error
    // (1000) and Application Hang (1002); Properties[3] is the faulting
    // module, which only event 1000 carries.
    let Some(values) = powershell_json_array(&format!(
        "$allow=@({allow}); $self='{SELF_PROCESS_NAME}'; \
         Get-WinEvent -FilterHashtable @{{LogName='Application'; ProviderName=@('Application Error','Application Hang'); StartTime=(Get-Date).AddDays(-7)}} -MaxEvents 300 -ErrorAction SilentlyContinue \
         | ForEach-Object {{ $p=[string]$_.Properties[0].Value; [pscustomobject]@{{ Proc=$p; Id=$_.Id; Mod=$(if($_.Id -eq 1000 -and $p -eq $self -and $_.Properties.Count -gt 3){{[string]$_.Properties[3].Value}}else{{$null}}) }} }} \
         | Where-Object {{ $allow -contains $_.Proc }} \
         | Group-Object Proc,Id,Mod | Sort-Object Count -Descending | Select-Object -First 10 \
         | ForEach-Object {{ [pscustomobject]@{{ Process=$_.Group[0].Proc; Id=$_.Group[0].Id; Module=$_.Group[0].Mod; Count=$_.Count }} }} \
         | ConvertTo-Json -Compress",
    )) else {
        return;
    };

    telemetry.shell_crashes = values
        .into_iter()
        .filter_map(|value| {
            let process = clean_string(value.get("Process").and_then(Value::as_str)?);
            // Belt and braces: the allowlist is enforced in PowerShell, but a
            // process name is the one field here that must never be something
            // we did not choose, so it is checked again on this side.
            if !SHELL_CRASH_ALLOWLIST.contains(&process.as_str()) && process != SELF_PROCESS_NAME {
                return None;
            }
            Some(ShellCrash {
                module: (process == SELF_PROCESS_NAME)
                    .then(|| value.get("Module").and_then(Value::as_str).map(clean_string))
                    .flatten(),
                process,
                event_id: value
                    .get("Id")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32),
                count: value
                    .get("Count")
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as u32,
            })
        })
        .collect();
}

#[cfg(test)]
mod shell_crash_tests {
    use super::*;

    #[test]
    fn allowlist_holds_only_windows_shell_binaries_and_our_own() {
        // A regression here is a privacy regression, not a bug: anything
        // added to this list starts being reported from every user's
        // Application log. Third-party names must never appear.
        for name in SHELL_CRASH_ALLOWLIST {
            assert!(
                name.ends_with(".exe"),
                "{name} is not an executable name"
            );
        }
        assert!(SHELL_CRASH_ALLOWLIST.contains(&"explorer.exe"));
        assert!(
            !SHELL_CRASH_ALLOWLIST.contains(&SELF_PROCESS_NAME),
            "our own binary is handled separately - it is the only entry allowed to report a module"
        );
        for forbidden in ["opera.exe", "chrome.exe", "3dsmax.exe", "javaw.exe"] {
            assert!(
                !SHELL_CRASH_ALLOWLIST.contains(&forbidden),
                "{forbidden} must never be collected"
            );
        }
    }

    #[test]
    fn noise_codes_exclude_store_updates_but_keep_servicing_failures() {
        assert!(EVENT_LOG_NOISE_CODES.contains(&"0x80073D02"));
        // These are what a genuinely broken Windows update looks like -
        // filtering them would hide the exact problem this data exists for.
        for real in ["0x800F0841", "0x800704C7", "0x800F0823", "0x800F0845"] {
            assert!(
                !EVENT_LOG_NOISE_CODES.contains(&real),
                "{real} is a servicing failure and must stay visible"
            );
        }
    }

    #[test]
    fn critical_error_count_is_no_longer_capped_at_the_old_saturation_point() {
        // 11623 stored samples sat at exactly 50 because the old cap was the
        // count; anything at or below that is measuring the cap, not the PC.
        assert!(
            EVENT_LOG_COUNT_CAP > 50,
            "cap must exceed the value the old metric saturated at"
        );
    }
}

#[cfg(test)]
mod event_log_live_check {
    /// The two event-log scripts are assembled with `format!` and are dense
    /// with escaped braces; a mistake there makes the PowerShell fail and
    /// the collector silently return empty. Run with:
    /// cargo test --lib event_log_live_check -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_event_log_on_this_machine() {
        let mut telemetry = super::AdvancedTelemetry::default();
        super::collect_event_log(&mut telemetry);
        println!(
            "event_log_critical_errors_24h = {:?}",
            telemetry.event_log_critical_errors_24h
        );
        for issue in &telemetry.latest_event_log_errors {
            println!(
                "  {:>4}x  {} / {:?}",
                issue.count.unwrap_or(0),
                issue.provider.as_deref().unwrap_or("?"),
                issue.event_id
            );
        }
        assert!(
            telemetry.event_log_critical_errors_24h.is_some(),
            "count script failed - check the format! brace escaping"
        );
        assert!(
            !telemetry.latest_event_log_errors.is_empty(),
            "sample script returned nothing - check the format! brace escaping"
        );
        assert!(
            telemetry
                .latest_event_log_errors
                .iter()
                .all(|issue| issue.count.is_some()),
            "grouping lost the count field"
        );
    }
}

#[cfg(test)]
mod shell_crash_live_check {
    /// Prints what this machine would actually report. Run with:
    /// cargo test --lib shell_crash_live_check -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_shell_crashes_on_this_machine() {
        let mut telemetry = super::AdvancedTelemetry::default();
        super::collect_shell_crashes(&mut telemetry);
        println!("shell_crashes = {:#?}", telemetry.shell_crashes);
        for crash in &telemetry.shell_crashes {
            assert!(
                super::SHELL_CRASH_ALLOWLIST.contains(&crash.process.as_str())
                    || crash.process == super::SELF_PROCESS_NAME,
                "leaked a process outside the allowlist: {}",
                crash.process
            );
            if crash.process != super::SELF_PROCESS_NAME {
                assert!(
                    crash.module.is_none(),
                    "module must only be reported for our own binary"
                );
            }
        }
    }
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

#[cfg(test)]
mod game_dvr_live_check {
    #[test]
    #[ignore] // machine-dependent - run manually with --ignored
    fn live_game_dvr_enabled_on_this_machine() {
        println!("game_dvr_enabled() = {:?}", super::game_dvr_enabled());
    }
}
