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
    #[serde(default)]
    pub failing_services: Vec<FailingService>,
    pub driver_inventory: Vec<DriverInfo>,
    /// Devices Windows itself has flagged with a real Device Manager
    /// problem code (Code 10, 43, ...) - a real user's Intel Bluetooth
    /// radio showing Code 10 (device cannot start) is what this was built
    /// from. See collect_failing_devices() for exactly which codes count.
    #[serde(default)]
    pub failing_devices: Vec<FailingDevice>,
    /// Two of the failing_devices above plus a count of hidden "Not
    /// Present" mobile-broadband interfaces, correlated into the single
    /// fault they mean together - see telemetry::modem_link. None unless
    /// the specific USB-placeholder device that triggers it is present.
    #[serde(default)]
    pub modem_usb_link_issue: Option<super::modem_link::ModemUsbLinkIssue>,
    /// What Defender's own log, its scan preferences, NTFS/disk errors and
    /// SMART say - gathered only while `disk_activity` reports Defender
    /// keeping a disk saturated (see collect_defender_disk_issue), never on
    /// a healthy machine. Counts and settings only, no paths.
    #[serde(default)]
    pub defender_disk_evidence: Option<DefenderDiskEvidence>,
    /// The single named cause behind "Defender is reading the disk
    /// non-stop", from the evidence above - see telemetry::defender_disk.
    #[serde(default)]
    pub defender_disk_issue: Option<super::defender_disk::DefenderDiskIssue>,
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
    /// Set only while a game has bounced (opened and closed within seconds)
    /// at least twice in the last 15 minutes AND a known cause was found on
    /// this machine - see optimizations::game_launch. Categorical only: no
    /// process name or path is ever included.
    #[serde(default)]
    pub game_launch_issue: Option<crate::optimizations::game_launch::GameLaunchIssue>,
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

/// A Windows service that keeps failing - Service Control Manager kept
/// logging it in the System event log over the last week.
///
/// `name` is the service's display name with any trailing per-session suffix
/// stripped ("MessagingService_12d159" -> "MessagingService"). Only the name,
/// the failure kind and how many times it happened leave the machine - never
/// the error text, which can name paths. Per-session user services that fail
/// transiently by design, and the Game DVR user services (already covered by
/// the Game DVR card), are filtered out - see `FAILING_SERVICE_NOISE`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailingService {
    pub name: String,
    /// "crashing" (started, then died - SCM 7031/7034/7023),
    /// "wont_start" (failed to start at all - SCM 7000),
    /// "start_timeout" (took too long to report ready - SCM 7009).
    pub kind: String,
    pub event_id: Option<u32>,
    /// Occurrences in the ~7-day window.
    pub count: u32,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailingDevice {
    pub name: Option<String>,
    pub device_id: String,
    pub device_class: Option<String>,
    /// Win32_PnPEntity's ConfigManagerErrorCode - see PROBLEM_DEVICE_CODES
    /// below for which values this is ever populated with.
    pub problem_code: u32,
}

/// Disk-provider error events (`disk` 7/11/51/153, `storahci`/`stornvme`
/// 129) in the last 7 days, per physical disk number as the event message
/// names it (`\Device\HarddiskN`, "for Disk N"). Per disk on purpose: the
/// dev machine logs 96 paging errors a week on Harddisk3 - a removable
/// drive - which must not indict the internal SSD under scan pressure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskErrorEvents {
    /// None when the message named no disk.
    pub disk_number: Option<u32>,
    pub count: u32,
}

/// See telemetry::defender_disk for what each of these decides. All
/// event counts are from `Microsoft-Windows-Windows Defender/Operational`
/// (24h) or the System log (7d); preferences are Get-MpPreference /
/// Get-MpComputerStatus, both readable unelevated.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DefenderDiskEvidence {
    pub collected_at: i64,
    /// 1000 / 1001 / 1002 / 1005.
    pub scans_started_24h: u32,
    pub scans_finished_24h: u32,
    pub scans_cancelled_24h: u32,
    pub scans_failed_24h: u32,
    /// 2001 (definitions update failed).
    pub signature_update_failures_24h: u32,
    /// 3002 (real-time protection failed) + 5008 (engine failure).
    pub engine_failures_24h: u32,
    /// 1116 (threat detected).
    pub detections_24h: u32,
    /// 1008 / 1118 / 1119 (action on a threat failed).
    pub remediation_failures_24h: u32,
    pub scan_avg_cpu_load_factor: Option<u32>,
    pub disable_cpu_throttle_on_idle_scans: Option<bool>,
    pub scan_only_if_idle: Option<bool>,
    /// 0 = every day, 1-7 = Sunday..Saturday, 8 = never.
    pub scan_schedule_day: Option<u32>,
    pub scan_schedule_hour: Option<u32>,
    /// 1 = quick, 2 = full.
    pub scan_parameters: Option<u32>,
    pub exclusion_path_count: Option<u32>,
    /// None when never run (Windows reports 4294967295 for that).
    pub quick_scan_age_days: Option<u32>,
    pub full_scan_age_days: Option<u32>,
    pub signature_age_days: Option<u32>,
    pub am_running_mode: Option<String>,
    /// Drive letters of fixed volumes whose Get-Volume HealthStatus is not
    /// Healthy.
    pub unhealthy_volumes: Vec<String>,
    /// Ntfs 55 (structure corrupt) + 130 (repaired - so it was corrupt) in
    /// 7 days. Ntfs 98 is deliberately excluded: it is the routine "volume
    /// is healthy" line logged at every mount.
    pub ntfs_corruption_events_7d: u32,
    pub disk_error_events_7d: Vec<DiskErrorEvents>,
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
    collect_failing_services(&mut telemetry);
    collect_driver_inventory(&mut telemetry);
    collect_failing_devices(&mut telemetry);
    collect_modem_usb_link_issue(&mut telemetry);
    collect_defender_disk_issue(&mut telemetry);
    telemetry.gpu_driver_status = collect_gpu_driver_status(gpu_name_hint);
    // Cheap registry reads (no WMI/PowerShell child process) - safe to run
    // on every refresh of this already-throttled (300s) block rather than
    // needing its own cache.
    telemetry.service_usage_signals = crate::optimizations::service_usage::current_signals();
    telemetry.game_dvr_enabled = game_dvr_enabled();
    telemetry.game_launch_issue = crate::optimizations::game_launch::current_issue();
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

/// SCM event IDs that mean a service is not staying up, mapped to the plain
/// failure kind reported in `FailingService::kind`.
const SERVICE_FAILURE_EVENTS: &[(u32, &str)] = &[
    (7031, "crashing"),      // terminated unexpectedly (with recovery action)
    (7034, "crashing"),      // terminated unexpectedly
    (7023, "crashing"),      // terminated with an error
    (7000, "wont_start"),    // failed to start
    (7009, "start_timeout"), // timed out waiting for the service to report ready
];

/// Service names (matched case-insensitively as a prefix, after the trailing
/// per-session suffix is stripped) that fail transiently by design and are
/// not worth surfacing:
///   - the per-user "…UserSvc"/"…Svc_<rand>" services Windows spins up per
///     sign-in and tears down again, which routinely log a start failure
///     during the race and recover on their own;
///   - the Game DVR user services, whose failures are already explained by
///     the Game DVR insight card.
const FAILING_SERVICE_NOISE: &[&str] = &[
    "MessagingService",
    "OneSyncSvc",
    "CDPUserSvc",
    "PimIndexMaintenanceSvc",
    "UnistoreSvc",
    "UserDataSvc",
    "WpnUserService",
    "cbdhsvc",
    "BluetoothUserService",
    "CaptureService",
    "DevicesFlowUserSvc",
    "PrintWorkflowUserSvc",
    "ConsentUxUserSvc",
    "CredentialEnrollmentManagerUserSvc",
    "DeviceAssociationBrokerSvc",
    "BcastDVRUserService",
];

fn is_noise_service(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if lower.contains("gamedvr") || lower.contains("game dvr") || lower.contains("bcastdvr") {
        return true;
    }
    FAILING_SERVICE_NOISE
        .iter()
        .any(|noise| lower.starts_with(&noise.to_ascii_lowercase()))
}

fn collect_failing_services(telemetry: &mut AdvancedTelemetry) {
    let kind_map = SERVICE_FAILURE_EVENTS
        .iter()
        .map(|(id, kind)| format!("{id}='{kind}'"))
        .collect::<Vec<_>>()
        .join(";");
    let ids = SERVICE_FAILURE_EVENTS
        .iter()
        .map(|(id, _)| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    // The service name lives in different Properties slots per event: [1] for
    // 7009 (the wait-timeout, whose [0] is the timeout in ms), [0] for the
    // rest. A trailing "_<hex/random>" is a per-session instance suffix and
    // is stripped so instances of one service group together.
    let Some(values) = powershell_json_array(&format!(
        "$kind=@{{{kind_map}}}; \
         Get-WinEvent -FilterHashtable @{{LogName='System'; ProviderName='Service Control Manager'; Id=@({ids}); StartTime=(Get-Date).AddDays(-7)}} -MaxEvents 400 -ErrorAction SilentlyContinue \
         | ForEach-Object {{ $n=if($_.Id -eq 7009){{[string]$_.Properties[1].Value}}else{{[string]$_.Properties[0].Value}}; \
             [pscustomobject]@{{ Name=($n -replace '_[0-9A-Fa-f]{{3,}}$',''); Kind=$kind[$_.Id]; Id=$_.Id }} }} \
         | Where-Object {{ $_.Name -and $_.Kind }} \
         | Group-Object Name,Kind,Id | Sort-Object Count -Descending | Select-Object -First 10 \
         | ForEach-Object {{ [pscustomobject]@{{ Name=$_.Group[0].Name; Kind=$_.Group[0].Kind; Id=$_.Group[0].Id; Count=$_.Count }} }} \
         | ConvertTo-Json -Compress",
    )) else {
        return;
    };

    telemetry.failing_services = values
        .into_iter()
        .filter_map(|value| {
            let name = clean_string(value.get("Name").and_then(Value::as_str)?);
            let count = value.get("Count").and_then(Value::as_u64).unwrap_or(1) as u32;
            // The card is about a service that *keeps* failing; a single
            // one-off is not a pattern and would only pad the payload (which
            // is bounded - the server rejects a batch whose `details`
            // exceeds 8000 chars).
            if name.is_empty() || count < 2 || is_noise_service(&name) {
                return None;
            }
            Some(FailingService {
                // Service display names run short; the cap is only a guard
                // against a pathological one, not expected to bite.
                name: name.chars().take(64).collect(),
                kind: value
                    .get("Kind")
                    .and_then(Value::as_str)
                    .unwrap_or("crashing")
                    .to_string(),
                event_id: value.get("Id").and_then(Value::as_u64).map(|v| v as u32),
                count,
            })
        })
        .take(4)
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

    #[test]
    fn is_noise_service_filters_per_session_and_game_dvr_but_keeps_real_ones() {
        // Per-session user services (random suffix already stripped) and any
        // Game DVR service - covered elsewhere or self-healing.
        for noisy in [
            "MessagingService",
            "OneSyncSvc",
            "cbdhsvc",
            "Serviço de Usuário do GameDVR e Transmissão",
            "BcastDVRUserService",
        ] {
            assert!(is_noise_service(noisy), "{noisy} should be filtered");
        }
        // Real services a user would want to know about.
        for real in [
            "Microsoft Route Policy Service",
            "ASUS AURA SYNC lighting service",
            "Steam Client Service",
            "Autodesk CER Service",
            "vgc",
        ] {
            assert!(!is_noise_service(real), "{real} must not be filtered");
        }
    }

    #[test]
    fn every_service_failure_event_maps_to_a_known_kind() {
        for (_, kind) in SERVICE_FAILURE_EVENTS {
            assert!(
                ["crashing", "wont_start", "start_timeout"].contains(kind),
                "unknown kind {kind}"
            );
        }
    }
}

#[cfg(test)]
mod failing_services_live_check {
    /// What this machine would report. Run with:
    /// cargo test --lib failing_services_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_failing_services_on_this_machine() {
        let mut telemetry = super::AdvancedTelemetry::default();
        super::collect_failing_services(&mut telemetry);
        println!("failing_services = {:#?}", telemetry.failing_services);
        for service in &telemetry.failing_services {
            assert!(
                !super::is_noise_service(&service.name),
                "noise leaked through: {}",
                service.name
            );
            assert!(
                ["crashing", "wont_start", "start_timeout"].contains(&service.kind.as_str()),
                "bad kind: {}",
                service.kind
            );
            assert!(!service.name.is_empty(), "empty service name");
        }
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

/// Win32_PnPEntity's ConfigManagerErrorCode values worth surfacing to the
/// user as "this is broken and might be fixable" - deliberately not "any
/// non-zero code", since several codes are either intentional (22 - user
/// disabled the device on purpose) or not really a fault (24/45 - a
/// removable device that simply isn't plugged in right now shows these
/// too, and cycling it wouldn't do anything since there's nothing attached).
/// This list is the subset a disable/enable cycle or a Windows restart can
/// plausibly clear: 10 (cannot start - the real case this was built from,
/// a real user's Intel Bluetooth radio), 12 (not enough free resources),
/// 14 (needs restart), 18 (reinstall drivers), 19/39/40 (corrupted driver
/// or registry entry), 28 (drivers not installed), 31/37/38 (Windows
/// already tried and failed to start it), 43 (device reported a problem
/// and Windows stopped it), 48 (driver blocked as incompatible).
const PROBLEM_DEVICE_CODES: &[u32] = &[10, 12, 14, 18, 19, 28, 31, 37, 38, 39, 40, 43, 48];

fn collect_failing_devices(telemetry: &mut AdvancedTelemetry) {
    let Some(values) = powershell_json_array(
        "Get-CimInstance Win32_PnPEntity | Where-Object { $_.ConfigManagerErrorCode -gt 0 } | Select-Object -First 20 Name,DeviceID,PNPClass,ConfigManagerErrorCode | ConvertTo-Json -Compress",
    ) else {
        return;
    };

    telemetry.failing_devices = values
        .into_iter()
        .filter_map(|value| {
            let problem_code = value.get("ConfigManagerErrorCode").and_then(Value::as_u64)? as u32;
            if !PROBLEM_DEVICE_CODES.contains(&problem_code) {
                return None;
            }
            let device_id = value.get("DeviceID").and_then(Value::as_str)?.to_string();
            Some(FailingDevice {
                name: value.get("Name").and_then(Value::as_str).map(clean_string),
                device_id,
                device_class: value.get("PNPClass").and_then(Value::as_str).map(clean_string),
                problem_code,
            })
        })
        .take(5)
        .collect();
}

/// Only does real work when failing_devices already contains the specific
/// USB placeholder device that makes this worth checking - see
/// telemetry::modem_link for the rule and the real case behind it.
fn collect_modem_usb_link_issue(telemetry: &mut AdvancedTelemetry) {
    use super::modem_link;

    telemetry.modem_usb_link_issue = modem_link::detect(
        &telemetry.failing_devices,
        usb_link_details,
        ghost_wwan_adapter_count,
    );
}

/// Only spends the ~4s of PowerShell below once `disk_activity` has seen
/// Defender keep a disk saturated for a sustained stretch - the same
/// "healthy machines pay nothing" rule as the modem detector. The
/// collector forces this block to refresh early when that flips (see
/// TelemetryCollector::advanced_telemetry), so the card does not wait out
/// the full 300s cache on top of the 5 minutes of pressure it already
/// needed.
fn collect_defender_disk_issue(telemetry: &mut AdvancedTelemetry) {
    use super::{defender_disk, disk_activity};

    let Some(activity) = disk_activity::latest() else {
        return;
    };
    if !activity.pressure.sustained {
        return;
    }
    let Some(evidence) = defender_disk_evidence() else {
        return;
    };
    telemetry.defender_disk_issue =
        defender_disk::classify(&activity, &evidence, telemetry.disk_predict_failure);
    telemetry.defender_disk_evidence = Some(evidence);
}

/// One PowerShell pass over everything defender_disk::classify needs.
/// Verified unelevated on the dev machine (both event logs, Get-MpPreference,
/// Get-MpComputerStatus and Get-Volume all answer without admin); ~4s.
pub(crate) fn defender_disk_evidence() -> Option<DefenderDiskEvidence> {
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$since24 = (Get-Date).AddHours(-24)
$since7d = (Get-Date).AddDays(-7)
$def = Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-Windows Defender/Operational'; StartTime=$since24} -MaxEvents 3000 -ErrorAction SilentlyContinue
$ids = @{}
foreach ($e in $def) { $k = [string]$e.Id; if ($ids.ContainsKey($k)) { $ids[$k]++ } else { $ids[$k] = 1 } }
$sys = Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName=@('Ntfs','Microsoft-Windows-Ntfs','disk','Disk','storahci','stornvme'); StartTime=$since7d} -MaxEvents 3000 -ErrorAction SilentlyContinue
$ntfs = 0
$diskErrors = @{}
foreach ($e in $sys) {
  $p = $e.ProviderName
  if ($p -like '*Ntfs*') { if ($e.Id -eq 55 -or $e.Id -eq 130) { $ntfs++ }; continue }
  $isDiskError = ($p -ieq 'disk' -and ($e.Id -in 7,11,51,153)) -or (($p -eq 'storahci' -or $p -eq 'stornvme') -and $e.Id -eq 129)
  if (-not $isDiskError) { continue }
  $n = ''
  if ($e.Message -match 'Harddisk(\d+)') { $n = $Matches[1] } elseif ($e.Message -match '(?i)for Disk (\d+)') { $n = $Matches[1] } elseif ($e.Message -match 'RaidPort(\d+)') { $n = $Matches[1] }
  if ($diskErrors.ContainsKey($n)) { $diskErrors[$n]++ } else { $diskErrors[$n] = 1 }
}
$pref = Get-MpPreference | Select-Object ScanAvgCPULoadFactor, DisableCpuThrottleOnIdleScans, ScanOnlyIfIdleEnabled, ScanScheduleDay, @{n='ScanScheduleHour';e={ $_.ScanScheduleTime.Hours }}, ScanParameters, @{n='ExclusionPathCount';e={ @($_.ExclusionPath).Count }}
$status = Get-MpComputerStatus | Select-Object QuickScanAge, FullScanAge, AntivirusSignatureAge, AMRunningMode
$vols = @(Get-Volume | Where-Object { $_.DriveType -eq 'Fixed' -and $_.DriveLetter -and $_.HealthStatus -ne 'Healthy' } | ForEach-Object { [string]$_.DriveLetter })
[pscustomobject]@{ defender_events = $ids; ntfs_corruption = $ntfs; disk_errors = $diskErrors; preferences = $pref; status = $status; unhealthy_volumes = $vols } | ConvertTo-Json -Compress -Depth 4
"#;

    let value = powershell_json(SCRIPT)?;
    let events = value.get("defender_events");
    let count = |id: &str| -> u32 {
        events
            .and_then(|map| map.get(id))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
    };
    let preferences = value.get("preferences");
    let status = value.get("status");
    let pref_u32 = |key: &str| {
        preferences
            .and_then(|p| p.get(key))
            .and_then(Value::as_u64)
            .map(|v| v as u32)
    };
    let pref_bool = |key: &str| preferences.and_then(|p| p.get(key)).and_then(Value::as_bool);
    // Windows reports "never" as u32::MAX.
    let age = |key: &str| {
        status
            .and_then(|s| s.get(key))
            .and_then(Value::as_u64)
            .filter(|days| *days < u32::MAX as u64)
            .map(|days| days as u32)
    };

    let mut disk_error_events_7d: Vec<DiskErrorEvents> = value
        .get("disk_errors")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(disk, count)| DiskErrorEvents {
                    disk_number: disk.parse().ok(),
                    count: count.as_u64().unwrap_or(0) as u32,
                })
                .collect()
        })
        .unwrap_or_default();
    disk_error_events_7d.sort_by_key(|entry| entry.disk_number);

    Some(DefenderDiskEvidence {
        collected_at: chrono::Utc::now().timestamp(),
        scans_started_24h: count("1000"),
        scans_finished_24h: count("1001"),
        scans_cancelled_24h: count("1002"),
        scans_failed_24h: count("1005"),
        signature_update_failures_24h: count("2001"),
        engine_failures_24h: count("3002") + count("5008"),
        detections_24h: count("1116"),
        remediation_failures_24h: count("1008") + count("1118") + count("1119"),
        scan_avg_cpu_load_factor: pref_u32("ScanAvgCPULoadFactor"),
        disable_cpu_throttle_on_idle_scans: pref_bool("DisableCpuThrottleOnIdleScans"),
        scan_only_if_idle: pref_bool("ScanOnlyIfIdleEnabled"),
        scan_schedule_day: pref_u32("ScanScheduleDay"),
        scan_schedule_hour: pref_u32("ScanScheduleHour"),
        scan_parameters: pref_u32("ScanParameters"),
        exclusion_path_count: pref_u32("ExclusionPathCount"),
        quick_scan_age_days: age("QuickScanAge"),
        full_scan_age_days: age("FullScanAge"),
        signature_age_days: age("AntivirusSignatureAge"),
        am_running_mode: status
            .and_then(|s| s.get("AMRunningMode"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        unhealthy_volumes: value
            .get("unhealthy_volumes")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        ntfs_corruption_events_7d: value
            .get("ntfs_corruption")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        disk_error_events_7d,
    })
}

/// Hidden "Generic Mobile Broadband Adapter" interfaces in "Not Present"
/// state - one is left behind every time a USB modem drops and
/// re-enumerates, so the count is a record of how often the link has
/// flapped. Matched on the interface description, which Windows doesn't
/// localize (the *names* are - "Celular N" on pt-BR - which is why those
/// aren't used).
fn ghost_wwan_adapter_count() -> u32 {
    powershell_json(
        "$n = @(Get-NetAdapter -IncludeHidden -ErrorAction SilentlyContinue | Where-Object { $_.InterfaceDescription -like '*Mobile Broadband*' -and $_.Status -eq 'Not Present' }).Count; [pscustomobject]@{ Count = $n } | ConvertTo-Json -Compress",
    )
    .and_then(|value| value.get("Count").and_then(Value::as_u64))
    .unwrap_or(0) as u32
}

/// Walks two levels up the PnP tree from the failing USB device: its parent
/// (a hub - root or intermediate) and, when the parent is a root hub, the
/// host controller above it. The instance ID is passed through an
/// environment variable rather than interpolated into the script, since a
/// DeviceID contains `\` and `&` (same reason as
/// windows_actions::pnp_device_status).
fn usb_link_details(device_id: &str) -> super::modem_link::UsbLinkDetails {
    use super::modem_link::{
        hub_version_from_instance_id, is_root_hub_instance_id, usb_port_from_instance_id,
        UsbLinkDetails,
    };

    let mut details = UsbLinkDetails {
        port: usb_port_from_instance_id(device_id),
        ..UsbLinkDetails::default()
    };

    let Some(value) = powershell_json_with_env(
        r#"$id = $env:ANALYSTBLAZE_DEVICE_ID
$parent = (Get-PnpDeviceProperty -InstanceId $id -KeyName 'DEVPKEY_Device_Parent' -ErrorAction SilentlyContinue).Data
$grandName = $null
if ($parent) {
  $grand = (Get-PnpDeviceProperty -InstanceId $parent -KeyName 'DEVPKEY_Device_Parent' -ErrorAction SilentlyContinue).Data
  if ($grand) { $grandName = (Get-PnpDevice -InstanceId $grand -ErrorAction SilentlyContinue).FriendlyName }
}
[pscustomobject]@{ Parent = $parent; GrandparentName = $grandName } | ConvertTo-Json -Compress"#,
        "ANALYSTBLAZE_DEVICE_ID",
        device_id,
    ) else {
        return details;
    };

    if let Some(parent) = value.get("Parent").and_then(Value::as_str) {
        details.on_root_hub = is_root_hub_instance_id(parent);
        details.hub_version = hub_version_from_instance_id(parent);
        if details.on_root_hub {
            details.controller = value
                .get("GrandparentName")
                .and_then(Value::as_str)
                .map(clean_string);
        }
    }

    details
}

fn powershell_json_with_env(script: &str, env_key: &str, env_value: &str) -> Option<Value> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env(env_key, env_value)
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = decode_console_bytes(&output.stdout);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str(text).ok()
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
