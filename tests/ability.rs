//! Ability-model tests: the pure estimator of what the athlete can do today,
//! exercised through its public API (`abilities` / `confidence_of`). Expected
//! numbers are computed inline from the documented formula (RPE-aware Epley +
//! per-set staleness decay), so the model's internals stay private.

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, NaiveDateTime};

use coach::pacing::ability::{Confidence, abilities, confidence_of};
use coach::pacing::types::SetRec;

const DECAY_FLOOR: f64 = 0.60; // must track ability.rs (checked via the floor test)

fn base() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 7, 6)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
}
fn at(days_ago: i64) -> NaiveDateTime {
    base() - Duration::days(days_ago)
}

/// RPE-aware Epley for a set, matching the model — for computing expected values.
fn e1rm(load: f64, reps: i32, rpe: Option<i32>) -> f64 {
    let rir = rpe.map(|r| (10 - r).max(0) as f64).unwrap_or(0.0);
    load * (1.0 + (reps as f64 + rir) / 30.0)
}

fn weighted(id: i64, days_ago: i64, load: f64, reps: i32, rpe: Option<i32>) -> SetRec {
    SetRec {
        id: 0,
        exercise_id: id,
        logged_at: at(days_ago),
        reps: Some(reps),
        load_kg: Some(load),
        hold_s: None,
        rpe,
    }
}
fn bodyweight(id: i64, days_ago: i64, reps: i32, rpe: Option<i32>) -> SetRec {
    SetRec {
        id: 0,
        exercise_id: id,
        logged_at: at(days_ago),
        reps: Some(reps),
        load_kg: None,
        hold_s: None,
        rpe,
    }
}
fn hold(id: i64, days_ago: i64, secs: i32) -> SetRec {
    SetRec {
        id: 0,
        exercise_id: id,
        logged_at: at(days_ago),
        reps: None,
        load_kg: None,
        hold_s: Some(secs),
        rpe: None,
    }
}

#[test]
fn fresh_weighted_set_is_taken_at_face_value() {
    let a = abilities(&[weighted(1, 1, 60.0, 5, None)], base());
    assert!((a[&1].e1rm.unwrap() - 70.0).abs() < 1e-9); // 60 × (1 + 5/30)
    assert_eq!(a[&1].confidence, Confidence::Medium);
}

#[test]
fn rpe_makes_a_reserved_set_worth_more() {
    // Same load+reps: RPE 7 (3 in reserve) implies more strength than RPE 10.
    let hard = abilities(&[weighted(1, 1, 60.0, 5, Some(10))], base())[&1]
        .e1rm
        .unwrap();
    let easy = abilities(&[weighted(1, 1, 60.0, 5, Some(7))], base())[&1]
        .e1rm
        .unwrap();
    assert!(easy > hard, "reserved set ({easy}) > grinding set ({hard})");
}

#[test]
fn never_fabricates_a_top_set_from_column_maxima() {
    // The chimera bug: 10×20 and 5×40 in one session must NOT yield 10×40.
    let a = abilities(
        &[
            weighted(1, 1, 20.0, 10, None),
            weighted(1, 1, 40.0, 5, None),
        ],
        base(),
    );
    let e = a[&1].e1rm.unwrap();
    let chimera = e1rm(40.0, 10, None); // 53.33…
    let real_best = e1rm(40.0, 5, None); // 46.66…
    assert!((e - real_best).abs() < 1e-9);
    assert!(
        e < chimera,
        "estimate {e} must stay below the chimera {chimera}"
    );
}

#[test]
fn stale_ability_decays_but_never_below_the_floor() {
    let raw = e1rm(100.0, 1, None); // 103.33…
    let fresh = abilities(&[weighted(1, 3, 100.0, 1, None)], base())[&1]
        .e1rm
        .unwrap();
    let ancient = abilities(&[weighted(1, 365, 100.0, 1, None)], base())[&1]
        .e1rm
        .unwrap();
    assert!((fresh - raw).abs() < 1e-9, "recent set undelayed");
    assert!(
        (ancient - raw * DECAY_FLOOR).abs() < 1e-9,
        "floored, not forgotten"
    );
}

#[test]
fn ability_is_monotone_under_idleness() {
    // Evaluating the same lone set later never raises its estimate.
    let est = |d| {
        abilities(&[weighted(1, d, 80.0, 3, None)], base())[&1]
            .e1rm
            .unwrap()
    };
    let mut prev = f64::INFINITY;
    for d in [1, 14, 21, 60, 200, 400] {
        let v = est(d);
        assert!(v <= prev + 1e-9, "idle {d}d: {v} should be ≤ {prev}");
        prev = v;
    }
}

#[test]
fn a_recent_set_can_override_a_decayed_old_pr() {
    // Old heavy PR decayed to floor vs a fresh, clearly-stronger set: max wins.
    let old_pr = e1rm(100.0, 1, None) * DECAY_FLOOR; // ≈ 62
    let fresh = e1rm(90.0, 3, None); // 99  > 62
    let a = abilities(
        &[
            weighted(1, 400, 100.0, 1, None),
            weighted(1, 1, 90.0, 3, None),
        ],
        base(),
    );
    assert!((a[&1].e1rm.unwrap() - fresh.max(old_pr)).abs() < 1e-9);
}

#[test]
fn a_long_break_resets_ability_to_the_recent_block() {
    // A strong old block, a long layoff, then a lighter return. Ability must read
    // from the *return*, not the decayed old PR — prescribing the old load to a
    // weaker (recovering) body would be unsafe. This is the case that matters most.
    let recent_light = e1rm(40.0, 5, None); // ≈ 47, the honest return level
    let old_pr = e1rm(100.0, 5, None) * DECAY_FLOOR; // the decayed 2024 ghost, far higher
    let a = abilities(
        &[
            weighted(1, 400, 100.0, 5, None), // old block, > a year ago
            weighted(1, 380, 100.0, 5, None),
            weighted(1, 3, 40.0, 5, None), // return block, this week
            weighted(1, 1, 40.0, 5, None),
        ],
        base(),
    );
    let est = a[&1].e1rm.unwrap();
    assert!(
        (est - recent_light).abs() < 1e-9,
        "estimate {est} must be the return level {recent_light}, not the old PR"
    );
    assert!(
        old_pr > recent_light,
        "the old ghost ({old_pr}) really is higher — the point of the reset"
    );
}

#[test]
fn a_light_set_within_a_block_does_not_erase_a_heavier_one() {
    // No break: a light technique/warm-up set today must not lower the estimate
    // below a heavier set a few days ago — within a block, the best set wins. (The
    // reset is for real interruptions, not normal training variation.)
    let heavy = e1rm(80.0, 5, None); // ≈ 93
    let a = abilities(
        &[
            weighted(1, 4, 80.0, 5, None), // heavy, 4 days ago
            weighted(1, 1, 30.0, 5, None), // light, today (same block)
        ],
        base(),
    );
    assert!(
        (a[&1].e1rm.unwrap() - heavy).abs() < 1e-9,
        "the heavier set in the block still defines ability"
    );
}

#[test]
fn confidence_counts_distinct_recent_days() {
    // Three separate days in the last six weeks → High.
    let high = abilities(
        &[
            weighted(1, 1, 50.0, 5, None),
            weighted(1, 3, 50.0, 5, None),
            weighted(1, 5, 50.0, 5, None),
        ],
        base(),
    );
    assert_eq!(high[&1].confidence, Confidence::High);
    assert_eq!(high[&1].sessions_recent, 3);

    // Two sets on the *same* day → one session → Medium.
    let same_day = abilities(
        &[weighted(1, 2, 50.0, 5, None), weighted(1, 2, 55.0, 5, None)],
        base(),
    );
    assert_eq!(same_day[&1].confidence, Confidence::Medium);
    assert_eq!(same_day[&1].sessions_recent, 1);

    // Only ancient data → Low (an estimate exists, but nothing recent).
    let stale = abilities(&[weighted(1, 120, 50.0, 5, None)], base());
    assert_eq!(stale[&1].confidence, Confidence::Low);
    assert_eq!(stale[&1].sessions_recent, 0);
}

#[test]
fn bodyweight_and_hold_estimates_track_their_metric() {
    let a = abilities(
        &[
            bodyweight(1, 1, 12, Some(8)), // 12 + 2 reserve = 14 eff reps
            hold(2, 1, 45),
        ],
        base(),
    );
    assert_eq!(a[&1].best_reps, Some(14));
    assert!(a[&1].e1rm.is_none());
    assert_eq!(a[&2].best_hold, Some(45));
}

#[test]
fn never_trained_is_absent_and_reads_as_none() {
    let a: BTreeMap<_, _> = abilities(&[weighted(1, 1, 50.0, 5, None)], base());
    assert_eq!(confidence_of(&a, 1), Confidence::Medium);
    assert_eq!(confidence_of(&a, 999), Confidence::None);
}

// ---- provenance: which set set the estimate ---------------------------------

/// The estimate must name the set it came from. Ability is a max, so one wrong
/// number is a ceiling nothing later can lower — and it is only correctable if
/// the app can say which set produced it.
#[test]
fn the_estimate_names_the_set_it_came_from() {
    let best = SetRec {
        id: 42,
        exercise_id: 1,
        logged_at: at(3),
        reps: Some(5),
        load_kg: Some(80.0),
        hold_s: None,
        rpe: None,
    };
    let lighter = SetRec {
        id: 43,
        exercise_id: 1,
        logged_at: at(1),
        reps: Some(8),
        load_kg: Some(40.0),
        hold_s: None,
        rpe: None,
    };
    let a = abilities(&[best, lighter], base());
    let src = a[&1].source.expect("an estimate must name its set");
    assert_eq!(src.set_id, 42, "the heavier set is what set the estimate");
    assert_eq!(src.load_kg, Some(80.0));
    assert_eq!(src.reps, Some(5));
}

/// The failure this exists for: the set that defines the estimate is usually
/// *old*, so anything that only offers the latest set cannot reach it.
#[test]
fn it_names_an_old_set_when_that_is_what_defines_the_estimate() {
    // A 140 kg slip weeks back, honest 40 kg work ever since.
    let mut h = vec![SetRec {
        id: 7,
        exercise_id: 1,
        logged_at: at(40),
        reps: Some(8),
        load_kg: Some(140.0),
        hold_s: None,
        rpe: None,
    }];
    h.extend((0..6).map(|d| weighted(1, d * 2, 40.0, 8, None)));

    let a = abilities(&h, base());
    let src = a[&1].source.expect("an estimate must name its set");
    assert_eq!(
        src.set_id, 7,
        "the old outlier is still the max — the card must point at it"
    );
    assert_eq!(src.load_kg, Some(140.0));
}

/// Bodyweight rep work names its set too.
#[test]
fn a_rep_estimate_names_its_set() {
    let h = vec![SetRec {
        id: 9,
        exercise_id: 2,
        logged_at: at(1),
        reps: Some(12),
        load_kg: None,
        hold_s: None,
        rpe: None,
    }];
    let src = abilities(&h, base())[&2].source.unwrap();
    assert_eq!(src.set_id, 9);
    assert_eq!(src.reps, Some(12));
}
