//! The weighted set-cover selector, exercised through its public surface.
//!
//! These pin the two needs that qualify a pick — coverage (paying down remaining
//! group volume) and confirmation (a one-time need to prove a started movement's
//! baseline) — and the novelty cap that keeps a calibration day from scattering
//! into one-off sets across every untrained group at once.

use coach::pacing::cover::{ByGroup, Candidate, GroupIx, select};

/// A dense need/credit vector from a plain list — built through the public API
/// (`filled` + indexed writes), since the backing store is private on purpose.
fn vec_of(v: Vec<f64>) -> ByGroup<f64> {
    let mut b = ByGroup::filled(v.len(), 0.0);
    for (i, x) in v.into_iter().enumerate() {
        b[GroupIx(i)] = x;
    }
    b
}

#[allow(clippy::too_many_arguments)]
fn cand(
    id: i64,
    credit: Vec<f64>,
    weight: f64,
    confirm: f64,
    novel: bool,
    min: i32,
    cap: i32,
) -> Candidate {
    Candidate {
        id,
        family: format!("ex{id}"),
        credit: vec_of(credit),
        weight,
        confirm,
        novel,
        min,
        cap,
    }
}

// R3-3: cousins of one movement family (farmers walk plain/suitcase/waiter, the
// hamstring-curl variants) train the same thing the same way — a session takes
// at most one of them, and the freed budget goes to whatever else still pays.
#[test]
fn one_pick_per_movement_family() {
    let mut a = cand(1, vec![1.0], 2.0, 0.0, false, 2, 4);
    let mut b = cand(2, vec![1.0], 2.0, 0.0, false, 2, 4);
    a.family = "Farmers walk".into();
    b.family = "Farmers walk".into();
    // Need deep enough that, without the family gate, the second cousin would
    // enter once the first hits its per-exercise cap.
    let chosen = select(&[a, b], &vec_of(vec![10.0]), 8, 5);
    assert_eq!(chosen.len(), 1, "one movement per family per session");
    assert_eq!(
        chosen[0].sets, 4,
        "the family's pick still earns its full dose"
    );
}

#[test]
fn confirmation_carries_a_movement_whose_group_is_already_covered() {
    // One group, no remaining coverage need. A plain candidate can't clear the bar
    // (coverage 0). One with a confirmation need does — and takes its minimum dose,
    // flagged as a confirmation, with a truthful zero coverage.
    let cands = vec![
        cand(1, vec![1.0], 2.0, 0.0, false, 2, 4), // pure coverage: 0 pay
        cand(2, vec![1.0], 2.0, 5.0, false, 2, 4), // confirmation: enters
    ];
    let chosen = select(&cands, &vec_of(vec![0.0]), 10, 5);
    assert_eq!(
        chosen.len(),
        1,
        "only the confirmable movement is selectable"
    );
    assert_eq!(cands[chosen[0].index].id, 2);
    assert_eq!(
        chosen[0].sets, 2,
        "confirmation takes the minimum effective dose"
    );
    assert!(chosen[0].confirming);
    assert_eq!(
        chosen[0].pays, 0.0,
        "coverage pays stays truthful — it paid no volume"
    );
}

#[test]
fn confirmation_bonus_applies_only_to_the_entering_set() {
    // A covered group: the movement enters on confirmation (2 sets), but its *third*
    // set would be judged on coverage alone (0) and never comes — the bonus is spent
    // once, it doesn't pad a movement to its cap.
    let cands = vec![cand(1, vec![1.0], 1.0, 5.0, false, 2, 4)];
    let chosen = select(&cands, &vec_of(vec![0.0]), 10, 5);
    assert_eq!(
        chosen[0].sets, 2,
        "min dose only; confirm doesn't refill each set"
    );
}

#[test]
fn coverage_qualifies_a_pick_without_marking_it_confirming() {
    // Real remaining need → the pick earns its place on volume; the confirm value is
    // present but the flag stays off because coverage alone cleared it.
    let cands = vec![cand(1, vec![1.0], 1.0, 5.0, false, 2, 4)];
    let chosen = select(&cands, &vec_of(vec![3.0]), 10, 5);
    assert!(!chosen[0].confirming);
    assert_eq!(chosen[0].pays, 3.0);
}

#[test]
fn the_novelty_cap_bounds_how_many_new_movements_enter() {
    // Four untrained groups each want work, one never-done movement apiece. With a
    // cap of two, only two are introduced — the rest of the need is left for a later
    // session rather than scattered across the day.
    let cands = vec![
        cand(1, vec![1.0, 0.0, 0.0, 0.0], 1.0, 0.0, true, 1, 1),
        cand(2, vec![0.0, 1.0, 0.0, 0.0], 1.0, 0.0, true, 1, 1),
        cand(3, vec![0.0, 0.0, 1.0, 0.0], 1.0, 0.0, true, 1, 1),
        cand(4, vec![0.0, 0.0, 0.0, 1.0], 1.0, 0.0, true, 1, 1),
    ];
    let chosen = select(&cands, &vec_of(vec![3.0, 3.0, 3.0, 3.0]), 10, 2);
    assert_eq!(chosen.len(), 2, "the cap holds new movements to two");
}

#[test]
fn a_non_novel_movement_is_never_held_back_by_the_novelty_cap() {
    // The cap is about *new* movements only; a known one always competes on need.
    let cands = vec![
        cand(1, vec![1.0, 0.0], 1.0, 0.0, true, 1, 1),
        cand(2, vec![0.0, 1.0], 1.0, 0.0, false, 2, 4),
    ];
    let chosen = select(&cands, &vec_of(vec![3.0, 3.0]), 10, 0);
    // Novelty cap 0 blocks the novel one entirely, but the known one is picked.
    let ids: Vec<i64> = chosen.iter().map(|c| cands[c.index].id).collect();
    assert_eq!(
        ids,
        vec![2],
        "cap 0 drops the novel pick, keeps the known one"
    );
}

#[test]
fn the_budget_remainder_never_starts_a_movement_below_its_minimum_dose() {
    // Two movements on disjoint groups, both with a 2-set minimum, budget 3.
    // The remainder after the first pick is one set — not enough to *commit* to
    // the second movement, so it must not appear as a 1-set orphan ("Push-up —
    // 1 set", round-3 field test). The spare set tops up the first movement
    // instead: its marginal gain was just re-ranked and it's already set up.
    let cands = vec![
        cand(1, vec![1.0, 0.0], 2.0, 0.0, false, 2, 4),
        cand(2, vec![0.0, 1.0], 2.0, 0.0, false, 2, 4),
    ];
    let chosen = select(&cands, &vec_of(vec![3.0, 3.0]), 3, 5);
    for c in &chosen {
        assert!(
            c.sets >= cands[c.index].min,
            "movement {} entered with {} sets — below its minimum dose",
            cands[c.index].id,
            c.sets
        );
    }
    let total: i32 = chosen.iter().map(|c| c.sets).sum();
    assert_eq!(total, 3, "the spare set tops up an existing pick");
}

#[test]
fn a_one_set_calibration_still_fits_a_one_set_remainder() {
    // A calibration's full dose IS one set (min = cap = 1) — the remainder rule
    // must not lock it out.
    let cands = vec![
        cand(1, vec![1.0, 0.0], 2.0, 0.0, false, 2, 4),
        cand(2, vec![0.0, 1.0], 2.0, 0.0, true, 1, 1),
    ];
    let chosen = select(&cands, &vec_of(vec![3.0, 3.0]), 3, 5);
    let calib = chosen.iter().find(|c| cands[c.index].id == 2);
    assert!(
        calib.is_some_and(|c| c.sets == 1),
        "the calibration takes the last set: {:?}",
        chosen
            .iter()
            .map(|c| (cands[c.index].id, c.sets))
            .collect::<Vec<_>>()
    );
}
