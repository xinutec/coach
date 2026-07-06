//! Coach owns the readiness judgment. health-sync hands over *raw* recovery data
//! (latest value + a trailing baseline per biometric); this pure function turns
//! that into a single 0..1 score + a band, which the engine uses to autoregulate
//! volume + progression. Kept separate + pure so the formula is tunable and
//! unit-tested without a network or a DB.
//!
//! Per signal we need a real baseline (`n >= MIN_BASELINE_N`, `sd > 0`) before we
//! trust a z-score; signals without one are dropped and the weights renormalise
//! over whatever's present. No usable signal at all → `None` (engine falls back
//! to its volume-spike heuristic).

use crate::health::{Recovery, Stat};
use crate::pacing::types::{Band, Readiness};

/// Days of baseline history required before a metric's z-score is trusted.
const MIN_BASELINE_N: i64 = 7;

// Signal weights (renormalised over the signals actually present). Sleep leads —
// it's the most reliable and the strongest same-day recovery lever.
const W_SLEEP: f64 = 0.40;
const W_HRV: f64 = 0.35;
const W_RHR: f64 = 0.25;

const BAND_LOW: f64 = 0.40;
const BAND_HIGH: f64 = 0.65;

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

/// Is this baseline usable for a z-score?
fn usable(s: &Stat) -> bool {
    s.n >= MIN_BASELINE_N && s.sd > 0.0
}

/// HRV: higher-than-baseline is better (more recovered).
fn hrv_term(s: &Stat) -> Option<f64> {
    usable(s).then(|| sigmoid((s.latest - s.mean) / s.sd))
}

/// Resting HR: lower-than-baseline is better (more recovered).
fn rhr_term(s: &Stat) -> Option<f64> {
    usable(s).then(|| sigmoid((s.mean - s.latest) / s.sd))
}

/// Sleep: 5 h → 0, 8 h → 1 (linear, clamped). Absolute, so it needs no baseline.
fn sleep_term(hours: f64) -> f64 {
    ((hours - 5.0) / 3.0).clamp(0.0, 1.0)
}

/// Compose the raw recovery streams into a readiness verdict, or `None` when no
/// stream carries a usable signal.
pub fn readiness(r: &Recovery) -> Option<Readiness> {
    let mut num = 0.0;
    let mut den = 0.0;

    if let Some(h) = r.sleep_hours {
        num += W_SLEEP * sleep_term(h);
        den += W_SLEEP;
    }
    if let Some(t) = r.hrv.as_ref().and_then(hrv_term) {
        num += W_HRV * t;
        den += W_HRV;
    }
    if let Some(t) = r.resting_hr.as_ref().and_then(rhr_term) {
        num += W_RHR * t;
        den += W_RHR;
    }

    if den <= 0.0 {
        return None;
    }
    let score = num / den;
    let band = if score < BAND_LOW {
        Band::Low
    } else if score > BAND_HIGH {
        Band::High
    } else {
        Band::Normal
    };
    Some(Readiness { score, band })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(latest: f64, mean: f64, sd: f64, n: i64) -> Stat {
        Stat {
            latest,
            mean,
            sd,
            n,
        }
    }

    #[test]
    fn no_streams_is_none() {
        let r = Recovery {
            sleep_hours: None,
            hrv: None,
            resting_hr: None,
        };
        assert!(readiness(&r).is_none());
    }

    #[test]
    fn thin_baseline_is_dropped() {
        // HRV way above baseline but only 3 days of history → not trusted, and
        // it's the only stream → no usable signal.
        let r = Recovery {
            sleep_hours: None,
            hrv: Some(stat(80.0, 40.0, 5.0, 3)),
            resting_hr: None,
        };
        assert!(readiness(&r).is_none());
    }

    #[test]
    fn good_recovery_scores_high() {
        let r = Recovery {
            sleep_hours: Some(8.5),                      // >8h → 1.0
            hrv: Some(stat(60.0, 45.0, 10.0, 14)),       // +1.5 sd → sigmoid ~0.82
            resting_hr: Some(stat(50.0, 56.0, 4.0, 14)), // -1.5 sd → sigmoid ~0.82
        };
        let out = readiness(&r).unwrap();
        assert!(out.score > BAND_HIGH, "score {} should be high", out.score);
        assert_eq!(out.band, Band::High);
    }

    #[test]
    fn poor_recovery_scores_low() {
        let r = Recovery {
            sleep_hours: Some(4.5),                      // <5h → 0.0
            hrv: Some(stat(30.0, 45.0, 10.0, 14)),       // -1.5 sd → ~0.18
            resting_hr: Some(stat(62.0, 56.0, 4.0, 14)), // +1.5 sd → ~0.18
        };
        let out = readiness(&r).unwrap();
        assert!(out.score < BAND_LOW, "score {} should be low", out.score);
        assert_eq!(out.band, Band::Low);
    }

    #[test]
    fn at_baseline_is_normal() {
        // Every z-score at baseline → sigmoid 0.5; sleep 6.5h → 0.5. Mean 0.5.
        let r = Recovery {
            sleep_hours: Some(6.5),
            hrv: Some(stat(45.0, 45.0, 10.0, 14)),
            resting_hr: Some(stat(56.0, 56.0, 4.0, 14)),
        };
        let out = readiness(&r).unwrap();
        assert!((out.score - 0.5).abs() < 1e-9);
        assert_eq!(out.band, Band::Normal);
    }

    #[test]
    fn renormalises_over_present_signals() {
        // Only sleep present → score is exactly the sleep term, weight-independent.
        let r = Recovery {
            sleep_hours: Some(8.0),
            hrv: None,
            resting_hr: None,
        };
        let out = readiness(&r).unwrap();
        assert!((out.score - 1.0).abs() < 1e-9);
    }
}
