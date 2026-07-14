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
/// The only way to obtain one is [`Known::of`], which refuses `Low`/`None`
/// confidence. Prescription functions take a `Known` by type, so it is not
/// possible — today or after any future edit — to derive a working load for an
/// exercise the athlete hasn't recently demonstrated. That is the G3 safety rule
/// ("when unsure, measure") expressed as a type rather than as a convention.
#[derive(Clone, Copy, Debug)]
pub struct Known<'a>(&'a Ability);

impl<'a> Known<'a> {
    /// The trusted estimate for `exercise_id`, or `None` when confidence is
    /// `Low`/`None` — in which case the caller must assess instead.
    pub fn of(abilities: &'a HashMap<i64, Ability>, exercise_id: i64) -> Option<Self> {
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
