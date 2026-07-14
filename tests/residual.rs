//! The prediction-error ledger and the progression it drives.
//!
//! Ability is a max over decayed sets, so before this a session that went badly
//! pulled nothing down — the athlete was handed the same load the sets had just
//! contradicted. These tests fix that behaviour in place: a miss holds, two misses
//! step down, three send the exercise back to being measured. And, as importantly,
//! a *good* history is unaffected — the ledger must not invent a miss out of an
//! ordinary session.

use chrono::{Duration, NaiveDate, NaiveDateTime};

use coach::pacing::residual::{Outcome, residuals};
use coach::pacing::types::SetRec;

fn day(n: i64) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        + Duration::days(n)
}

fn wset(day_n: i64, load: f64, reps: i32) -> SetRec {
    SetRec {
        exercise_id: 1,
        logged_at: day(day_n),
        reps: Some(reps),
        load_kg: Some(load),
        hold_s: None,
        rpe: None,
    }
}

fn ledger_for(sets: Vec<SetRec>) -> coach::pacing::residual::Residual {
    residuals(&sets).remove(&1).unwrap_or_default()
}

#[test]
fn a_steady_history_records_no_misses() {
    // The same solid session, three weeks running. Nothing is a miss — the estimate
    // describes him, which is the case the ledger must not cry wolf on.
    let r = ledger_for(vec![wset(0, 40.0, 5), wset(7, 40.0, 5), wset(14, 40.0, 5)]);
    assert_eq!(r.consecutive_misses, 0);
    assert!(!r.wants_hold() && !r.wants_back_off() && !r.wants_remeasure());
}

#[test]
fn improving_sessions_are_beats_not_misses() {
    let r = ledger_for(vec![wset(0, 40.0, 5), wset(7, 45.0, 5), wset(14, 50.0, 5)]);
    assert!(r.outcomes.iter().all(|o| *o != Outcome::Missed));
    assert_eq!(r.consecutive_misses, 0);
}

#[test]
fn a_single_bad_session_asks_for_a_hold_not_a_back_off() {
    // Two solid sessions set an estimate; the third comes in well under it.
    let r = ledger_for(vec![
        wset(0, 40.0, 5),
        wset(7, 40.0, 5),
        wset(14, 30.0, 5), // a clear miss
    ]);
    assert_eq!(r.consecutive_misses, 1);
    assert!(r.wants_hold(), "one miss holds the number");
    assert!(!r.wants_back_off(), "one miss is not yet a back-off");
    assert!(!r.wants_remeasure());
}

#[test]
fn two_misses_back_off_and_three_re_open_the_measurement() {
    let base = vec![wset(0, 40.0, 5), wset(7, 40.0, 5)];
    let two = [base.clone(), vec![wset(14, 30.0, 5), wset(21, 30.0, 5)]].concat();
    let r = ledger_for(two);
    assert_eq!(r.consecutive_misses, 2);
    assert!(r.wants_back_off());
    assert!(
        !r.wants_remeasure(),
        "two is a back-off, not yet a re-measure"
    );

    let three = [
        base,
        vec![wset(14, 30.0, 5), wset(21, 30.0, 5), wset(28, 30.0, 5)],
    ]
    .concat();
    let r = ledger_for(three);
    assert_eq!(r.consecutive_misses, 3);
    assert!(
        r.wants_remeasure(),
        "three misses running is a wrong estimate, so measure again"
    );
}

#[test]
fn a_good_session_after_misses_clears_the_streak() {
    // Miss, miss, then a session back at the estimate. The streak is what the engine
    // acts on, and a recovery answers it — a bad patch three weeks ago must not keep
    // holding him back once he's past it.
    let r = ledger_for(vec![
        wset(0, 40.0, 5),
        wset(7, 40.0, 5),
        wset(14, 30.0, 5),
        wset(21, 30.0, 5),
        wset(28, 40.0, 5), // back on it
    ]);
    assert_eq!(r.consecutive_misses, 0);
    assert!(!r.wants_hold());
}

#[test]
fn the_first_session_is_never_a_miss() {
    // Nothing preceded it, so there was no prediction to fall short of. A cold start
    // is a measurement, not a failure.
    let r = ledger_for(vec![wset(0, 40.0, 5)]);
    assert!(r.outcomes.is_empty());
    assert_eq!(r.consecutive_misses, 0);
}
