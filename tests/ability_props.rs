//! Property tests for the ability model: the two guarantees prescription leans
//! on for *every* history — ability never rises as an exercise sits idle
//! (monotone under time), and the estimate never exceeds what a real set
//! demonstrated (no fabrication, the chimera bug can't come back).

use chrono::{Duration, NaiveDate, NaiveDateTime};

use coach::pacing::ability::abilities;
use coach::pacing::types::SetRec;
use proptest::prelude::*;

fn base() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 7, 6)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
}

/// The undecayed RPE-aware Epley estimate for a set — the ceiling ability must
/// never exceed (ability decays each set, then takes the max).
fn raw_e1rm(load: f64, reps: i32, rpe: Option<i32>) -> f64 {
    let rir = rpe.map(|r| (10 - r).max(0) as f64).unwrap_or(0.0);
    load * (1.0 + (reps as f64 + rir) / 30.0)
}

// Weighted sets: (days_ago, load, reps, rpe?) all on one exercise (id 1).
type RawSet = (i64, f64, i32, Option<i32>);

fn weighted(raw: &[RawSet]) -> Vec<SetRec> {
    raw.iter()
        .map(|&(days_ago, load, reps, rpe)| SetRec {
            exercise_id: 1,
            logged_at: base() - Duration::days(days_ago),
            reps: Some(reps),
            load_kg: Some(load),
            hold_s: None,
            rpe,
        })
        .collect()
}

fn sets_strategy() -> impl Strategy<Value = Vec<RawSet>> {
    prop::collection::vec(
        (
            0i64..400,
            2.5f64..200.0,
            1i32..20,
            prop::option::of(5i32..10),
        ),
        1..30,
    )
}

proptest! {
    // No fabrication: the estimate never exceeds the best single real set's raw
    // e1RM — decaying-then-maxing can only pull down, never invent a bigger lift.
    #[test]
    fn e1rm_never_exceeds_the_best_real_set(raw in sets_strategy()) {
        let a = abilities(&weighted(&raw), base());
        let est = a[&1].e1rm.unwrap();
        let ceiling = raw
            .iter()
            .map(|&(_, load, reps, rpe)| raw_e1rm(load, reps, rpe))
            .fold(0.0_f64, f64::max);
        prop_assert!(est <= ceiling + 1e-6, "est {est} > ceiling {ceiling}");
    }

    // Monotone under idleness: evaluating the same history further in the future
    // (more days idle on every set) never raises the estimate.
    #[test]
    fn ability_never_rises_with_more_idle_time(raw in sets_strategy(), extra in 1i64..400) {
        let hist = weighted(&raw);
        let now = base();
        let later = base() + Duration::days(extra);
        let e_now = abilities(&hist, now)[&1].e1rm.unwrap();
        let e_later = abilities(&hist, later)[&1].e1rm.unwrap();
        prop_assert!(e_later <= e_now + 1e-6, "idle raised ability: {e_later} > {e_now}");
    }
}
