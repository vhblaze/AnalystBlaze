//! "Windows Defender is reading the disk non-stop and it sits at 100%" -
//! one symptom, at least five different faults behind it, each with a
//! different fix. This module turns the pressure signal from
//! `disk_activity` plus the evidence `advanced::collect_defender_disk_evidence`
//! gathers (Defender's own event log, its scan preferences, NTFS and disk
//! errors, SMART) into one named cause, or honestly into "we can see the
//! pressure but not the cause".
//!
//! Ordering matters and is deliberate: hardware first, because a dying
//! disk makes *every* reader look guilty and a scan-throttle "fix" would
//! only hide it; then file-system corruption for the same reason; then
//! Defender's own failure modes, which its event log states outright; and
//! only then "it is just an unthrottled scan on a slow disk", the benign
//! case that is also the only one where slowing Defender down is the
//! right answer.
//!
//! Nothing in here or in the evidence carries a file path: Defender's
//! event log mentions the files it scans and finds things in, and which
//! files a user has is exactly the kind of thing this app does not collect
//! (same boundary as the shell-crash allowlist in advanced.rs). Counts and
//! codes only.

use serde::{Deserialize, Serialize};

use super::advanced::DefenderDiskEvidence;
use super::disk_activity::{DefenderDiskPressure, DiskActivity};

/// Disk-provider error events on the saturated disk itself, in 7 days,
/// before the disk (not Defender) is called the problem. A handful can be
/// a flaky cable or an unplugged external drive; dozens are a pattern.
pub const DISK_ERROR_EVENTS_THRESHOLD: u32 = 10;
/// Repeated detections in a day - Defender finding something, acting, and
/// finding it again - is the remediation loop, not a scan.
pub const DETECTION_LOOP_THRESHOLD: u32 = 3;
/// Failed scans in a day before "the scan cannot finish" is the call.
pub const SCAN_FAILURE_THRESHOLD: u32 = 2;
/// Windows' default `ScanAvgCPULoadFactor`; below this the user (or an
/// admin policy) already throttled scanning, so that is not the cause.
pub const DEFAULT_SCAN_CPU_LOAD_FACTOR: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefenderDiskCause {
    /// SMART predicts failure, or the saturated disk keeps logging
    /// controller/paging errors: the disk is slow because it is failing.
    DiskFailing,
    /// A volume reports unhealthy, or NTFS logged corruption: reads stall
    /// on bad structures; chkdsk, not Defender, is the fix.
    FileSystemCorruption,
    /// Defender keeps detecting and failing to remediate the same thing,
    /// so it keeps rescanning.
    ThreatRemediationLoop,
    /// Scans start and fail/never finish - typically one file Defender
    /// cannot get through (huge archive, corrupt container, cloud
    /// placeholder), retried over and over.
    ScanCannotFinish,
    /// Signature updates or the engine itself are failing; corrupt
    /// definitions are a classic cause of scan loops.
    DefinitionsOrEngineFailing,
    /// Just a scan, on a disk that cannot keep up, with Windows' default
    /// "use half the CPU, ignore idle" settings.
    UnthrottledScan,
    /// Pressure is real and sustained, but none of the evidence points
    /// anywhere - said as such, with the generic mitigations.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefenderDiskIssue {
    pub cause: DefenderDiskCause,
    pub detected_at: i64,
    /// PDH instance of the saturated disk (`1 D: E:`) and the disk number
    /// parsed out of it, which matches `\Device\HarddiskN` and
    /// Get-PhysicalDisk's DeviceId.
    pub disk: Option<String>,
    pub disk_number: Option<u32>,
    pub sustained_minutes: u64,
    pub peak_disk_active_percent: f64,
    pub peak_defender_mb_s: f64,
    /// Snapshot of the numbers the cause was decided on, so the card can
    /// quote them instead of asking the user to trust the label.
    pub disk_error_events_7d: u32,
    pub ntfs_corruption_events_7d: u32,
    pub unhealthy_volumes: Vec<String>,
    pub detections_24h: u32,
    pub remediation_failures_24h: u32,
    pub scans_started_24h: u32,
    pub scans_finished_24h: u32,
    pub scans_failed_24h: u32,
    pub signature_update_failures_24h: u32,
    pub engine_failures_24h: u32,
    pub scan_avg_cpu_load_factor: Option<u32>,
    pub disable_cpu_throttle_on_idle_scans: Option<bool>,
    pub scan_only_if_idle: Option<bool>,
    pub full_scan_age_days: Option<u32>,
}

/// `1 D: E:` -> 1. The PDH PhysicalDisk instance leads with the disk number.
pub fn disk_number_from_instance(instance: &str) -> Option<u32> {
    instance.split_whitespace().next()?.parse().ok()
}

pub fn classify(
    activity: &DiskActivity,
    evidence: &DefenderDiskEvidence,
    smart_predict_failure: Option<bool>,
) -> Option<DefenderDiskIssue> {
    let pressure: &DefenderDiskPressure = &activity.pressure;
    if !pressure.sustained {
        return None;
    }

    let disk_number = activity
        .busiest_disk
        .as_deref()
        .and_then(disk_number_from_instance);
    let disk_error_events_7d = evidence
        .disk_error_events_7d
        .iter()
        .filter(|entry| disk_number.is_none() || entry.disk_number == disk_number)
        .map(|entry| entry.count)
        .sum::<u32>();

    let cause = if smart_predict_failure == Some(true)
        || disk_error_events_7d >= DISK_ERROR_EVENTS_THRESHOLD
    {
        DefenderDiskCause::DiskFailing
    } else if !evidence.unhealthy_volumes.is_empty() || evidence.ntfs_corruption_events_7d > 0 {
        DefenderDiskCause::FileSystemCorruption
    } else if evidence.detections_24h >= DETECTION_LOOP_THRESHOLD
        || evidence.remediation_failures_24h > 0
    {
        DefenderDiskCause::ThreatRemediationLoop
    } else if evidence.scans_failed_24h >= SCAN_FAILURE_THRESHOLD
        || (evidence.scans_started_24h >= 4
            && evidence.scans_finished_24h * 2 < evidence.scans_started_24h)
    {
        DefenderDiskCause::ScanCannotFinish
    } else if evidence.signature_update_failures_24h >= 2 || evidence.engine_failures_24h > 0 {
        DefenderDiskCause::DefinitionsOrEngineFailing
    } else if evidence.scans_started_24h > 0
        && (evidence.scan_avg_cpu_load_factor.unwrap_or(DEFAULT_SCAN_CPU_LOAD_FACTOR)
            >= DEFAULT_SCAN_CPU_LOAD_FACTOR
            || evidence.disable_cpu_throttle_on_idle_scans == Some(true)
            || evidence.scan_only_if_idle == Some(false))
    {
        DefenderDiskCause::UnthrottledScan
    } else {
        DefenderDiskCause::Unknown
    };

    Some(DefenderDiskIssue {
        cause,
        detected_at: chrono::Utc::now().timestamp(),
        disk: activity.busiest_disk.clone(),
        disk_number,
        sustained_minutes: pressure.sustained_seconds / 60,
        peak_disk_active_percent: pressure.peak_disk_active_percent,
        peak_defender_mb_s: pressure.peak_defender_mb_s,
        disk_error_events_7d,
        ntfs_corruption_events_7d: evidence.ntfs_corruption_events_7d,
        unhealthy_volumes: evidence.unhealthy_volumes.clone(),
        detections_24h: evidence.detections_24h,
        remediation_failures_24h: evidence.remediation_failures_24h,
        scans_started_24h: evidence.scans_started_24h,
        scans_finished_24h: evidence.scans_finished_24h,
        scans_failed_24h: evidence.scans_failed_24h,
        signature_update_failures_24h: evidence.signature_update_failures_24h,
        engine_failures_24h: evidence.engine_failures_24h,
        scan_avg_cpu_load_factor: evidence.scan_avg_cpu_load_factor,
        disable_cpu_throttle_on_idle_scans: evidence.disable_cpu_throttle_on_idle_scans,
        scan_only_if_idle: evidence.scan_only_if_idle,
        full_scan_age_days: evidence.full_scan_age_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::advanced::DiskErrorEvents;

    fn activity(sustained: bool) -> DiskActivity {
        DiskActivity {
            busiest_disk: Some("1 D: E:".to_string()),
            busiest_disk_active_percent: Some(99.0),
            pressure: DefenderDiskPressure {
                active_now: true,
                sustained_seconds: if sustained { 900 } else { 60 },
                sustained,
                peak_defender_mb_s: 3.0,
                peak_disk_active_percent: 100.0,
            },
            ..DiskActivity::default()
        }
    }

    fn quiet_evidence() -> DefenderDiskEvidence {
        DefenderDiskEvidence {
            scans_started_24h: 1,
            scans_finished_24h: 1,
            scan_avg_cpu_load_factor: Some(50),
            ..DefenderDiskEvidence::default()
        }
    }

    #[test]
    fn no_sustained_pressure_means_no_issue_whatever_the_logs_say() {
        let mut evidence = quiet_evidence();
        evidence.scans_failed_24h = 10;
        assert!(classify(&activity(false), &evidence, Some(true)).is_none());
    }

    #[test]
    fn a_plain_scan_on_a_slow_disk_with_default_settings_is_unthrottled_scan() {
        let issue = classify(&activity(true), &quiet_evidence(), Some(false)).unwrap();
        assert_eq!(issue.cause, DefenderDiskCause::UnthrottledScan);
        assert_eq!(issue.disk_number, Some(1));
        assert_eq!(issue.sustained_minutes, 15);
    }

    #[test]
    fn already_throttled_scan_with_no_other_evidence_is_unknown() {
        let mut evidence = quiet_evidence();
        evidence.scan_avg_cpu_load_factor = Some(20);
        evidence.scan_only_if_idle = Some(true);
        evidence.disable_cpu_throttle_on_idle_scans = Some(false);
        let issue = classify(&activity(true), &evidence, None).unwrap();
        assert_eq!(issue.cause, DefenderDiskCause::Unknown);
    }

    #[test]
    fn hardware_wins_over_everything_but_only_for_the_saturated_disk() {
        let mut evidence = quiet_evidence();
        evidence.scans_failed_24h = 5;
        // 96 paging errors on Harddisk3 - a removable drive, not the disk
        // under pressure (the real numbers from the dev machine).
        evidence.disk_error_events_7d = vec![DiskErrorEvents {
            disk_number: Some(3),
            count: 96,
        }];
        let issue = classify(&activity(true), &evidence, Some(false)).unwrap();
        assert_eq!(issue.cause, DefenderDiskCause::ScanCannotFinish);
        assert_eq!(issue.disk_error_events_7d, 0);

        evidence.disk_error_events_7d.push(DiskErrorEvents {
            disk_number: Some(1),
            count: 12,
        });
        let issue = classify(&activity(true), &evidence, Some(false)).unwrap();
        assert_eq!(issue.cause, DefenderDiskCause::DiskFailing);
        assert_eq!(issue.disk_error_events_7d, 12);

        let issue = classify(&activity(true), &quiet_evidence(), Some(true)).unwrap();
        assert_eq!(issue.cause, DefenderDiskCause::DiskFailing);
    }

    #[test]
    fn corruption_beats_defender_failure_modes() {
        let mut evidence = quiet_evidence();
        evidence.detections_24h = 8;
        evidence.unhealthy_volumes = vec!["D".to_string()];
        let issue = classify(&activity(true), &evidence, None).unwrap();
        assert_eq!(issue.cause, DefenderDiskCause::FileSystemCorruption);
    }

    #[test]
    fn defender_log_failure_modes_in_priority_order() {
        let mut evidence = quiet_evidence();
        evidence.remediation_failures_24h = 1;
        evidence.scans_failed_24h = 4;
        evidence.signature_update_failures_24h = 5;
        assert_eq!(
            classify(&activity(true), &evidence, None).unwrap().cause,
            DefenderDiskCause::ThreatRemediationLoop
        );
        evidence.remediation_failures_24h = 0;
        assert_eq!(
            classify(&activity(true), &evidence, None).unwrap().cause,
            DefenderDiskCause::ScanCannotFinish
        );
        evidence.scans_failed_24h = 0;
        assert_eq!(
            classify(&activity(true), &evidence, None).unwrap().cause,
            DefenderDiskCause::DefinitionsOrEngineFailing
        );
        // Scans that keep starting and not finishing, without an explicit
        // failure event, still read as "cannot finish".
        evidence.signature_update_failures_24h = 0;
        evidence.scans_started_24h = 6;
        evidence.scans_finished_24h = 1;
        assert_eq!(
            classify(&activity(true), &evidence, None).unwrap().cause,
            DefenderDiskCause::ScanCannotFinish
        );
    }

    #[test]
    fn parses_disk_number_from_pdh_instances() {
        assert_eq!(disk_number_from_instance("1 D: E:"), Some(1));
        assert_eq!(disk_number_from_instance("0 C:"), Some(0));
        assert_eq!(disk_number_from_instance("_Total"), None);
    }
}

#[cfg(test)]
mod defender_disk_live_check {
    use super::*;
    use crate::telemetry::disk_activity::{DefenderDiskPressure, DiskActivity};

    /// Prints the real evidence gathered on this machine and what the
    /// classifier would say *if* the disk were under sustained pressure -
    /// the pressure itself can't be faked here, so this checks the
    /// PowerShell pass and the parsing, not the trigger.
    #[test]
    #[ignore] // machine-dependent - run manually with --ignored --nocapture
    fn live_defender_disk_evidence_on_this_machine() {
        let evidence = crate::telemetry::advanced::defender_disk_evidence().expect("evidence pass");
        println!("evidence = {evidence:#?}");
        let live = crate::telemetry::disk_activity::sample();
        println!("live activity = {live:#?}");
        let pretend = DiskActivity {
            busiest_disk: live.as_ref().and_then(|a| a.busiest_disk.clone()).or(Some("0 C:".into())),
            pressure: DefenderDiskPressure {
                active_now: true,
                sustained_seconds: 600,
                sustained: true,
                peak_defender_mb_s: 4.0,
                peak_disk_active_percent: 99.0,
            },
            ..DiskActivity::default()
        };
        println!("would classify as = {:#?}", classify(&pretend, &evidence, Some(false)).map(|i| i.cause));
    }
}
