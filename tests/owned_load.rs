//! The log-time plausibility guard: a load far beyond anything the athlete owns
//! is a mistyped field, and the ability model (a max over history) cannot
//! unlearn it. These pin the decision rule; the query that finds the heaviest
//! buildable weight is covered by the DB tests.

use coach::location::owned::{IMPLAUSIBLE_FACTOR, implausible};

/// The case that motivated the guard: he benches 40 kg and types 140.
#[test]
fn a_digit_slip_is_implausible() {
    // Heaviest he owns for this movement: a 50 kg loaded bar.
    assert!(implausible(140.0, Some(50.0)));
}

/// An improvised weight is legitimate and must not be nagged about — the ledger
/// deliberately judges improvised loads honestly (R5).
#[test]
fn improvising_slightly_heavier_is_fine() {
    assert!(!implausible(55.0, Some(50.0)));
    assert!(!implausible(60.0, Some(50.0)));
    // Exactly at the threshold is still fine — only *past* it asks.
    assert!(!implausible(75.0, Some(50.0)));
    assert!(implausible(75.01, Some(50.0)));
}

/// Training at your own rack never trips it.
#[test]
fn a_weight_you_own_is_never_implausible() {
    for load in [2.5, 10.0, 27.5, 50.0] {
        assert!(
            !implausible(load, Some(50.0)),
            "{load} kg tripped the guard"
        );
    }
}

/// An unknown rack is not evidence of a typo. An athlete who has registered no
/// weights (or trains somewhere the catalog can't describe — G9) must be able to
/// log freely rather than confirm every single set.
#[test]
fn nothing_registered_never_asks() {
    assert!(!implausible(140.0, None));
    assert!(!implausible(1000.0, None));
    // A rack that registers as zero is the same non-answer, not a rack of zero.
    assert!(!implausible(140.0, Some(0.0)));
}

/// The factor is a labelled heuristic, not a magic number — pin it so a change
/// is a deliberate edit with a failing test to update.
#[test]
fn the_threshold_is_half_again() {
    assert!((IMPLAUSIBLE_FACTOR - 1.5).abs() < f64::EPSILON);
}
