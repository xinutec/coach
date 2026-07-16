//! A logged set's fields must fit its exercise's metric — the server-side half
//! of R2-1 (docs/field-test.md): a stale client field must not be able to store
//! "10 reps · 4 kg" against a bodyweight mobility drill.

use coach::exercise::types::Metric;
use coach::workout::types::NewSet;

fn set(reps: Option<i32>, load_kg: Option<f64>, hold_s: Option<i32>) -> NewSet {
    NewSet {
        exercise_id: 1,
        reps,
        load_kg,
        hold_s,
        rpe: None,
        note: None,
        logged_at: None,
    }
}

#[test]
fn a_bodyweight_drill_takes_no_load() {
    assert!(
        set(Some(10), Some(4.0), None)
            .shape_error(Metric::Reps)
            .is_some()
    );
    assert!(
        set(Some(10), None, None)
            .shape_error(Metric::Reps)
            .is_none()
    );
}

#[test]
fn a_hold_takes_seconds_not_reps() {
    assert!(
        set(Some(10), None, None)
            .shape_error(Metric::Hold)
            .is_some()
    );
    assert!(
        set(None, None, Some(30))
            .shape_error(Metric::Hold)
            .is_none()
    );
}

#[test]
fn a_weighted_lift_takes_load_and_reps_but_no_clock() {
    assert!(
        set(Some(5), Some(60.0), Some(30))
            .shape_error(Metric::WeightedReps)
            .is_some()
    );
    assert!(
        set(Some(5), Some(60.0), None)
            .shape_error(Metric::WeightedReps)
            .is_none()
    );
}

#[test]
fn a_carry_takes_load_and_seconds_but_no_reps() {
    assert!(
        set(Some(5), Some(24.0), Some(30))
            .shape_error(Metric::WeightedHold)
            .is_some()
    );
    assert!(
        set(None, Some(24.0), Some(30))
            .shape_error(Metric::WeightedHold)
            .is_none()
    );
}

#[test]
fn partial_data_within_the_metric_is_fine() {
    // Logging reps without a load on a weighted lift is honest (e.g. an empty-bar
    // technique set the athlete chose not to weigh) — the metric allows the field,
    // it doesn't demand it.
    assert!(
        set(Some(5), None, None)
            .shape_error(Metric::WeightedReps)
            .is_none()
    );
}

// ---- plausibility bounds (round 3, R3-1): values must describe something a
// human did. The field test stored a fat-fingered 3 530-second farmers walk.

#[test]
fn a_fifty_nine_minute_carry_is_not_a_set() {
    assert!(
        set(None, Some(12.0), Some(3530))
            .shape_error(Metric::WeightedHold)
            .is_some()
    );
    assert!(
        set(None, Some(12.0), Some(35))
            .shape_error(Metric::WeightedHold)
            .is_none()
    );
}

#[test]
fn zero_and_negative_values_are_refused() {
    assert!(set(Some(0), None, None).shape_error(Metric::Reps).is_some());
    assert!(
        set(Some(5), Some(-2.0), None)
            .shape_error(Metric::WeightedReps)
            .is_some()
    );
    assert!(set(None, None, Some(0)).shape_error(Metric::Hold).is_some());
}

#[test]
fn generous_but_finite_ceilings() {
    assert!(
        set(Some(500), None, None)
            .shape_error(Metric::Reps)
            .is_some()
    );
    assert!(
        set(Some(100), None, None)
            .shape_error(Metric::Reps)
            .is_none()
    );
    assert!(
        set(Some(5), Some(400.0), None)
            .shape_error(Metric::WeightedReps)
            .is_some()
    );
    assert!(
        set(None, None, Some(600))
            .shape_error(Metric::Hold)
            .is_none()
    );
}

#[test]
fn an_out_of_scale_rpe_is_refused() {
    let mut s = set(Some(5), None, None);
    s.rpe = Some(11);
    assert!(s.shape_error(Metric::Reps).is_some());
    s.rpe = Some(9);
    assert!(s.shape_error(Metric::Reps).is_none());
}
