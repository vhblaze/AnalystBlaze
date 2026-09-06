//! Ground-truth in-game performance capture, via Intel's PresentMon
//! (https://github.com/GameTechDev/PresentMon, MIT licensed).
//!
//! Everything else in this codebase infers "did this help?" from proxies -
//! CPU/GPU usage, temperature, process priority. None of that is what a
//! player actually experiences: frame time. PresentMon observes present
//! events via ETW (Event Tracing for Windows) - the same mechanism the
//! Xbox Game Bar performance overlay and CapFrameX use - which means it
//! reads what the display driver is already doing rather than hooking or
//! injecting into the game process itself, so it doesn't carry the
//! anti-cheat risk a DirectX/Vulkan hook would.
//!
//! This module owns the *data* side: parsing PresentMon's CSV output into
//! per-frame samples and reducing those into the aggregate stats a
//! before/after comparison or a training row actually needs. It
//! deliberately does not spawn PresentMon.exe itself yet - starting an ETW
//! trace session needs elevation, so that half belongs next to the
//! existing privileged helper service (see optimizations::privileged_helper),
//! not here, and shouldn't be wired up before someone can validate the
//! actual capture against a real GPU/game session.
//!
//! "1% low" / "0.1% low" below follow the convention most benchmarking
//! tools (CapFrameX, PresentMon's own analysis) use: the *average frame
//! time of the slowest N% of frames*, converted back to an FPS number -
//! not simply the value at that percentile. Averaging the tail is what
//! correlates with perceived stutter; a single percentile point can land
//! on an unusually good or bad individual frame.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSample {
    /// PresentMon's `MsBetweenPresents` - the actual frame time, in
    /// milliseconds, of this presented frame.
    pub ms_between_presents: f64,
    /// PresentMon's `Dropped` column - the frame was presented by the
    /// application but never made it to the screen (superseded before
    /// vsync, over-full present queue, etc.). Counted separately from
    /// stutter: a high drop rate with otherwise-smooth frame times still
    /// means real presents are being wasted.
    pub dropped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FrameTimeStats {
    pub sample_count: usize,
    pub avg_fps: f64,
    pub avg_frame_time_ms: f64,
    /// Average FPS of the slowest 1% of frames - the standard "1% low".
    pub low_1pct_fps: f64,
    /// Average FPS of the slowest 0.1% of frames - the standard "0.1% low".
    pub low_0_1pct_fps: f64,
    pub dropped_frame_count: usize,
    /// Frames at least 2x the session's median frame time - a common,
    /// simple stutter heuristic (a single missed-vsync frame on an
    /// otherwise-60fps session shows up here even though it barely moves
    /// the 1% low, which is what makes it worth tracking separately).
    pub stutter_count: usize,
}

/// Parses one PresentMon CSV export (including its header row) into frame
/// samples. Column position isn't assumed - PresentMon's exact column set
/// varies by version/flags, so this looks up `MsBetweenPresents` and
/// `Dropped` by header name and ignores every other column.
pub fn parse_presentmon_csv(csv: &str) -> Vec<FrameSample> {
    let mut lines = csv.lines();
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let columns: Vec<&str> = header.split(',').map(str::trim).collect();
    let frame_time_index = columns.iter().position(|&name| name == "MsBetweenPresents");
    let dropped_index = columns.iter().position(|&name| name == "Dropped");
    let Some(frame_time_index) = frame_time_index else {
        return Vec::new();
    };

    lines
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            let ms_between_presents = fields.get(frame_time_index)?.trim().parse::<f64>().ok()?;
            if !ms_between_presents.is_finite() || ms_between_presents <= 0.0 {
                return None;
            }
            let dropped = dropped_index
                .and_then(|index| fields.get(index))
                .is_some_and(|value| matches!(value.trim(), "1" | "true" | "True"));
            Some(FrameSample {
                ms_between_presents,
                dropped,
            })
        })
        .collect()
}

pub fn compute_frame_time_stats(samples: &[FrameSample]) -> FrameTimeStats {
    if samples.is_empty() {
        return FrameTimeStats::default();
    }

    let mut frame_times: Vec<f64> = samples.iter().map(|sample| sample.ms_between_presents).collect();
    frame_times.sort_by(f64::total_cmp);

    let sample_count = frame_times.len();
    let avg_frame_time_ms = frame_times.iter().sum::<f64>() / sample_count as f64;
    let median_frame_time_ms = percentile_value(&frame_times, 0.50);

    FrameTimeStats {
        sample_count,
        avg_fps: fps_from_ms(avg_frame_time_ms),
        avg_frame_time_ms,
        low_1pct_fps: fps_from_ms(tail_average(&frame_times, 0.01)),
        low_0_1pct_fps: fps_from_ms(tail_average(&frame_times, 0.001)),
        dropped_frame_count: samples.iter().filter(|sample| sample.dropped).count(),
        stutter_count: frame_times
            .iter()
            .filter(|&&ms| ms >= median_frame_time_ms * 2.0)
            .count(),
    }
}

fn fps_from_ms(ms: f64) -> f64 {
    if ms > 0.0 {
        1000.0 / ms
    } else {
        0.0
    }
}

/// `sorted_ascending` must already be sorted from fastest (lowest ms) to
/// slowest (highest ms) frame time - true of both `frame_times` above.
fn percentile_value(sorted_ascending: &[f64], fraction: f64) -> f64 {
    if sorted_ascending.is_empty() {
        return 0.0;
    }
    let index = ((sorted_ascending.len() as f64 - 1.0) * fraction).round() as usize;
    sorted_ascending[index.min(sorted_ascending.len() - 1)]
}

/// Average frame time of the slowest `fraction` of frames (e.g. 0.01 for
/// the worst 1%) - always at least one frame, so this stays meaningful even
/// on a short capture window.
fn tail_average(sorted_ascending: &[f64], fraction: f64) -> f64 {
    if sorted_ascending.is_empty() {
        return 0.0;
    }
    let tail_len = ((sorted_ascending.len() as f64 * fraction).ceil() as usize).max(1);
    let tail = &sorted_ascending[sorted_ascending.len() - tail_len..];
    tail.iter().sum::<f64>() / tail.len() as f64
}

/// Groups samples by ProcessID when a capture spans more than one target
/// process (PresentMon supports capturing multiple `--process_id`s at
/// once) - not used yet since nothing spawns PresentMon with more than one
/// target today, but kept as a documented seam for when it does, so the
/// per-process split doesn't need to be re-derived later.
pub fn parse_presentmon_csv_by_process(csv: &str) -> HashMap<String, Vec<FrameSample>> {
    let mut lines = csv.lines();
    let Some(header) = lines.next() else {
        return HashMap::new();
    };
    let columns: Vec<&str> = header.split(',').map(str::trim).collect();
    let Some(frame_time_index) = columns.iter().position(|&name| name == "MsBetweenPresents") else {
        return HashMap::new();
    };
    let dropped_index = columns.iter().position(|&name| name == "Dropped");
    let process_id_index = columns.iter().position(|&name| name == "ProcessID");

    let mut grouped: HashMap<String, Vec<FrameSample>> = HashMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();
        let Some(ms_between_presents) = fields
            .get(frame_time_index)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            continue;
        };
        let dropped = dropped_index
            .and_then(|index| fields.get(index))
            .is_some_and(|value| matches!(value.trim(), "1" | "true" | "True"));
        let process_id = process_id_index
            .and_then(|index| fields.get(index))
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        grouped.entry(process_id).or_default().push(FrameSample {
            ms_between_presents,
            dropped,
        });
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv_with_frame_times(frame_times_ms: &[f64]) -> String {
        let mut csv = "Application,ProcessID,MsBetweenPresents,Dropped\n".to_string();
        for ms in frame_times_ms {
            csv.push_str(&format!("game.exe,1234,{ms},0\n"));
        }
        csv
    }

    #[test]
    fn parses_frame_times_regardless_of_column_order() {
        let csv = "ProcessID,Dropped,MsBetweenPresents,Application\n1234,0,16.67,game.exe\n1234,1,33.33,game.exe\n";
        let samples = parse_presentmon_csv(csv);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].ms_between_presents, 16.67);
        assert!(!samples[0].dropped);
        assert_eq!(samples[1].ms_between_presents, 33.33);
        assert!(samples[1].dropped);
    }

    #[test]
    fn ignores_malformed_or_non_numeric_rows_instead_of_failing_the_whole_parse() {
        let csv = "MsBetweenPresents,Dropped\n16.67,0\nNOT_A_NUMBER,0\n0,0\n-5,0\n8.33,0\n";
        let samples = parse_presentmon_csv(csv);
        assert_eq!(samples.len(), 2);
    }

    #[test]
    fn returns_empty_when_the_frame_time_column_is_missing() {
        let csv = "Application,ProcessID\ngame.exe,1234\n";
        assert!(parse_presentmon_csv(csv).is_empty());
    }

    #[test]
    fn a_perfectly_steady_60fps_session_reports_matching_avg_and_lows() {
        let frame_times = vec![16.667; 1000];
        let csv = csv_with_frame_times(&frame_times);
        let stats = compute_frame_time_stats(&parse_presentmon_csv(&csv));

        assert_eq!(stats.sample_count, 1000);
        assert!((stats.avg_fps - 60.0).abs() < 0.1);
        assert!((stats.low_1pct_fps - 60.0).abs() < 0.1);
        assert!((stats.low_0_1pct_fps - 60.0).abs() < 0.1);
        assert_eq!(stats.stutter_count, 0);
    }

    #[test]
    fn a_stutter_tail_drags_down_the_lows_but_barely_touches_the_average() {
        // 990 frames at a smooth 16.67ms (60fps), 10 frames stalled at
        // 66.67ms (15fps) - a real "microstutter near the end of a level
        // load" shape, not a sustained slowdown.
        let mut frame_times = vec![16.667; 990];
        frame_times.extend(vec![66.667; 10]);
        let csv = csv_with_frame_times(&frame_times);
        let stats = compute_frame_time_stats(&parse_presentmon_csv(&csv));

        assert_eq!(stats.sample_count, 1000);
        // The stutter is only 1% of frames, so the plain average barely moves...
        assert!(stats.avg_fps > 55.0, "avg_fps was {}", stats.avg_fps);
        // ...but the 1% low is exactly what it's meant to expose.
        assert!(
            (stats.low_1pct_fps - 15.0).abs() < 0.5,
            "low_1pct_fps was {}",
            stats.low_1pct_fps
        );
        assert_eq!(stats.stutter_count, 10);
    }

    #[test]
    fn worse_tail_pulls_the_0_1_percent_low_further_down_than_the_1_percent_low() {
        let mut frame_times = vec![16.667; 989];
        frame_times.extend(vec![50.0; 10]); // 1% at ~20fps
        frame_times.extend(vec![200.0; 1]); // 0.1% at 5fps - a hard hitch
        let csv = csv_with_frame_times(&frame_times);
        let stats = compute_frame_time_stats(&parse_presentmon_csv(&csv));

        assert!(stats.low_0_1pct_fps < stats.low_1pct_fps);
        assert!(stats.low_1pct_fps < stats.avg_fps);
    }

    #[test]
    fn counts_dropped_frames_independently_of_stutter() {
        let csv = "MsBetweenPresents,Dropped\n16.67,1\n16.67,0\n16.67,1\n";
        let stats = compute_frame_time_stats(&parse_presentmon_csv(csv));
        assert_eq!(stats.dropped_frame_count, 2);
        assert_eq!(stats.stutter_count, 0);
    }

    #[test]
    fn empty_capture_reports_zeroed_stats_instead_of_dividing_by_zero() {
        let stats = compute_frame_time_stats(&[]);
        assert_eq!(stats, FrameTimeStats::default());
    }

    #[test]
    fn groups_samples_by_process_id_when_multiple_targets_are_captured() {
        let csv = "ProcessID,MsBetweenPresents\n111,16.67\n222,33.33\n111,16.67\n";
        let grouped = parse_presentmon_csv_by_process(csv);
        assert_eq!(grouped.get("111").map(Vec::len), Some(2));
        assert_eq!(grouped.get("222").map(Vec::len), Some(1));
    }
}
