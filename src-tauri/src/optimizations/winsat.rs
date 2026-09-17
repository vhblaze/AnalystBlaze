//! Reads Windows' own one-time hardware capability benchmark
//! (`Win32_WinSAT`, the old "Windows Experience Index" subsystem) to tell
//! apart two things that look identical in live telemetry alone: a
//! bottleneck caused by temporary software/usage pressure (close some apps,
//! it goes away) versus one caused by the hardware itself being the ceiling
//! (upgrade advice is the honest answer). See performance_suite.rs, which
//! is the only caller - this module only produces the raw scores.
//!
//! Windows stopped auto-running this assessment after 8.1: on a real
//! machine it's only present/fresh if someone manually ran `winsat formal`
//! at some point (OEM factory image, or a user who knows the command), so
//! every score here is optional and `assessment_valid` must be checked -
//! never treat a present-but-invalid score as reliable.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

use crate::process_ext::{decode_console_bytes, CommandExt};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WinsatScores {
    pub cpu_score: Option<f64>,
    pub memory_score: Option<f64>,
    pub disk_score: Option<f64>,
    pub graphics_score: Option<f64>,
    pub d3d_score: Option<f64>,
    /// `WinSATAssessmentState == 1` ("Valid"). False when no assessment was
    /// ever run, or the stored one is stale after a hardware change -
    /// scores may still be present (Windows keeps the last-known numbers)
    /// but should not be trusted when this is false.
    pub assessment_valid: bool,
}

static CACHE: OnceLock<Mutex<Option<WinsatScores>>> = OnceLock::new();

/// Cached for the life of the process - WinSAT only changes when someone
/// manually reruns `winsat formal` or swaps hardware, neither of which
/// happens while AnalystBlaze is running, so there's no reason to spawn a
/// fresh PowerShell call for every performance scan.
pub fn current_scores() -> Option<WinsatScores> {
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Some(scores) = cache.lock().unwrap().as_ref() {
        return Some(scores.clone());
    }

    let scores = query_winsat_scores();
    if let Some(scores) = &scores {
        *cache.lock().unwrap() = Some(scores.clone());
    }
    scores
}

#[cfg(windows)]
fn query_winsat_scores() -> Option<WinsatScores> {
    use std::process::Command;

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "Get-CimInstance Win32_WinSAT | Select-Object CPUScore,MemoryScore,DiskScore,GraphicsScore,D3DScore,WinSATAssessmentState | ConvertTo-Json -Compress",
        ])
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = decode_console_bytes(&output.stdout);
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    parse_winsat_json(&value)
}

#[cfg(not(windows))]
fn query_winsat_scores() -> Option<WinsatScores> {
    None
}

fn parse_winsat_json(value: &Value) -> Option<WinsatScores> {
    // An empty object (Win32_WinSAT returned no instance at all - not even
    // seen live, but WMI classes can be absent on stripped-down/Server
    // Core-style installs) means there's nothing to report, not a score of
    // zero for everything.
    if value.as_object().is_some_and(|map| map.is_empty()) {
        return None;
    }

    Some(WinsatScores {
        cpu_score: valid_score(value.get("CPUScore").and_then(Value::as_f64)),
        memory_score: valid_score(value.get("MemoryScore").and_then(Value::as_f64)),
        disk_score: valid_score(value.get("DiskScore").and_then(Value::as_f64)),
        graphics_score: valid_score(value.get("GraphicsScore").and_then(Value::as_f64)),
        d3d_score: valid_score(value.get("D3DScore").and_then(Value::as_f64)),
        assessment_valid: value.get("WinSATAssessmentState").and_then(Value::as_i64) == Some(1),
    })
}

/// WinSAT reports -1.0 for a subsystem it never got to measure (e.g. no
/// dedicated GPU) - that is "not applicable", not "score zero", so it's
/// dropped to None rather than dragging an average down.
fn valid_score(score: Option<f64>) -> Option<f64> {
    score.filter(|value| *value >= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_valid_assessment_with_all_scores_present() {
        let value = json!({
            "CPUScore": 9.3,
            "MemoryScore": 9.3,
            "DiskScore": 8.85,
            "GraphicsScore": 9.5,
            "D3DScore": 9.9,
            "WinSATAssessmentState": 1,
        });

        let scores = parse_winsat_json(&value).expect("scores");
        assert_eq!(scores.cpu_score, Some(9.3));
        assert_eq!(scores.disk_score, Some(8.85));
        assert!(scores.assessment_valid);
    }

    #[test]
    fn an_assessment_state_other_than_one_is_not_valid() {
        let value = json!({
            "CPUScore": 5.0,
            "WinSATAssessmentState": 0,
        });

        let scores = parse_winsat_json(&value).expect("scores");
        assert!(!scores.assessment_valid);
    }

    #[test]
    fn negative_not_applicable_scores_become_none_not_zero() {
        let value = json!({
            "CPUScore": 7.0,
            "GraphicsScore": -1.0,
            "WinSATAssessmentState": 1,
        });

        let scores = parse_winsat_json(&value).expect("scores");
        assert_eq!(scores.cpu_score, Some(7.0));
        assert_eq!(
            scores.graphics_score, None,
            "a -1 subsystem score means 'not measured', not a real zero"
        );
    }

    #[test]
    fn an_empty_object_means_no_instance_returned_at_all() {
        assert!(parse_winsat_json(&json!({})).is_none());
    }
}

#[cfg(test)]
mod winsat_live_check {
    #[test]
    #[ignore] // machine-dependent - run manually with --ignored
    fn live_winsat_scores_on_this_machine() {
        println!("current_scores() = {:?}", super::current_scores());
    }
}
