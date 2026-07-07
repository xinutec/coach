//! Tests for barbell load expansion — the pure `reachable_loads`, exercised
//! through its public API (the repo feeds its output to the pacing engine).

use coach::location::loads::reachable_loads;

fn has(v: &[f64], x: f64) -> bool {
    v.iter().any(|&w| (w - x).abs() < 1e-6)
}

#[test]
fn floor_is_the_empty_bar() {
    let loads = reachable_loads(20.0, &[1.25, 2.5, 5.0]);
    assert_eq!(
        loads.first().copied(),
        Some(20.0),
        "never below the bare bar"
    );
    // Ascending.
    assert!(loads.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn small_plates_give_fine_steps() {
    // A 1.25 kg plate on each side = a 2.5 kg total step.
    let loads = reachable_loads(20.0, &[1.25]);
    assert!(has(&loads, 20.0));
    assert!(has(&loads, 22.5));
    assert!(has(&loads, 25.0));
    assert!(!has(&loads, 21.0), "1 kg isn't buildable from 1.25s");
}

#[test]
fn coarse_plates_give_coarse_steps() {
    // Only 5 kg plates → totals jump by 10 kg (5 per side).
    let loads = reachable_loads(20.0, &[5.0]);
    assert!(has(&loads, 20.0));
    assert!(has(&loads, 30.0));
    assert!(has(&loads, 40.0));
    assert!(!has(&loads, 25.0), "no half-jumps without a 2.5 plate");
}

#[test]
fn combinations_are_reachable() {
    // 2.5 and 5 kg plates → every multiple of 2.5 per side (5 kg total steps).
    let loads = reachable_loads(15.0, &[2.5, 5.0]);
    assert_eq!(loads.first().copied(), Some(15.0));
    assert!(has(&loads, 20.0)); // +2.5 each side
    assert!(has(&loads, 25.0)); // +5 each side
    assert!(!has(&loads, 17.5), "1.25 per side needs a 1.25 plate");
}

#[test]
fn no_plates_falls_back_to_a_sane_step() {
    // Bar weight known but no plates entered → default to a 1.25 kg plate so the
    // bar still has a floor + a sane increment rather than pegging at the bar.
    let loads = reachable_loads(20.0, &[]);
    assert_eq!(loads.first().copied(), Some(20.0));
    assert!(has(&loads, 22.5));
}
