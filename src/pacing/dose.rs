//! What to actually do for a chosen exercise — as types that make the wrong
//! thing unsayable.
//!
//! The prescription used to be a `(i32, Option<i32>, Option<i32>, Option<f64>,
//! Option<i32>)` tuple: five fields, thirty-two representable shapes, about three
//! legal ones. Every bug in this area lived in the gap — a weighted lift carrying
//! no load, a load conjured for a lift never performed, a "1 kg overhead press"
//! that was really the lightest dumbbell in the room standing in for an unknown.
//! Closing the gap is three types:
//!
//! - [`Inventory`] — the weights you own here, **non-empty by construction**. So
//!   [`Inventory::snap`] is total: it always returns a weight you actually own,
//!   and there is no "unknown inventory" branch to invent 13.5 kg from. An
//!   exercise needing load where no weights are registered isn't loadable, and
//!   the engine simply doesn't select it (and says so) rather than guessing.
//! - [`Dose`] / [`Measure`] — a sum type per metric, so a weighted lift *has* a
//!   `load: f64` (not an `Option`), a bodyweight lift has no load field at all,
//!   and a hold has seconds.
//! - [`Known`] — an ability estimate the engine trusts. `prescribe` takes one *by
//!   type*, and the only constructor checks confidence. "When I don't know what
//!   you can do, I measure instead of guessing" is the safety principle that
//!   keeps a returning athlete off their pre-illness numbers; it is now enforced
//!   by the compiler rather than by a code path that a later edit could bypass.

use std::collections::HashMap;

use super::ability::{Ability, Confidence, confidence_of};
use super::residual::Residual;
use crate::settings::types::Mode;

// ---- what a dose looks like ------------------------------------------------
//
// These live here, next to `Dose`, because the ledger reads them too: it has to
// know what the coach *asked* in order to judge whether the athlete did it (see
// `residual::judge`). Two copies of these numbers would mean the coach asking
// one thing and the ledger marking another — and the athlete taking the blame
// for the difference.

/// Reps in reserve the working load targets at the top of the rep range. `0` =
/// prescribe to demonstrated capacity: a load whose top-of-range reps match your
/// estimated e1RM. Progression is then *earned* — the load only steps up when
/// logged sets raise the e1RM enough to cross the next owned weight — never a
/// blind +2.5 kg the reps don't support.
pub const TARGET_RIR: f64 = 0.0;
/// Extra reps-in-reserve when the coach is easing off — a low-readiness day, or
/// the miss-response holding/backing off. A lighter working load, fewer reps
/// asked at a given one.
pub const LOW_READINESS_EXTRA_RIR: f64 = 2.0;
/// Seconds added to a hold when progressing (bounded properly in a later stage).
pub const HOLD_STEP_S: i32 = 5;
/// A loaded carry's working duration, and the ceiling it climbs to before the
/// weight steps instead. Double progression, with seconds where the reps go: a
/// carry that has reached the ceiling is asking for more weight, not more walking.
pub const CARRY_BASE_S: i32 = 30;
pub const CARRY_TOP_S: i32 = 60;

/// Readiness score below this → hold progression (don't chase PRs on a bad day).
pub const READINESS_HOLD_BELOW: f64 = 0.40;

/// Was this a full-effort day, biometrically? `None` (health has no data, or the
/// day is too old to reconstruct) means "no reason to think otherwise" — the same
/// answer the engine gives when health is down, so a missing signal never invents
/// an easing that didn't happen.
pub fn readiness_advances(score: Option<f64>) -> bool {
    !matches!(score, Some(s) if s < READINESS_HOLD_BELOW)
}

/// The reserve the ask leaves. `advance` is "today is a full-effort day" — false
/// on a low-readiness day or while the miss-response is easing off.
pub fn reserve(advance: bool) -> f64 {
    if advance {
        TARGET_RIR
    } else {
        TARGET_RIR + LOW_READINESS_EXTRA_RIR
    }
}

/// Rep range for a mode + metric (holds are seconds, handled in `engine::prescribe`).
pub fn rep_range(mode: Mode, weighted: bool) -> RepTarget {
    let (low, high) = match mode {
        Mode::Strength => {
            if weighted {
                (3, 6)
            } else {
                (5, 8)
            }
        }
        Mode::Balanced => {
            if weighted {
                (6, 10)
            } else {
                (8, 12)
            }
        }
        Mode::Skills => (3, 6),
        Mode::Conditioning => {
            if weighted {
                (12, 20)
            } else {
                (15, 25)
            }
        }
    };
    RepTarget { low, high }
}

/// The discrete weights available for one exercise's kit at this location —
/// sorted ascending, deduped, and **never empty** (the only constructor rejects
/// that). Holding the invariant in the type is what makes [`Inventory::snap`]
/// total: no empty-inventory fallback, so no invented weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Inventory(Vec<f64>);

impl Inventory {
    /// The weights you own, or `None` if you own none — in which case the
    /// exercise is not loadable here and must not be prescribed.
    pub fn new(mut loads: Vec<f64>) -> Option<Self> {
        loads.sort_by(f64::total_cmp);
        loads.dedup();
        (!loads.is_empty()).then_some(Inventory(loads))
    }

    /// Snap a target load to the nearest weight owned here (ties → lighter).
    /// Total — an `Inventory` always has at least one weight.
    pub fn snap(&self, target: f64) -> f64 {
        self.0
            .iter()
            .copied()
            .min_by(|a, b| (a - target).abs().total_cmp(&(b - target).abs()))
            .expect("Inventory is non-empty by construction")
    }

    /// The lightest weight owned here — where a build-up starts when there's no
    /// estimate to start it from.
    pub fn lightest(&self) -> f64 {
        self.0[0]
    }

    /// The next weight up from `load`, or the heaviest owned when there is none —
    /// the rung a carry steps to once it has topped out its time. Total, like
    /// [`snap`](Self::snap): at the top of the rack there is nowhere further to go,
    /// and saying so is better than inventing a weight.
    pub fn next_above(&self, load: f64) -> f64 {
        self.0
            .iter()
            .copied()
            .find(|w| *w > load + 1e-9)
            .unwrap_or_else(|| *self.0.last().expect("Inventory is non-empty"))
    }

    /// The next weight *down* from `load` — the rung a lift backs off to after
    /// repeated misses. At the lightest weight owned there is nowhere further down,
    /// and the answer is that weight rather than a lighter one you don't have.
    pub fn next_below(&self, load: f64) -> f64 {
        self.0
            .iter()
            .copied()
            .rev()
            .find(|w| *w < load - 1e-9)
            .unwrap_or_else(|| self.lightest())
    }
}

/// A rep target: climb from `low` to `high` before the load is allowed to step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepTarget {
    pub low: i32,
    pub high: i32,
}

/// A prescription — what a trusted estimate says you can do today. One variant
/// per metric, so the fields that exist are exactly the fields that mean
/// something.
#[derive(Clone, Debug, PartialEq)]
pub enum Dose {
    /// A weighted lift *has* a load. Not `Option<f64>` — a weighted set with no
    /// weight isn't a lighter prescription, it's a nonsense one.
    Weighted {
        load: f64,
        reps: RepTarget,
    },
    Bodyweight {
        reps: RepTarget,
    },
    Hold {
        secs: i32,
    },
    /// A loaded carry: both, because a carry is both. Same reasoning as `Weighted`
    /// — a farmer's walk with no weight is not a light farmer's walk, and one with
    /// no duration is not a short one. Neither field is optional.
    WeightedHold {
        load: f64,
        secs: i32,
    },
}

/// A calibration set — what the engine asks for when it *doesn't* trust its
/// estimate. The logged result is the measurement; the next verdict prescribes
/// from it (G3). Never a guessed number dressed up as a prescription.
#[derive(Clone, Debug, PartialEq)]
pub enum Measure {
    /// Build up to a hard-but-clean set of `reps` and log load/reps/RPE. `start`
    /// is a safe opening weight — from a stale estimate when there is one, else
    /// the lightest weight owned here.
    BuildUp { start: f64, reps: i32 },
    /// As many clean reps as you have — stop at form breakdown.
    Amrap,
    /// One max hold.
    MaxHold,
    /// Carry `start` for as long as form holds, and log the weight *and* the time
    /// — both are the measurement. `start` is a safe opening weight, from a stale
    /// carry when there is one, else the lightest owned.
    LoadedCarry { start: f64 },
}

/// An ability estimate the engine **trusts enough to prescribe from**.
///
/// The only way to obtain one is [`Known::of`]. Prescription functions take a
/// `Known` by type, so it is not possible — today or after any future edit — to
/// derive a working load for an exercise the engine doesn't actually know. That is
/// the safety rule ("when unsure, measure") expressed as a type rather than as a
/// convention.
///
/// Trust has two halves, and an estimate needs both:
///
/// - **Recent enough** — `High`/`Medium` confidence. An estimate built from stale
///   sets, or from none, describes someone else.
/// - **Not repeatedly wrong** — the athlete has not missed it several sessions
///   running ([`Residual::wants_remeasure`]). An estimate the sets keep
///   contradicting is not a run of bad luck; it is a wrong number, and prescribing
///   from it grinds the athlete against a claim they have already disproved. So it
///   goes back to being *measured*.
#[derive(Clone, Copy, Debug)]
pub struct Known<'a>(&'a Ability);

impl<'a> Known<'a> {
    /// The trusted estimate for `exercise_id`, or `None` — in which case the caller
    /// must assess instead.
    pub fn of(
        abilities: &'a HashMap<i64, Ability>,
        residuals: &HashMap<i64, Residual>,
        exercise_id: i64,
    ) -> Option<Self> {
        if residuals
            .get(&exercise_id)
            .is_some_and(Residual::wants_remeasure)
        {
            return None;
        }
        match confidence_of(abilities, exercise_id) {
            Confidence::High | Confidence::Medium => abilities.get(&exercise_id).map(Known),
            Confidence::Low | Confidence::None => None,
        }
    }
}

impl std::ops::Deref for Known<'_> {
    type Target = Ability;
    fn deref(&self) -> &Ability {
        self.0
    }
}
