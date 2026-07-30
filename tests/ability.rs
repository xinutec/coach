//! Ability-model tests: the pure estimator of what the athlete can do today,
//! exercised through its public API (`abilities` / `confidence_of`). Expected
//! numbers are computed inline from the documented formula (RPE-aware Epley +
//! per-set staleness decay), so the model's internals stay private.

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, NaiveDateTime};

use coach::pacing::ability::{Confidence, abilities, confidence_of};
use coach::pacing::types::SetRec;
use coach_pacing::domain::{ExerciseId, SetId};

const DECAY_FLOOR: f64 = 0.60; // must track ability.rs (checked via the floor test)
const CAP_MULTIPLE: f64 = 1.15; // must track ability.rs (checked via the decline tests)

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
        id: SetId(0),
        exercise_id: ExerciseId(id),
        logged_at: at(days_ago),
        reps: Some(reps),
        load_kg: Some(load),
        hold_s: None,
        rpe,
    }
}
fn bodyweight(id: i64, days_ago: i64, reps: i32, rpe: Option<i32>) -> SetRec {
    SetRec {
        id: SetId(0),
        exercise_id: ExerciseId(id),
        logged_at: at(days_ago),
        reps: Some(reps),
        load_kg: None,
        hold_s: None,
        rpe,
    }
}
fn hold(id: i64, days_ago: i64, secs: i32) -> SetRec {
    SetRec {
        id: SetId(0),
        exercise_id: ExerciseId(id),
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
    assert!((a[&ExerciseId(1)].e1rm.unwrap() - 70.0).abs() < 1e-9); // 60 × (1 + 5/30)
    assert_eq!(a[&ExerciseId(1)].confidence, Confidence::Medium);
}

#[test]
fn rpe_makes_a_reserved_set_worth_more() {
    // Same load+reps: RPE 7 (3 in reserve) implies more strength than RPE 10.
    let hard = abilities(&[weighted(1, 1, 60.0, 5, Some(10))], base())[&ExerciseId(1)]
        .e1rm
        .unwrap();
    let easy = abilities(&[weighted(1, 1, 60.0, 5, Some(7))], base())[&ExerciseId(1)]
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
    let e = a[&ExerciseId(1)].e1rm.unwrap();
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
    let fresh = abilities(&[weighted(1, 3, 100.0, 1, None)], base())[&ExerciseId(1)]
        .e1rm
        .unwrap();
    let ancient = abilities(&[weighted(1, 365, 100.0, 1, None)], base())[&ExerciseId(1)]
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
        abilities(&[weighted(1, d, 80.0, 3, None)], base())[&ExerciseId(1)]
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
    assert!((a[&ExerciseId(1)].e1rm.unwrap() - fresh.max(old_pr)).abs() < 1e-9);
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
    let est = a[&ExerciseId(1)].e1rm.unwrap();
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
        (a[&ExerciseId(1)].e1rm.unwrap() - heavy).abs() < 1e-9,
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
    assert_eq!(high[&ExerciseId(1)].confidence, Confidence::High);
    assert_eq!(high[&ExerciseId(1)].sessions_recent, 3);

    // Two sets on the *same* day → one session → Medium.
    let same_day = abilities(
        &[weighted(1, 2, 50.0, 5, None), weighted(1, 2, 55.0, 5, None)],
        base(),
    );
    assert_eq!(same_day[&ExerciseId(1)].confidence, Confidence::Medium);
    assert_eq!(same_day[&ExerciseId(1)].sessions_recent, 1);

    // Only ancient data → Low (an estimate exists, but nothing recent).
    let stale = abilities(&[weighted(1, 120, 50.0, 5, None)], base());
    assert_eq!(stale[&ExerciseId(1)].confidence, Confidence::Low);
    assert_eq!(stale[&ExerciseId(1)].sessions_recent, 0);
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
    assert_eq!(a[&ExerciseId(1)].best_reps, Some(14));
    assert!(a[&ExerciseId(1)].e1rm.is_none());
    assert_eq!(a[&ExerciseId(2)].best_hold, Some(45));
}

#[test]
fn never_trained_is_absent_and_reads_as_none() {
    let a: BTreeMap<_, _> = abilities(&[weighted(1, 1, 50.0, 5, None)], base());
    assert_eq!(confidence_of(&a, ExerciseId(1)), Confidence::Medium);
    assert_eq!(confidence_of(&a, ExerciseId(999)), Confidence::None);
}

// ---- provenance: which set set the estimate ---------------------------------

/// The estimate must name the set it came from. Ability is a max, so one wrong
/// number is a ceiling nothing later can lower — and it is only correctable if
/// the app can say which set produced it.
#[test]
fn the_estimate_names_the_set_it_came_from() {
    let best = SetRec {
        id: SetId(42),
        exercise_id: ExerciseId(1),
        logged_at: at(3),
        reps: Some(5),
        load_kg: Some(80.0),
        hold_s: None,
        rpe: None,
    };
    let lighter = SetRec {
        id: SetId(43),
        exercise_id: ExerciseId(1),
        logged_at: at(1),
        reps: Some(8),
        load_kg: Some(40.0),
        hold_s: None,
        rpe: None,
    };
    let a = abilities(&[best, lighter], base());
    let src = a[&ExerciseId(1)]
        .source
        .expect("an estimate must name its set");
    assert_eq!(
        src.set_id,
        SetId(42),
        "the heavier set is what set the estimate"
    );
    assert_eq!(src.load_kg, Some(80.0));
    assert_eq!(src.reps, Some(5));
}

/// The failure this exists for: the set that defines the estimate is usually
/// *old*, so anything that only offers the latest set cannot reach it.
#[test]
fn it_names_an_old_set_when_that_is_what_defines_the_estimate() {
    // A 140 kg slip weeks back, and a couple of sessions of honest 40 kg work
    // since — not yet the run of sessions it takes to form a ceiling over it.
    let mut h = vec![SetRec {
        id: SetId(7),
        exercise_id: ExerciseId(1),
        logged_at: at(40),
        reps: Some(8),
        load_kg: Some(140.0),
        hold_s: None,
        rpe: None,
    }];
    h.extend((0..2).map(|d| weighted(1, d * 2, 40.0, 8, None)));

    let a = abilities(&h, base());
    let src = a[&ExerciseId(1)]
        .source
        .expect("an estimate must name its set");
    assert_eq!(
        src.set_id,
        SetId(7),
        "the old outlier is still the max — the card must point at it"
    );
    assert_eq!(src.load_kg, Some(140.0));
}

/// The other half of the same story: once enough recent sessions disagree with the
/// outlier, they overrule it — and the number the athlete is shown then comes from
/// *them*, so that is the set `source` must name. The old slip is no longer worth
/// pointing at; it has already lost.
#[test]
fn a_capped_estimate_names_the_recent_set_that_caps_it() {
    let mut h = vec![SetRec {
        id: SetId(7),
        exercise_id: ExerciseId(1),
        logged_at: at(40),
        reps: Some(8),
        load_kg: Some(140.0),
        hold_s: None,
        rpe: None,
    }];
    h.extend((0..3).map(|d| weighted(1, d * 2, 40.0, 8, None)));

    let a = abilities(&h, base());
    let est = a[&ExerciseId(1)].e1rm.expect("an estimate");
    assert!(
        (est - CAP_MULTIPLE * e1rm(40.0, 8, None)).abs() < 1e-9,
        "three sessions of 40 kg put a ceiling over the 140 kg slip, got {est}"
    );
    let src = a[&ExerciseId(1)]
        .source
        .expect("an estimate must name its set");
    assert_ne!(
        src.set_id,
        SetId(7),
        "the outlier no longer sets the number"
    );
    assert_eq!(src.load_kg, Some(40.0));
}

/// R6-3, the finding this cap exists for: getting weaker has to be representable.
/// Ability is a max, so the honest low measurement loses to the very number it is
/// trying to correct — and the coach then spends months prescribing a strength the
/// athlete has already disproved, re-measuring it, and discarding the answer.
#[test]
fn a_sustained_decline_lowers_the_estimate() {
    // A real 100 kg × 5 a fortnight ago, then three sessions that all say 40 kg is
    // the truth now. Nothing here is a bad day — it is every session since.
    let mut h = vec![weighted(1, 14, 100.0, 5, None)];
    h.extend([3, 2, 1].map(|d| weighted(1, d, 40.0, 5, None)));

    let est = abilities(&h, base())[&ExerciseId(1)]
        .e1rm
        .expect("an estimate");
    let shown = e1rm(40.0, 5, None);
    assert!(
        est < e1rm(100.0, 5, None),
        "the old PR cannot still be the estimate, got {est}"
    );
    assert!(
        (est - CAP_MULTIPLE * shown).abs() < 1e-9,
        "the estimate settles a little above what recent work shows, got {est}"
    );
}

/// The shape R6-3 actually took in the field test: a bodyweight movement believed
/// at 7 reps against a true 5, held for fifteen straight sessions.
#[test]
fn a_decline_in_bodyweight_reps_is_representable_too() {
    let mut h = vec![bodyweight(2, 14, 12, None)];
    h.extend([3, 2, 1].map(|d| bodyweight(2, d, 5, None)));

    let est = abilities(&h, base())[&ExerciseId(2)]
        .best_reps
        .expect("an estimate");
    // 1.15 × 5 = 5.75, floored — reps are only ever claimed whole, and downwards.
    assert_eq!(
        est, 5,
        "the estimate lands on what recent work shows, not the old 12"
    );
}

/// The ceiling reads the *best* of the recent sessions, never the latest. A single
/// light day — or the coach's own low-readiness easing, which asks for less and
/// then sees less — must not ratchet the athlete downwards and pin them there.
#[test]
fn one_light_session_does_not_lower_the_estimate() {
    let h = vec![
        weighted(1, 14, 100.0, 5, None),
        weighted(1, 3, 40.0, 5, None),
        weighted(1, 2, 100.0, 5, None), // the strength is still there
        weighted(1, 1, 40.0, 5, None),
    ];

    let est = abilities(&h, base())[&ExerciseId(1)]
        .e1rm
        .expect("an estimate");
    assert!(
        (est - e1rm(100.0, 5, None)).abs() < 1e-9,
        "one good day inside the window is enough to hold the estimate up, got {est}"
    );
}

/// The boundary: a ceiling forms only once the decline has a *run* of sessions
/// behind it. With the PR still among the last few sessions there is no run
/// disagreeing with it — one or two lighter days are ordinary training variation.
#[test]
fn the_ceiling_forms_only_once_the_decline_has_a_run_of_sessions() {
    let h = vec![
        weighted(1, 14, 100.0, 5, None),
        weighted(1, 2, 40.0, 5, None),
        weighted(1, 1, 40.0, 5, None),
    ];

    let est = abilities(&h, base())[&ExerciseId(1)]
        .e1rm
        .expect("an estimate");
    assert!(
        (est - e1rm(100.0, 5, None)).abs() < 1e-9,
        "two lighter days do not yet overrule the PR, got {est}"
    );
}

/// Bodyweight rep work names its set too.
#[test]
fn a_rep_estimate_names_its_set() {
    let h = vec![SetRec {
        id: SetId(9),
        exercise_id: ExerciseId(2),
        logged_at: at(1),
        reps: Some(12),
        load_kg: None,
        hold_s: None,
        rpe: None,
    }];
    let src = abilities(&h, base())[&ExerciseId(2)].source.unwrap();
    assert_eq!(src.set_id, SetId(9));
    assert_eq!(src.reps, Some(12));
}
