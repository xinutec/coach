//! Readiness tests. Integration tests against the public `readiness` function —
//! the pure formula that turns health-sync's raw recovery streams into a 0..1
//! score + band, exercised through the same surface `service` uses.

use coach::health::{Recovery, Stat};
use coach::pacing::readiness::readiness;
use coach::pacing::types::Band;

// z-score bands mirror the constants in readiness.rs (kept private there).
const BAND_LOW: f64 = 0.40;
const BAND_HIGH: f64 = 0.65;

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
