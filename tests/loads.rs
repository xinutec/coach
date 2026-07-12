//! Tests for load expansion — the pure `reachable_loads` / `loads_for`, exercised
//! through their public API (the service feeds their output to the pacing engine,
//! which may only ever prescribe a weight the athlete can physically assemble).

use coach::location::loads::{Bar, KitLoads, Plate, loads_for, reachable_loads};

fn has(v: &[f64], x: f64) -> bool {
    v.iter().any(|&w| (w - x).abs() < 1e-6)
}

/// Plates in the numbers a gym has: effectively unlimited.
fn plenty(sizes: &[f64]) -> Vec<Plate> {
    sizes.iter().map(|&kg| Plate { kg, qty: None }).collect()
}

fn plate(kg: f64, qty: u32) -> Plate {
    Plate { kg, qty: Some(qty) }
}

#[test]
fn floor_is_the_empty_bar() {
    let loads = reachable_loads(20.0, &plenty(&[1.25, 2.5, 5.0]), 1, None);
    assert_eq!(
        loads.first().copied(),
        Some(20.0),
        "never below the bare bar"
    );
    assert!(loads.windows(2).all(|w| w[0] < w[1]), "ascending");
}

#[test]
fn small_plates_give_fine_steps() {
    // A 1.25 kg plate on each side = a 2.5 kg total step.
    let loads = reachable_loads(20.0, &plenty(&[1.25]), 1, None);
    assert!(has(&loads, 22.5));
    assert!(has(&loads, 25.0));
    assert!(!has(&loads, 21.0), "1 kg isn't buildable from 1.25s");
}

#[test]
fn coarse_plates_give_coarse_steps() {
    // Only 5 kg plates → totals jump by 10 kg (5 per side).
    let loads = reachable_loads(20.0, &plenty(&[5.0]), 1, None);
    assert!(has(&loads, 30.0));
    assert!(!has(&loads, 25.0), "no half-jumps without a 2.5 plate");
}

#[test]
fn no_plates_falls_back_to_a_sane_step() {
    // Bar weight known but no plates entered → default to a 1.25 kg plate so the
    // bar still has a floor + a sane increment rather than pegging at the bar.
    let loads = reachable_loads(20.0, &[], 1, None);
    assert_eq!(loads.first().copied(), Some(20.0));
    assert!(has(&loads, 22.5));
}

#[test]
fn a_single_disc_cannot_be_loaded_at_all() {
    // Plates go on both ends or not at all: one 2.5 kg disc is dead weight.
    let loads = reachable_loads(20.0, &[plate(2.5, 1)], 1, None);
    assert_eq!(loads, vec![20.0], "an unpaired disc buys you nothing");
}

#[test]
fn you_cannot_load_more_plates_than_you_own() {
    // One pair of 2.5s: 2.5-per-side is reachable, 5-per-side is not. The old model
    // assumed unlimited plates and would cheerfully suggest a weight you can't build.
    let loads = reachable_loads(20.0, &[plate(2.5, 2)], 1, None);
    assert_eq!(
        loads,
        vec![20.0, 25.0],
        "the bar, and the bar plus your one pair — that's all there is"
    );
}

#[test]
fn a_sleeve_runs_out_of_space() {
    // Plenty of 1.25s, but only two fit on each sleeve → +2.5 a side, no further.
    let loads = reachable_loads(20.0, &plenty(&[1.25]), 1, Some(2));
    assert!(has(&loads, 25.0), "two discs a side");
    assert!(!has(&loads, 27.5), "a third disc doesn't fit");
}

// ---- the real home kit: 2 handles @ 1.66 kg, 4 each of 0.5 / 1.25 / 2.5 ------

fn home_handles() -> KitLoads {
    KitLoads {
        fixed: vec![(5.0, Some(1))], // one plain, non-adjustable 5 kg dumbbell
        bar: Some(Bar {
            kg: 1.66,
            qty: Some(2),   // two handles
            slots: Some(5), // five discs a side and the sleeve is full
        }),
        plates: vec![plate(0.5, 4), plate(1.25, 4), plate(2.5, 4)],
    }
}

#[test]
fn a_pair_of_dumbbells_splits_the_disc_budget() {
    // The whole point of the model. Four of each disc is *two* per dumbbell when the
    // movement needs two — and two discs make one pair — so each side of each
    // dumbbell takes at most one of each size: 0.5 + 1.25 + 2.5 = 4.25 a side.
    let pair = loads_for(&home_handles(), 2);
    let heaviest_of_pair = pair.last().copied().unwrap();
    assert!(
        (heaviest_of_pair - (1.66 + 2.0 * 4.25)).abs() < 1e-6,
        "a both-arms press tops out at {heaviest_of_pair} kg per dumbbell"
    );

    // One dumbbell gets the whole disc budget: two of each size a side, capped at
    // five discs by the sleeve → 2×2.5 + 2×1.25 + 1×0.5 = 8.0 a side.
    let single = loads_for(&home_handles(), 1);
    let heaviest_single = single.last().copied().unwrap();
    assert!(
        (heaviest_single - (1.66 + 2.0 * 8.0)).abs() < 1e-6,
        "a goblet squat reaches {heaviest_single} kg on one dumbbell"
    );
    assert!(
        heaviest_single > heaviest_of_pair,
        "one dumbbell must reach heavier than each of a pair — same discs, shared out"
    );
}

#[test]
fn a_lone_fixed_dumbbell_cannot_serve_a_two_dumbbell_movement() {
    let kit = home_handles(); // exactly one plain 5 kg dumbbell
    assert!(
        has(&loads_for(&kit, 1), 5.0),
        "a goblet squat can use the single 5 kg"
    );
    assert!(
        !has(&loads_for(&kit, 2), 5.0),
        "a two-dumbbell press can't — you only own one of them"
    );
}

#[test]
fn not_enough_handles_means_not_loadable() {
    // One handle can't do a two-dumbbell movement, and there's no fixed weight to
    // fall back on → nothing is buildable, so the caller must not prescribe it.
    let kit = KitLoads {
        fixed: vec![],
        bar: Some(Bar {
            kg: 1.66,
            qty: Some(1),
            slots: None,
        }),
        plates: vec![plate(2.5, 4)],
    };
    assert!(
        !loads_for(&kit, 1).is_empty(),
        "one handle, one dumbbell: fine"
    );
    assert!(
        loads_for(&kit, 2).is_empty(),
        "one handle, two dumbbells: nothing honest to prescribe"
    );
}
