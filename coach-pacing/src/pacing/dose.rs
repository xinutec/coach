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

use crate::prelude::*;
use alloc::collections::BTreeMap;

use super::ability::{Ability, Confidence, confidence_of};
use super::residual::Residual;
use crate::domain::Mode;

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

/// Reps taken off the ask when the coach is easing — the rep-side twin of
/// [`reserve`], for an ask that climbs reps at a held rung rather than inverting
/// an e1RM at a target reserve. The same number said in the other unit: a rep left
/// in reserve is a rep not asked for.
pub fn eased_reps(advance: bool) -> i32 {
    (reserve(advance) - TARGET_RIR) as i32
}

/// The load whose top set of `reps` reps (leaving `rir` in reserve) matches an
/// estimated 1-rep-max of `e1rm` — inverse Epley.
pub fn load_for(e1rm: f64, reps: f64, rir: f64) -> f64 {
    e1rm / (1.0 + (reps + rir) / 30.0)
}

/// Reps within reach at `load` (leaving `rir` in reserve) given `e1rm` — Epley,
/// solved for reps. The dual of [`load_for`].
pub fn reps_at(e1rm: f64, load: f64, rir: f64) -> f64 {
    30.0 * (e1rm / load - 1.0) - rir
}

/// The weight the coach last sent the athlete to on a lift, and the reps
/// demonstrated there.
///
/// This is a fact about the **coach's** history, not the athlete's, which is why
/// it lives with the ledger that replays it rather than with the ability estimate.
/// Deriving the working weight from `e1rm` each session cannot progress at all:
/// top-of-range reps at load `L` produce exactly the e1RM that prescribes `L`, so
/// the load is a fixed point of its own prescription (R6-1 — confirmed in the real
/// back-test, where six consecutive sessions were all asked for `10 × 9 kg`).
/// Deriving it from the athlete's *latest* session escapes the fixed point but
/// hands the rung to the athlete: the coach then follows a bad patch — or a
/// lighter bell picked off the rack — straight down, and the miss ladder stops
/// escalating because every shortfall becomes next session's target.
///
/// So the rung moves only when the coach moves it: up when the reps top the range
/// and a probe is due, down when the ledger backs off. Everything else holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rung {
    pub load: f64,
    /// Best reps demonstrated *at this weight* since the coach moved to it.
    pub reps: i32,
}

/// The weighted ask: which weight, and how many reps of it.
///
/// One function, because both sides of the loop need the identical answer — the
/// coach to write the card, the ledger to judge what came back. Two copies would
/// have the coach asking one number and the ledger marking another, with the
/// athlete taking the blame for the difference; the constants above already live
/// here for exactly that reason, and the rule that reads them belongs with them.
pub fn weighted_ask(
    inv: &Inventory,
    e1rm: Option<f64>,
    rung: Option<Rung>,
    mode: Mode,
    feedback: &Residual,
    recovered: bool,
) -> (f64, i32) {
    let range = rep_range(mode, true);
    // A miss is not a day to add load on; low readiness says the same for its own
    // reason. `recovered` alone (without the miss-response) is what decides how
    // many reps come *off* the ask — easing twice for one event would double-count.
    let advance = recovered && !feedback.wants_hold();
    let probe = advance && feedback.probe_due();
    let back_off = feedback.wants_back_off();

    let Some(r) = rung else {
        // No rung yet — a fresh movement, or one whose only loaded set was the
        // calibration's hard single. Enter where the estimate says: the weight
        // owned nearest the one that puts the top of the range in reach.
        let Some(e) = e1rm else {
            return (inv.lightest(), range.low);
        };
        let reserve = reserve(advance);
        let load = inv.snap(load_for(e, range.high as f64, reserve));
        let low = (libm::floor(reps_at(e, load, reserve)) as i32).clamp(1, range.high);
        return (load, low);
    };

    if back_off {
        // Two misses running: the rung is too heavy, not the day. One off, and the
        // reps carry over — dropping the weight *is* the easing.
        let lower = inv.next_below(inv.snap(r.load));
        if lower < r.load - 1e-9 {
            return (lower, r.reps.clamp(1, range.high));
        }
        // Already on the lightest weight owned: there is nothing lighter to send
        // them to, so the reps have to come down instead. The range *floor* is a
        // style preference and does not apply here — a set the athlete has no way
        // to finish is not a style, and pinning the ask at the floor on the
        // lightest bell is how a genuine regression ends up re-asked forever.
        return (lower, (r.reps - 1).clamp(1, range.high));
    }
    if r.reps >= range.high && probe {
        // Topped the range here, and a probe is due — that is what "earned" means.
        // The next rung is a floor on the step, not the whole of it: it guarantees
        // movement (strictly heavier, so there is no fixed point to sit in), while
        // an estimate that can see further is still believed. A set logged with
        // reps in reserve demonstrates strength the rung ladder cannot read, and
        // stepping 2.5 kg at a time would take months to reach a weight the sets
        // already justify. Both candidates are weights the athlete owns.
        let stepped = inv.next_above(inv.snap(r.load));
        let load = match e1rm {
            Some(e) => stepped.max(inv.snap(load_for(e, range.high as f64, reserve(advance)))),
            None => stepped,
        };
        return (load, range.low);
    }
    // Holding the rung: climb the reps on a probe, consolidate at what has been
    // shown between them, and ask fewer when the morning says to.
    let aim = if probe { r.reps + 1 } else { r.reps };
    (
        inv.snap(r.load),
        (aim - eased_reps(recovered)).clamp(1, range.high),
    )
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
/// sorted ascending, deduped, and **never empty**. Non-emptiness is the shape of
/// the struct rather than a rule the constructor promises to have checked: the
/// first rung is held separately, so every query has a weight to return and none
/// of them can panic on an empty ladder.
#[derive(Clone, Debug, PartialEq)]
pub struct Inventory {
    /// The lightest weight owned — the ladder always has this one.
    lightest: f64,
    /// The rungs above it, ascending. Empty when only one weight is owned.
    heavier: Vec<f64>,
}

impl Inventory {
    /// The weights you own, or `None` if you own none — in which case the
    /// exercise is not loadable here and must not be prescribed.
    ///
    /// Non-positive and non-finite entries are dropped, not just empties
    /// rejected: a `0.0` (or negative/NaN/inf) weight would make [`reps_at`]
    /// divide by a non-positive load — `e1rm / 0.0 = +inf`, floored and cast to
    /// a saturated rep count — so "do max reps at 0 kg" reaches the athlete with
    /// no panic to flag it. Holding *positive-finite* in the type (not merely
    /// non-empty) is what keeps the dose math total.
    ///
    /// [`reps_at`]: crate::pacing::engine
    pub fn new(mut loads: Vec<f64>) -> Option<Self> {
        loads.retain(|w| w.is_finite() && *w > 0.0);
        loads.sort_by(f64::total_cmp);
        loads.dedup();
        let mut rungs = loads.into_iter();
        // No weight at all is the one honest failure: the exercise isn't loadable
        // here. Splitting the first rung off is what puts "non-empty" in the
        // shape of the type — every query below then has an answer to return,
        // with no `unwrap` standing in for a comment about the constructor.
        let lightest = rungs.next()?;
        Some(Inventory {
            lightest,
            heavier: rungs.collect(),
        })
    }

    /// Every weight owned here, ascending. Double-ended so a step *down* can walk
    /// back from the top without collecting.
    fn rungs(&self) -> impl DoubleEndedIterator<Item = f64> + '_ {
        core::iter::once(self.lightest).chain(self.heavier.iter().copied())
    }

    /// Snap a target load to the nearest weight owned here (ties → lighter).
    pub fn snap(&self, target: f64) -> f64 {
        // Folding from the lightest rung (rather than `min_by` over the whole
        // ladder) keeps the result a plain `f64`: the starting value *is* an
        // answer, so there is no empty case to unwrap. A strict `<` keeps the
        // earlier — lighter — rung when two are equidistant.
        self.rungs().fold(self.lightest, |best, w| {
            if (w - target).abs() < (best - target).abs() {
                w
            } else {
                best
            }
        })
    }

    /// The lightest weight owned here — where a build-up starts when there's no
    /// estimate to start it from.
    pub fn lightest(&self) -> f64 {
        self.lightest
    }

    /// The heaviest weight owned here. With no rung above the first, the lightest
    /// *is* the heaviest — not a fallback, the answer.
    pub fn heaviest(&self) -> f64 {
        self.heavier.last().copied().unwrap_or(self.lightest)
    }

    /// The next weight up from `load`, or the heaviest owned when there is none —
    /// the rung a carry steps to once it has topped out its time. At the top of
    /// the rack there is nowhere further to go, and saying so is better than
    /// inventing a weight.
    pub fn next_above(&self, load: f64) -> f64 {
        self.rungs()
            .find(|w| *w > load + 1e-9)
            .unwrap_or_else(|| self.heaviest())
    }

    /// The next weight *down* from `load` — the rung a lift backs off to after
    /// repeated misses. At the lightest weight owned there is nowhere further down,
    /// and the answer is that weight rather than a lighter one you don't have.
    pub fn next_below(&self, load: f64) -> f64 {
        self.rungs()
            .rfind(|w| *w < load - 1e-9)
            .unwrap_or(self.lightest)
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
        abilities: &'a BTreeMap<i64, Ability>,
        residuals: &BTreeMap<i64, Residual>,
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

impl core::ops::Deref for Known<'_> {
    type Target = Ability;
    fn deref(&self) -> &Ability {
        self.0
    }
}
