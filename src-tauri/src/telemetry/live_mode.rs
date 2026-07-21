use serde::Serialize;
use std::collections::VecDeque;
use sysinfo::{ProcessesToUpdate, System};

use crate::optimizations::detection::{foreground_pid, normalize_process_name};
use crate::telemetry::network;

/// Executable names recognized as streaming/broadcast software. This is a
/// separate list from optimizations::detection's game list on purpose -
/// classifying OBS as "maybe a game" would be wrong, and streaming apps
/// have their own detection semantics (network-focused, not CPU/priority).
const KNOWN_STREAMING_APPS: &[&str] = &[
    "obs64.exe",
    "obs32.exe",
    "obs.exe",
    "streamlabs obs.exe",
    "streamlabs desktop.exe",
    "xsplit.core.exe",
    "xsplit.gamecaster.exe",
    "wirecast.exe",
    "nvidia broadcast.exe",
    "twitch studio.exe",
];

/// Samples closer together than this are treated as the same tick for
/// "recent window" math (avoids div-by-zero-ish edge cases on a tiny buffer).
const RECENT_WINDOW_SAMPLES: usize = 15;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveModeSample {
    pub timestamp: i64,
    pub ping_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BitrateRecommendation {
    pub recommended_kbps: u32,
    /// 0..1 - lower when there's little data or signals conflict. Never
    /// presented as more than an estimate - see `reason`.
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbableCause {
    pub label: String,
    pub confidence: f64,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentReport {
    pub generated_at: i64,
    pub causes: Vec<ProbableCause>,
    pub sample_count: usize,
}

/// Takes one lightweight network reading (2 ping probes - see
/// network::collect_network_sample) - cheap enough to run every couple of
/// seconds while Live Mode is active, unlike the full adapter/Wi-Fi scan
/// used elsewhere in the app.
pub fn sample_now() -> LiveModeSample {
    let diagnostics = network::collect_network_sample();
    LiveModeSample {
        timestamp: chrono::Utc::now().timestamp(),
        ping_ms: diagnostics.external_latency_ms,
        jitter_ms: diagnostics.jitter_ms,
        packet_loss_percent: diagnostics.packet_loss_percent,
    }
}

/// Best-effort streaming-app match against the current foreground window.
/// Returns the matched executable name, or None if nothing recognized is in
/// the foreground right now (including when no foreground process could be
/// read at all - never guesses).
pub fn detect_foreground_streaming_app() -> Option<String> {
    let pid = foreground_pid()?;
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let process = system
        .processes()
        .iter()
        .find_map(|(candidate_pid, process)| (candidate_pid.as_u32() == pid).then_some(process))?;
    let name = process.name().to_string_lossy().trim().to_string();
    let normalized = normalize_process_name(&name);

    KNOWN_STREAMING_APPS
        .iter()
        .any(|candidate| *candidate == normalized)
        .then_some(name)
}

/// Conservative, clearly-labeled estimate - never a value AnalystBlaze
/// applies anywhere itself (there is no OBS/Streamlabs integration). Starts
/// from a common 1080p60 reference point (6000 kbps, in line with what
/// Twitch/YouTube commonly recommend) and scales it down only when recent
/// samples show real instability, floored at a safe minimum instead of
/// recommending something unusably low from a couple of bad samples.
pub fn recommend_bitrate(samples: &VecDeque<LiveModeSample>) -> Option<BitrateRecommendation> {
    if samples.is_empty() {
        return None;
    }
    let recent: Vec<&LiveModeSample> = samples.iter().rev().take(RECENT_WINDOW_SAMPLES).collect();

    let avg_loss = average(recent.iter().filter_map(|sample| sample.packet_loss_percent));
    let avg_jitter = average(recent.iter().filter_map(|sample| sample.jitter_ms));
    let avg_ping = average(recent.iter().filter_map(|sample| sample.ping_ms));

    const BASELINE_KBPS: f64 = 6000.0;
    const MIN_KBPS: f64 = 1500.0;

    let mut factor = 1.0_f64;
    let mut reasons = Vec::new();

    if let Some(loss) = avg_loss {
        if loss >= 5.0 {
            factor *= 0.5;
            reasons.push(format!("perda de pacotes media de {loss:.1}% nos ultimos ~30s"));
        } else if loss >= 1.5 {
            factor *= 0.75;
            reasons.push(format!("perda de pacotes de {loss:.1}%"));
        }
    }
    if let Some(jitter) = avg_jitter {
        if jitter >= 40.0 {
            factor *= 0.7;
            reasons.push(format!("jitter alto ({jitter:.0} ms)"));
        } else if jitter >= 20.0 {
            factor *= 0.85;
            reasons.push(format!("jitter moderado ({jitter:.0} ms)"));
        }
    }
    if let Some(ping) = avg_ping {
        if ping >= 150.0 {
            factor *= 0.85;
            reasons.push(format!("latencia elevada ({ping:.0} ms)"));
        }
    }

    let recommended_kbps = (BASELINE_KBPS * factor).clamp(MIN_KBPS, BASELINE_KBPS).round() as u32;
    let reason = if reasons.is_empty() {
        "Rede estavel nos ultimos ~30s - valor de referencia para 1080p60. Estimativa local, ajuste no seu software de transmissao.".to_string()
    } else {
        format!(
            "Reduzido por: {}. Estimativa local com base na sua rede, ajuste no seu software de transmissao - o AnalystBlaze nao controla o encoder.",
            reasons.join(", ")
        )
    };
    let confidence = confidence_from_sample_count(recent.len());

    Some(BitrateRecommendation {
        recommended_kbps,
        confidence,
        reason,
    })
}

fn confidence_from_sample_count(count: usize) -> f64 {
    (0.35 + (count as f64 / RECENT_WINDOW_SAMPLES as f64) * 0.45).clamp(0.35, 0.8)
}

fn average(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0usize;
    for value in values {
        sum += value;
        count += 1;
    }
    (count > 0).then_some(sum / count as f64)
}

/// Anomaly-triggered (auto) or manual (on-demand) incident report: ranks
/// probable causes from the samples buffer, each with the concrete evidence
/// behind it. Never suggests a fix that requires a Critical action (Winsock
/// reset, etc.) - only observations the user can act on themselves.
pub fn build_incident_report(samples: &VecDeque<LiveModeSample>) -> IncidentReport {
    let recent: Vec<&LiveModeSample> = samples.iter().rev().take(RECENT_WINDOW_SAMPLES).collect();
    let mut causes = Vec::new();

    if let Some(loss) = average(recent.iter().filter_map(|sample| sample.packet_loss_percent)) {
        if loss >= 3.0 {
            causes.push(ProbableCause {
                label: "Perda de pacotes na rede".to_string(),
                confidence: (0.5 + (loss / 20.0)).clamp(0.5, 0.92),
                evidence: format!("Perda media de {loss:.1}% nas ultimas amostras."),
            });
        }
    }
    if let Some(jitter) = average(recent.iter().filter_map(|sample| sample.jitter_ms)) {
        if jitter >= 25.0 {
            causes.push(ProbableCause {
                label: "Variacao de latencia (jitter)".to_string(),
                confidence: (0.45 + (jitter / 150.0)).clamp(0.45, 0.85),
                evidence: format!("Jitter medio de {jitter:.0} ms - conexao inconsistente."),
            });
        }
    }
    if let Some(ping) = average(recent.iter().filter_map(|sample| sample.ping_ms)) {
        if ping >= 120.0 {
            causes.push(ProbableCause {
                label: "Latencia elevada ate a internet".to_string(),
                confidence: (0.4 + (ping / 500.0)).clamp(0.4, 0.8),
                evidence: format!("Latencia media de {ping:.0} ms nas ultimas amostras."),
            });
        }
    }
    if causes.is_empty() {
        causes.push(ProbableCause {
            label: "Nenhuma instabilidade de rede detectada localmente".to_string(),
            confidence: 0.4,
            evidence: "As ultimas amostras nao mostram perda, jitter ou latencia fora do normal - a causa pode estar fora da rede local (CPU/GPU do encoder, lado do servico de streaming, etc.).".to_string(),
        });
    }

    causes.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

    IncidentReport {
        generated_at: chrono::Utc::now().timestamp(),
        causes,
        sample_count: recent.len(),
    }
}

/// True when the most recent samples look like a real anomaly worth an
/// automatic incident report (vs. a single noisy ping) - checked on every
/// tick of the Live Mode loop; see lib.rs::spawn_live_mode_loop.
pub fn detect_anomaly(samples: &VecDeque<LiveModeSample>) -> bool {
    let Some(latest) = samples.back() else {
        return false;
    };
    let recent_loss_spike = latest.packet_loss_percent.is_some_and(|loss| loss >= 8.0);
    let recent_latency_spike = latest.ping_ms.is_some_and(|ping| ping >= 250.0);
    recent_loss_spike || recent_latency_spike
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ping_ms: f64, jitter_ms: f64, loss: f64) -> LiveModeSample {
        LiveModeSample {
            timestamp: 0,
            ping_ms: Some(ping_ms),
            jitter_ms: Some(jitter_ms),
            packet_loss_percent: Some(loss),
        }
    }

    #[test]
    fn recommends_baseline_bitrate_on_stable_network() {
        let mut samples = VecDeque::new();
        for _ in 0..10 {
            samples.push_back(sample(20.0, 5.0, 0.0));
        }
        let recommendation = recommend_bitrate(&samples).expect("should recommend with samples present");
        assert_eq!(recommendation.recommended_kbps, 6000);
    }

    #[test]
    fn reduces_recommendation_on_high_packet_loss() {
        let mut samples = VecDeque::new();
        for _ in 0..10 {
            samples.push_back(sample(20.0, 5.0, 7.0));
        }
        let recommendation = recommend_bitrate(&samples).expect("should recommend with samples present");
        assert!(recommendation.recommended_kbps < 6000);
        assert!(recommendation.reason.contains("perda de pacotes"));
    }

    #[test]
    fn never_recommends_below_the_safety_floor() {
        let mut samples = VecDeque::new();
        for _ in 0..10 {
            samples.push_back(sample(400.0, 90.0, 15.0));
        }
        let recommendation = recommend_bitrate(&samples).expect("should recommend with samples present");
        assert!(recommendation.recommended_kbps >= 1500);
    }

    #[test]
    fn no_samples_means_no_recommendation() {
        assert!(recommend_bitrate(&VecDeque::new()).is_none());
    }

    #[test]
    fn incident_report_ranks_the_worst_cause_first() {
        let mut samples = VecDeque::new();
        for _ in 0..10 {
            samples.push_back(sample(30.0, 10.0, 12.0));
        }
        let report = build_incident_report(&samples);
        assert_eq!(report.causes[0].label, "Perda de pacotes na rede");
    }

    #[test]
    fn incident_report_is_honest_when_nothing_is_wrong() {
        let mut samples = VecDeque::new();
        for _ in 0..10 {
            samples.push_back(sample(15.0, 4.0, 0.0));
        }
        let report = build_incident_report(&samples);
        assert_eq!(report.causes.len(), 1);
        assert!(report.causes[0].label.contains("Nenhuma instabilidade"));
    }

    #[test]
    fn detects_anomaly_from_a_packet_loss_spike() {
        let mut samples = VecDeque::new();
        samples.push_back(sample(20.0, 5.0, 12.0));
        assert!(detect_anomaly(&samples));
    }

    #[test]
    fn does_not_flag_anomaly_on_normal_samples() {
        let mut samples = VecDeque::new();
        samples.push_back(sample(20.0, 5.0, 0.5));
        assert!(!detect_anomaly(&samples));
    }
}
