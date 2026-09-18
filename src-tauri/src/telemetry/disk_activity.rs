//! Disk *activity* (as opposed to disk *space*, which is what
//! `TelemetrySample::disk_usage_percent` has always been): how busy each
//! physical disk is and which processes are generating the I/O. Built for
//! one report (2026-09-18): "Windows Defender keeps reading corrupted files,
//! HDD and SSD stuck at 100%". Until this module the app could not see a
//! saturated disk at all, let alone say who was saturating it.
//!
//! Source is PDH, the same counters Task Manager and perfmon read:
//!
//! - `\PhysicalDisk(*)\% Idle Time` - Task Manager's "Active time" is
//!   `100 - idle`. Per disk, not `_Total`: with an HDD and an SSD the
//!   average of a pegged HDD and an idle SSD reads "50%", hiding the
//!   problem.
//! - `\Process(*)\IO Data Bytes/sec`, `IO Data Operations/sec`,
//!   `% Processor Time` - per process, without opening a handle to any of
//!   them. That matters for MsMpEng.exe specifically: it is a protected
//!   (PPL) process and `GetProcessIoCounters` on it fails even from an
//!   elevated caller (verified: sysinfo reports 0/0/0 for it), while the
//!   PDH process object is fed by the kernel and works unelevated.
//!
//! `IO Data Operations/sec` is carried alongside bytes on purpose: a scan
//! saturating an HDD with random 4K reads moves maybe 1 MB/s, so bytes
//! alone would look harmless. Share of operations catches that case.
//!
//! The tracker is process-wide (same reasoning as collector.rs's shared
//! caches - several throwaway collectors exist, and a "sustained for N
//! minutes" judgement needs one continuous history, not a fresh one per
//! collector). Everything here stays on the machine: process names are not
//! part of any backend payload (see engine.rs's explicit allowlists).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Task Manager shows "100%" from well below it; 90 is where a disk is
/// effectively saturated for anything else that wants it.
pub const DISK_SATURATED_PERCENT: f64 = 90.0;
/// Defender is "the one saturating it" when it owns at least this share of
/// all process I/O (by bytes or by operations, whichever is higher).
pub const DEFENDER_IO_SHARE_PERCENT: f64 = 50.0;
/// Below both of these Defender's share is meaningless noise (50% of
/// nothing), even if the disk is busy for some other reason.
pub const DEFENDER_MIN_MB_S: f64 = 1.0;
pub const DEFENDER_MIN_OPS_S: f64 = 50.0;
/// How far back the pressure judgement looks, and how much of that window
/// has to be under pressure before it is called sustained. Five minutes
/// rules out a quick scan doing its normal thing on a slow disk.
pub const PRESSURE_WINDOW: Duration = Duration::from_secs(30 * 60);
pub const SUSTAINED_TRIGGER_SECONDS: u64 = 5 * 60;
/// A sample only vouches for the time up to the next sample, capped: with
/// the dashboard closed ticks are 60s apart, and a gap longer than this
/// (sleep, suspended collection) must not be counted as "under pressure".
const MAX_SAMPLE_GAP_SECONDS: u64 = 120;
/// Ticks can come every 2s with the dashboard open; PDH rate counters need
/// a real interval to be meaningful, so anything closer than this reuses
/// the previous reading.
const MIN_SAMPLE_INTERVAL: Duration = Duration::from_millis(900);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessIo {
    /// PDH instance name: lowercase, no `.exe`, `#N` duplicates merged
    /// (`msedgewebview2#3` + `msedgewebview2` -> `msedgewebview2`).
    pub name: String,
    pub mb_s: f64,
    pub ops_s: f64,
    /// Of all process I/O bytes in this sample.
    pub share_percent: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DefenderIo {
    pub mb_s: f64,
    pub ops_s: f64,
    /// max(share by bytes, share by operations) - see module docs.
    pub share_percent: f64,
    /// Normalised to 0-100 across all logical CPUs.
    pub cpu_percent: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DefenderDiskPressure {
    /// The latest sample met the pressure rule.
    pub active_now: bool,
    /// Seconds under pressure within the last `PRESSURE_WINDOW`.
    pub sustained_seconds: u64,
    /// `sustained_seconds >= SUSTAINED_TRIGGER_SECONDS`.
    pub sustained: bool,
    pub peak_defender_mb_s: f64,
    pub peak_disk_active_percent: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DiskActivity {
    pub sampled_at: i64,
    /// PDH instance of the busiest disk, e.g. `0 C:` or `1 D: E:`.
    pub busiest_disk: Option<String>,
    pub busiest_disk_active_percent: Option<f64>,
    pub top_io_processes: Vec<ProcessIo>,
    pub defender: Option<DefenderIo>,
    pub pressure: DefenderDiskPressure,
}

/// One raw PDH read, before any judgement.
#[derive(Debug, Clone, Default)]
struct RawReading {
    /// (instance, idle percent) per physical disk, `_Total` excluded.
    disk_idle: Vec<(String, f64)>,
    /// (instance, bytes/s, ops/s, cpu percent-of-one-core) per process,
    /// `_Total` and `Idle` excluded.
    processes: Vec<(String, f64, f64, f64)>,
}

/// The pressure rule on one sample. Pure so the tests can drive it.
pub fn sample_under_pressure(disk_active_percent: Option<f64>, defender: Option<&DefenderIo>) -> bool {
    let Some(active) = disk_active_percent else {
        return false;
    };
    let Some(defender) = defender else {
        return false;
    };
    active >= DISK_SATURATED_PERCENT
        && defender.share_percent >= DEFENDER_IO_SHARE_PERCENT
        && (defender.mb_s >= DEFENDER_MIN_MB_S || defender.ops_s >= DEFENDER_MIN_OPS_S)
}

#[derive(Debug, Clone, Copy)]
struct HistoryPoint {
    at: Instant,
    under_pressure: bool,
    defender_mb_s: f64,
    disk_active_percent: f64,
}

/// Time-weighted "seconds under pressure" over `history` (oldest first),
/// each point vouching for the gap to the next one (capped), the last one
/// for the gap to `now`.
fn sustained_seconds(history: &[HistoryPoint], now: Instant) -> u64 {
    let mut total = 0u64;
    for (index, point) in history.iter().enumerate() {
        if !point.under_pressure {
            continue;
        }
        let next = history.get(index + 1).map(|next| next.at).unwrap_or(now);
        let gap = next.saturating_duration_since(point.at).as_secs();
        total += gap.min(MAX_SAMPLE_GAP_SECONDS);
    }
    total
}

fn pressure_from_history(history: &[HistoryPoint], now: Instant) -> DefenderDiskPressure {
    let sustained_seconds = sustained_seconds(history, now);
    DefenderDiskPressure {
        active_now: history.last().is_some_and(|point| point.under_pressure),
        sustained_seconds,
        sustained: sustained_seconds >= SUSTAINED_TRIGGER_SECONDS,
        peak_defender_mb_s: history
            .iter()
            .map(|point| point.defender_mb_s)
            .fold(0.0, f64::max),
        peak_disk_active_percent: history
            .iter()
            .map(|point| point.disk_active_percent)
            .fold(0.0, f64::max),
    }
}

/// `msedgewebview2#3` -> `msedgewebview2`; PDH names carry no `.exe`.
fn canonical_process_name(instance: &str) -> String {
    let base = instance.split('#').next().unwrap_or(instance);
    base.trim().to_ascii_lowercase()
}

fn is_defender_engine(name: &str) -> bool {
    name == "msmpeng"
}

/// Turns one raw reading into the sample-level view: busiest disk, top
/// I/O processes, Defender's share. Pure, so the tests can feed it.
fn summarise(raw: &RawReading, logical_cpus: f64) -> (Option<String>, Option<f64>, Vec<ProcessIo>, Option<DefenderIo>) {
    let busiest = raw
        .disk_idle
        .iter()
        .map(|(name, idle)| (name.clone(), (100.0 - idle).clamp(0.0, 100.0)))
        .max_by(|left, right| left.1.total_cmp(&right.1));
    let (busiest_disk, busiest_active) = match busiest {
        Some((name, active)) => (Some(name), Some(active)),
        None => (None, None),
    };

    // Merge `#N` duplicates so a browser with 30 helper processes shows as
    // one line - and so Defender's share is against everything, not
    // against a list of fragments.
    let mut merged: Vec<(String, f64, f64, f64)> = Vec::new();
    for (instance, bytes, ops, cpu) in &raw.processes {
        let name = canonical_process_name(instance);
        match merged.iter_mut().find(|entry| entry.0 == name) {
            Some(entry) => {
                entry.1 += bytes;
                entry.2 += ops;
                entry.3 += cpu;
            }
            None => merged.push((name, *bytes, *ops, *cpu)),
        }
    }
    let total_bytes: f64 = merged.iter().map(|entry| entry.1).sum();
    let total_ops: f64 = merged.iter().map(|entry| entry.2).sum();
    let share = |part: f64, total: f64| if total > 0.0 { (part / total * 100.0).clamp(0.0, 100.0) } else { 0.0 };

    let defender = merged
        .iter()
        .find(|entry| is_defender_engine(&entry.0))
        .map(|entry| DefenderIo {
            mb_s: entry.1 / 1_048_576.0,
            ops_s: entry.2,
            share_percent: share(entry.1, total_bytes).max(share(entry.2, total_ops)),
            cpu_percent: (logical_cpus > 0.0).then(|| (entry.3 / logical_cpus).clamp(0.0, 100.0)),
        });

    merged.sort_by(|left, right| (right.1 + right.2 * 4096.0).total_cmp(&(left.1 + left.2 * 4096.0)));
    let top = merged
        .into_iter()
        .filter(|entry| entry.1 > 0.0 || entry.2 > 0.0)
        .take(5)
        .map(|entry| ProcessIo {
            name: entry.0,
            mb_s: entry.1 / 1_048_576.0,
            ops_s: entry.2,
            share_percent: share(entry.1, total_bytes),
        })
        .collect();

    (busiest_disk, busiest_active, top, defender)
}

struct Tracker {
    #[cfg(windows)]
    reader: Option<PdhDiskActivityReader>,
    history: VecDeque<HistoryPoint>,
    last: Option<(Instant, DiskActivity)>,
    logical_cpus: f64,
}

impl Tracker {
    fn new() -> Self {
        Self {
            #[cfg(windows)]
            reader: PdhDiskActivityReader::new(),
            history: VecDeque::with_capacity(64),
            last: None,
            logical_cpus: std::thread::available_parallelism()
                .map(|count| count.get() as f64)
                .unwrap_or(1.0),
        }
    }

    fn sample(&mut self, now: Instant) -> Option<DiskActivity> {
        if let Some((at, activity)) = &self.last {
            if now.saturating_duration_since(*at) < MIN_SAMPLE_INTERVAL {
                return Some(activity.clone());
            }
        }

        let raw = self.read_raw()?;
        let (busiest_disk, busiest_active, top_io_processes, defender) =
            summarise(&raw, self.logical_cpus);

        self.history.push_back(HistoryPoint {
            at: now,
            under_pressure: sample_under_pressure(busiest_active, defender.as_ref()),
            defender_mb_s: defender.as_ref().map(|io| io.mb_s).unwrap_or(0.0),
            disk_active_percent: busiest_active.unwrap_or(0.0),
        });
        while self
            .history
            .front()
            .is_some_and(|point| now.saturating_duration_since(point.at) > PRESSURE_WINDOW)
        {
            self.history.pop_front();
        }
        let pressure = pressure_from_history(self.history.make_contiguous(), now);

        let activity = DiskActivity {
            sampled_at: chrono::Utc::now().timestamp(),
            busiest_disk,
            busiest_disk_active_percent: busiest_active,
            top_io_processes,
            defender,
            pressure,
        };
        self.last = Some((now, activity.clone()));
        Some(activity)
    }

    #[cfg(windows)]
    fn read_raw(&mut self) -> Option<RawReading> {
        self.reader.as_mut()?.read()
    }

    #[cfg(not(windows))]
    fn read_raw(&mut self) -> Option<RawReading> {
        None
    }
}

static TRACKER: std::sync::OnceLock<std::sync::Mutex<Tracker>> = std::sync::OnceLock::new();

fn tracker() -> &'static std::sync::Mutex<Tracker> {
    TRACKER.get_or_init(|| std::sync::Mutex::new(Tracker::new()))
}

/// Called once per telemetry tick by the collector. Returns the current
/// view (possibly the previous one if the tick came too soon), or `None`
/// when PDH is unavailable - never blocks on anything but the PDH read.
pub fn sample() -> Option<DiskActivity> {
    let mut guard = tracker().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.sample(Instant::now())
}

/// The last view without taking a new reading - for the low-frequency
/// collectors (advanced.rs) that want to know whether it is worth spending
/// PowerShell on Defender evidence at all.
pub fn latest() -> Option<DiskActivity> {
    let guard = tracker().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.last.as_ref().map(|(_, activity)| activity.clone())
}

#[cfg(windows)]
struct PdhDiskActivityReader {
    query: windows::Win32::System::Performance::PDH_HQUERY,
    disk_idle: windows::Win32::System::Performance::PDH_HCOUNTER,
    io_bytes: windows::Win32::System::Performance::PDH_HCOUNTER,
    io_ops: windows::Win32::System::Performance::PDH_HCOUNTER,
    cpu: windows::Win32::System::Performance::PDH_HCOUNTER,
    has_baseline: bool,
}

#[cfg(windows)]
unsafe impl Send for PdhDiskActivityReader {}

#[cfg(windows)]
impl PdhDiskActivityReader {
    fn new() -> Option<Self> {
        use windows::core::PCWSTR;
        use windows::Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW, PDH_HCOUNTER,
            PDH_HQUERY,
        };

        let mut query = PDH_HQUERY::default();
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }

        let add = |path: &str| -> Option<PDH_HCOUNTER> {
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let mut counter = PDH_HCOUNTER::default();
            (unsafe { PdhAddEnglishCounterW(query, PCWSTR(wide.as_ptr()), 0, &mut counter) } == 0)
                .then_some(counter)
        };

        let counters = (|| {
            Some((
                add(r"\PhysicalDisk(*)\% Idle Time")?,
                add(r"\Process(*)\IO Data Bytes/sec")?,
                add(r"\Process(*)\IO Data Operations/sec")?,
                add(r"\Process(*)\% Processor Time")?,
            ))
        })();
        let Some((disk_idle, io_bytes, io_ops, cpu)) = counters else {
            unsafe {
                PdhCloseQuery(query);
            }
            return None;
        };

        unsafe {
            PdhCollectQueryData(query);
        }

        Some(Self {
            query,
            disk_idle,
            io_bytes,
            io_ops,
            cpu,
            has_baseline: false,
        })
    }

    fn read(&mut self) -> Option<RawReading> {
        use windows::Win32::System::Performance::PdhCollectQueryData;

        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return None;
        }
        // Rate counters need two collections; the first read after the
        // constructor's priming collect is the first usable one, but a
        // query that was created moments ago still has a near-zero window
        // - skip it rather than report garbage rates.
        if !self.has_baseline {
            self.has_baseline = true;
            return None;
        }

        let disk_idle = formatted_counter_array(self.disk_idle)?
            .into_iter()
            .filter(|(name, _)| name != "_Total")
            .collect();
        let bytes = formatted_counter_array(self.io_bytes)?;
        let ops = formatted_counter_array(self.io_ops)?;
        let cpu = formatted_counter_array(self.cpu)?;

        let lookup = |values: &[(String, f64)], name: &str| {
            values
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, value)| *value)
                .unwrap_or(0.0)
        };
        let processes = bytes
            .iter()
            .filter(|(name, _)| name != "_Total" && name != "Idle")
            .map(|(name, value)| (name.clone(), *value, lookup(&ops, name), lookup(&cpu, name)))
            .collect();

        Some(RawReading {
            disk_idle,
            processes,
        })
    }
}

#[cfg(windows)]
impl Drop for PdhDiskActivityReader {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Performance::PdhCloseQuery(self.query);
        }
    }
}

/// `PdhGetFormattedCounterArrayW` with the usual size-probe-then-fill
/// dance. Items whose own status is not valid (an instance that vanished
/// between collections) are dropped rather than reported as 0.
#[cfg(windows)]
fn formatted_counter_array(
    counter: windows::Win32::System::Performance::PDH_HCOUNTER,
) -> Option<Vec<(String, f64)>> {
    use windows::Win32::System::Performance::{
        PdhGetFormattedCounterArrayW, PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
        PDH_MORE_DATA,
    };

    // PDH_FMT_NOCAP100 (0x8000) is not in the windows crate's bindings:
    // without it a per-process "% Processor Time" (which sums cores, so
    // legitimately exceeds 100) would be capped and the CPU figure wrong.
    let format = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);
    let mut buffer_size = 0u32;
    let mut item_count = 0u32;
    let status = unsafe {
        PdhGetFormattedCounterArrayW(counter, format, &mut buffer_size, &mut item_count, None)
    };
    if status != PDH_MORE_DATA || buffer_size == 0 {
        return None;
    }

    // The buffer holds the item structs followed by the names they point
    // into; over-align to the struct so the cast below is sound.
    let mut buffer = vec![0u64; (buffer_size as usize).div_ceil(8) + 1];
    let items = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
    let status = unsafe {
        PdhGetFormattedCounterArrayW(counter, format, &mut buffer_size, &mut item_count, Some(items))
    };
    if status != 0 {
        return None;
    }

    let mut values = Vec::with_capacity(item_count as usize);
    for index in 0..item_count as usize {
        let item = unsafe { &*items.add(index) };
        if item.FmtValue.CStatus != 0 {
            continue;
        }
        let name = unsafe { item.szName.to_string() }.unwrap_or_default();
        let value = unsafe { item.FmtValue.Anonymous.doubleValue };
        if name.is_empty() || !value.is_finite() {
            continue;
        }
        values.push((name, value));
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defender(mb_s: f64, ops_s: f64, share: f64) -> DefenderIo {
        DefenderIo {
            mb_s,
            ops_s,
            share_percent: share,
            cpu_percent: None,
        }
    }

    #[test]
    fn pressure_needs_a_saturated_disk_and_defender_owning_the_io() {
        assert!(sample_under_pressure(Some(97.0), Some(&defender(12.0, 300.0, 80.0))));
        // HDD hammered with random reads: few bytes, many operations.
        assert!(sample_under_pressure(Some(99.0), Some(&defender(0.8, 400.0, 70.0))));
        // Busy disk but Defender is a bystander.
        assert!(!sample_under_pressure(Some(99.0), Some(&defender(0.2, 3.0, 4.0))));
        // Defender scanning hard on a disk that keeps up (NVMe) is fine.
        assert!(!sample_under_pressure(Some(35.0), Some(&defender(80.0, 2000.0, 95.0))));
        // 60% of almost nothing is not pressure.
        assert!(!sample_under_pressure(Some(95.0), Some(&defender(0.1, 5.0, 60.0))));
        assert!(!sample_under_pressure(None, Some(&defender(50.0, 500.0, 90.0))));
        assert!(!sample_under_pressure(Some(99.0), None));
    }

    #[test]
    fn summarise_merges_pdh_duplicates_and_reports_the_busiest_disk() {
        let raw = RawReading {
            disk_idle: vec![("0 C:".into(), 95.0), ("1 D:".into(), 2.0)],
            processes: vec![
                ("chrome".into(), 1_048_576.0, 10.0, 10.0),
                ("chrome#1".into(), 1_048_576.0, 10.0, 10.0),
                ("MsMpEng".into(), 6_291_456.0, 500.0, 160.0),
            ],
        };
        let (disk, active, top, defender) = summarise(&raw, 16.0);
        assert_eq!(disk.as_deref(), Some("1 D:"));
        assert_eq!(active, Some(98.0));
        assert_eq!(top[0].name, "msmpeng");
        assert_eq!(top[1].name, "chrome");
        assert!((top[1].mb_s - 2.0).abs() < 1e-9, "duplicates merge");
        let defender = defender.expect("MsMpEng present");
        assert!((defender.share_percent - 96.15).abs() < 0.1, "share by ops (500/520) beats share by bytes (6/8)");
        assert_eq!(defender.cpu_percent, Some(10.0));
    }

    #[test]
    fn summarise_without_defender_running() {
        let raw = RawReading {
            disk_idle: vec![("0 C:".into(), 50.0)],
            processes: vec![("chrome".into(), 100.0, 1.0, 0.0)],
        };
        let (_, _, _, defender) = summarise(&raw, 8.0);
        assert!(defender.is_none());
    }

    #[test]
    fn sustained_seconds_is_time_weighted_and_capped_per_gap() {
        let start = Instant::now();
        let point = |offset: u64, under_pressure: bool| HistoryPoint {
            at: start + Duration::from_secs(offset),
            under_pressure,
            defender_mb_s: 0.0,
            disk_active_percent: 0.0,
        };
        // 60s ticks: pressure at 0 and 60 (vouch for 60s each), not at 120,
        // pressure at 180 followed by a 10-minute hole (capped at 120s),
        // then pressure at 780 vouching for the 30s until "now".
        let history = vec![
            point(0, true),
            point(60, true),
            point(120, false),
            point(180, true),
            point(780, true),
        ];
        let now = start + Duration::from_secs(810);
        assert_eq!(sustained_seconds(&history, now), 60 + 60 + 120 + 30);
        let pressure = pressure_from_history(&history, now);
        assert!(pressure.active_now);
        assert!(!pressure.sustained, "270s is under the 300s trigger");
    }

    #[test]
    fn canonical_names_drop_pdh_suffixes() {
        assert_eq!(canonical_process_name("msedgewebview2#12"), "msedgewebview2");
        assert_eq!(canonical_process_name("MsMpEng"), "msmpeng");
    }
}

#[cfg(test)]
mod disk_activity_live_check {
    #[test]
    #[ignore] // machine-dependent - run manually with --ignored --nocapture
    fn live_disk_activity_on_this_machine() {
        let _ = super::sample();
        std::thread::sleep(std::time::Duration::from_secs(3));
        let activity = super::sample();
        println!("{activity:#?}");
        assert!(activity.is_some(), "PDH query failed to open on this machine");
    }
}
