//! The prediction-error ledger: how well the estimate has been describing the athlete
//! lately. Every prescription is a prediction, and ability is a max, so on its own a
//! bad session pulls nothing down.
//!
//! The ledger is **recomputed from history**, keeping the engine stateless: for each
//! training day, what the estimate was *before* it (the same [`ability::estimate`] over
//! earlier sets) against what the day produced. One miss holds the load, two in a row
//! step down a rung, and persistent misses re-open the measurement.
//!
//! ⚠ It compares **sessions, not sets**: a day is judged on its best set, since a third
//! set's fatigue is not a miss.

use crate::num::{count, whole};
use crate::prelude::*;
use alloc::collections::BTreeMap;

use chrono::{Duration, NaiveDate, NaiveDateTime};

use super::ability::{self, Ability};
use super::dose::{
    self, CARRY_BASE_S, CARRY_TOP_S, HOLD_STEP_S, Inventory, Rung, readiness_advances, rep_range,
    reserve,
};
use super::types::{Readiness, SetRec};
use crate::domain::ExerciseId;
use crate::domain::Mode;

// ---- tunable heuristics ----------------------------------------------------

/// How far below the estimate a session must land to count as a miss. Weight snaps
/// to the nearest owned plate and reps are integers, so a small shortfall is
/// quantisation, not failure.
const MISS_MARGIN: f64 = 0.05;
/// How far above the estimate a session must land to count as a beat — the estimate
/// was too cautious. Same reasoning, mirrored.
const BEAT_MARGIN: f64 = 0.05;
/// Consecutive misses before the load steps down instead of holding.
pub const BACK_OFF_AFTER: i32 = 2;
/// Quiet sessions (nothing beaten) between attempts at more. A probe (best+1) is earned
/// by a beat or by this much consolidation; without the cadence a failing +1 would be
/// re-asked every session (R4-1).
pub const PROBE_EVERY: i32 = 3;
/// How far back a plateau looks, and the least evidence it needs. A month of
/// sessions with nothing beaten is a movement that has stopped producing
/// progress — the trigger for the variation ladder (G7). Fewer sessions than
/// the minimum is thin data, not a verdict.
const PLATEAU_WINDOW_DAYS: i64 = 28;
const PLATEAU_MIN_SESSIONS: usize = 4;
/// Consecutive misses before the engine stops prescribing and measures again. A
/// wrong estimate is not a run of bad luck, and grinding an athlete against it is
/// how you dig a hole.
pub const REMEASURE_AFTER: i32 = 3;
/// Below this share of the asked work a session is a [`Outcome::Rout`], not an ordinary
/// miss. Measured as volume, which discriminates: one rep of ten is a 22 % shortfall in
/// implied 1RM but 90 % in work done. A third sits between one rep of ten (a rout) and
/// 40 kg × 5 → 30 kg × 5 against a ten-rep ask (an ordinary miss the back-off ladder
/// should walk).
const ROUT_FRACTION: f64 = 0.3;

/// How the athlete's session compared with what the engine believed beforehand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Beat the estimate — it was too cautious.
    Beat,
    /// Landed where the estimate said, within the quantisation margin.
    Met,
    /// Came in under the estimate.
    Missed,
    /// Came in *far* under it — less than `ROUT_FRACTION` of the work asked for.
    /// A miss with a magnitude, and a different kind of evidence: missing ten reps
    /// by one is a bad day, managing one of them is a wrong number, and the athlete
    /// has already supplied the correction. Escalates on its own rather than
    /// waiting for [`REMEASURE_AFTER`] sessions of it.
    Rout,
}

/// The recent prediction error for one exercise.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Residual {
    /// Sessions, oldest first, each judged at its own date — the ledger itself.
    /// Dated so plateau detection can ask "how long since anything was beaten?"
    /// in weeks rather than in sessions of unknown spacing.
    pub outcomes: Vec<(NaiveDate, Outcome)>,
    /// Misses at the end of the ledger. This is what the engine acts on: a miss
    /// answered by the next session's success is history, not a trend.
    pub consecutive_misses: i32,
    /// The weight the coach is currently working this lift at, and the reps shown
    /// there — carried forward across the walk, because it is a fact about what
    /// the coach *asked*, not about what the athlete can do. `None` for a movement
    /// that carries no load, or one with no loaded session yet. See [`Rung`].
    pub rung: Option<Rung>,
}

impl Residual {
    /// The estimate has been wrong often enough, or badly enough once, to be
    /// re-measured rather than prescribed from: a count alone would treat a rout like a
    /// rep short, three sessions running.
    pub fn wants_remeasure(&self) -> bool {
        self.consecutive_misses >= REMEASURE_AFTER
            || matches!(self.outcomes.last(), Some((_, Outcome::Rout)))
    }
    /// Back off a rung: two misses in a row is the estimate being too heavy, not a
    /// bad night's sleep.
    pub fn wants_back_off(&self) -> bool {
        self.consecutive_misses >= BACK_OFF_AFTER
    }
    /// Any miss at all → don't add load or reps on top of it.
    pub fn wants_hold(&self) -> bool {
        self.consecutive_misses > 0
    }
    /// Sessions since the athlete last beat the estimate — every one of them, when
    /// nothing was ever beaten. Zero for a movement with no ledger yet: a fresh
    /// movement progresses eagerly, there is nothing to consolidate.
    pub fn sessions_since_beat(&self) -> i32 {
        count(
            self.outcomes
                .iter()
                .rev()
                .take_while(|(_, o)| *o != Outcome::Beat)
                .count(),
        )
    }
    /// Is today a day to ask for more? Immediately after a beat (an earned climb
    /// keeps climbing), and periodically after enough quiet sessions; the sessions
    /// in between consolidate at the demonstrated best.
    pub fn probe_due(&self) -> bool {
        let n = self.sessions_since_beat();
        n == 0 || n % PROBE_EVERY == 0
    }
    /// A month of real sessions with nothing beaten: the movement has stopped
    /// producing progress. Not a slump — misses are the back-off's business, and
    /// stepping *up* the ladder mid-slump would answer weakness with more.
    pub fn plateaued(&self, now: NaiveDateTime) -> bool {
        if self.consecutive_misses > 0 {
            return false;
        }
        let cut = now.date() - Duration::days(PLATEAU_WINDOW_DAYS);
        let recent: Vec<_> = self.outcomes.iter().filter(|(d, _)| *d >= cut).collect();
        recent.len() >= PLATEAU_MIN_SESSIONS && recent.iter().all(|(_, o)| *o != Outcome::Beat)
    }
}

/// The ledger for every exercise in `history`. It takes no `now`: each session is
/// judged against what was known then, so it is cheap to recompute. `loads` is the rack
/// the engine plans against, needed to name the rung an ask was set on; an exercise
/// absent from it never grows a [`Rung`].
pub fn residuals(
    history: &[SetRec],
    mode: Mode,
    readiness: &BTreeMap<NaiveDate, Readiness>,
    loads: &BTreeMap<ExerciseId, Vec<f64>>,
) -> BTreeMap<ExerciseId, Residual> {
    let mut by_ex: BTreeMap<ExerciseId, Vec<&SetRec>> = BTreeMap::new();
    for s in history {
        by_ex.entry(s.exercise_id).or_default().push(s);
    }
    by_ex
        .into_iter()
        .map(|(id, sets)| {
            let inv = loads.get(&id).cloned().and_then(Inventory::new);
            (id, ledger(&sets, mode, readiness, inv.as_ref()))
        })
        .collect()
}

fn ledger(
    sets: &[&SetRec],
    mode: Mode,
    readiness: &BTreeMap<NaiveDate, Readiness>,
    inv: Option<&Inventory>,
) -> Residual {
    // Sessions, oldest first. A session is a distinct local day — the same unit
    // confidence counts in.
    let mut days: Vec<NaiveDateTime> = sets.iter().map(|s| s.logged_at).collect();
    days.sort();
    let mut sessions: Vec<NaiveDateTime> = Vec::new();
    for d in days {
        if sessions.last().map(|l| l.date()) != Some(d.date()) {
            sessions.push(d);
        }
    }

    // Walked forward, because each session is judged against what the engine
    // believed *and asked* that morning — and the ask depends on the ledger up to
    // that point (a hold, a back-off, a probe). `led` therefore is, at every step,
    // exactly the feedback the engine held when it wrote that day's prescription.
    let mut led = Residual::default();
    for day in &sessions {
        // What the engine knew that morning: strictly-earlier sets, estimated at the
        // moment the session began. The very first session has nothing to predict
        // from — it *was* the measurement — so it produces no outcome.
        let prior: Vec<&SetRec> = sets
            .iter()
            .copied()
            .filter(|s| s.logged_at.date() < day.date())
            .collect();
        if prior.is_empty() {
            continue;
        }
        let predicted = ability::estimate(&prior, *day);

        let today: Vec<&SetRec> = sets
            .iter()
            .copied()
            .filter(|s| s.logged_at.date() == day.date())
            .collect();

        // What health knew about that morning — absent means no reason to think the
        // day was anything but full-effort.
        let recovered = readiness_advances(readiness.get(&day.date()).map(|r| r.score()));

        // The weighted ask that morning, from the *same* function that wrote the
        // card. Computed before judging, because it is what the session is judged
        // against; and kept afterwards, because it is what the next morning's ask
        // climbs from.
        let asked =
            inv.map(|i| dose::weighted_ask(i, predicted.e1rm, led.rung, mode, &led, recovered));

        if let Some(o) = judge(&predicted, &today, &led, mode, recovered, asked) {
            led.consecutive_misses = if matches!(o, Outcome::Missed | Outcome::Rout) {
                led.consecutive_misses + 1
            } else {
                0
            };
            led.outcomes.push((day.date(), o));
        }

        if let Some(ask) = asked {
            led.rung = advance_rung(ask, &today, mode);
        }
    }
    led
}

/// Where the coach stands on this lift after the session just judged: the rung is the
/// **ask itself**, and the athlete moves it only by doing more at that weight. It never
/// follows a short session down, or every shortfall would become the next target and
/// the miss ladder could never escalate (R6-1).
fn advance_rung((ask_load, ask_reps): (f64, i32), today: &[&SetRec], mode: Mode) -> Option<Rung> {
    let range = rep_range(mode, true);
    // What the athlete did *at the weight they were sent to*. Work at some other
    // weight says nothing about this rung — a bell picked off the rack because the
    // right one was in use must not drag the coach off it.
    let done = today
        .iter()
        .filter(|s| s.load_kg.is_some_and(|l| (l - ask_load).abs() < 1e-9))
        .filter_map(|s| s.reps)
        .max();
    let reps = done.unwrap_or(ask_reps).max(ask_reps).clamp(1, range.high);
    Some(Rung {
        load: ask_load,
        reps,
    })
}

/// How the session compared with **what the engine asked that morning**, not with the
/// athlete's ceiling. The engine deliberately asks for less while holding, backing off
/// or on an under-recovered day; judged against the ceiling, compliance would read as
/// failure and the back-off would feed itself.
///
/// The ask is recomputed from the same numbers `prescribe` used, at the load actually
/// logged, so an improvised weight is judged honestly. `recovered` comes from what
/// health knew that morning ([`PacingInput::readiness_history`]); a day it can't answer
/// for is full-effort. `None` when the session says nothing about the ask: silence is
/// not a miss.
fn judge(
    predicted: &Ability,
    today: &[&SetRec],
    feedback: &Residual,
    mode: Mode,
    recovered: bool,
    asked_weighted: Option<(f64, i32)>,
) -> Option<Outcome> {
    // Exactly the reconstruction `prescribe` performs from the same inputs.
    let advance = recovered && !feedback.wants_hold();
    let probe = advance && feedback.probe_due();
    let back_off = feedback.wants_back_off();
    let rir = reserve(advance);

    // A carry is judged first, and as a carry: it carries both a load and a hold,
    // so a plain hold comparison below would silently claim it and judge a walk
    // by its clock alone.
    if let Some(c) = predicted.carry {
        // Take the two fields a carry is judged on up front, so the comparison
        // works on values instead of re-asserting the filter's promise at every
        // use. A set missing either simply isn't a carry.
        let best = today
            .iter()
            .filter_map(|s| Some((s.load_kg?, s.hold_s?)))
            // total_cmp, not partial_cmp: a NaN load would make the tuple
            // comparison return None and panic the whole pacing pass mid-sort.
            .max_by(|(a_load, a_secs), (b_load, b_secs)| {
                a_load.total_cmp(b_load).then(a_secs.cmp(b_secs))
            });
        if let Some((load, done)) = best {
            // The weight is the coach's choice, so only the clock is the athlete's
            // to miss. A stepped weight (either way) restarts the clock; otherwise
            // the clock climbs on a probe and holds between them.
            let stepped = (load - c.load).abs() > 1e-9;
            let asked = if stepped {
                CARRY_BASE_S
            } else if probe {
                (c.secs + HOLD_STEP_S).min(CARRY_TOP_S)
            } else {
                c.secs
            };
            // Volume for a carry is weight × time; the weight is the coach's
            // choice, so a shortfall lives entirely in the clock.
            return Some(sized(
                band(f64::from(done), f64::from(asked)),
                load * f64::from(done),
                load * f64::from(asked),
            ));
        }
    }

    // Weighted work, judged against the ask the coach actually wrote — reps at a
    // rung, handed in from the same `dose::weighted_ask` that wrote it.
    if let Some((ask_load, ask_reps)) = asked_weighted {
        let best = today
            .iter()
            .filter_map(|s| Some((s.load_kg?, s.reps?)))
            .max_by(|(a_load, a_reps), (b_load, b_reps)| {
                face(*a_load, *a_reps).total_cmp(&face(*b_load, *b_reps))
            });
        if let Some((load, done)) = best {
            // Compared as work, not as a rep count. The ask names a weight, so
            // "same reps, lighter bell" is not compliance — counting reps alone
            // would let the athlete walk the coach down the rack — and "fewer reps,
            // heavier bell" is not a failure. Epley is the one unit both are
            // expressible in, and `band`'s margin absorbs the rounding.
            return Some(sized(
                band(face(load, done), face(ask_load, ask_reps)),
                load * f64::from(done),
                ask_load * f64::from(ask_reps),
            ));
        }
    } else if let Some(e) = predicted.e1rm {
        // No rack registered here, so no rung to have been sent to: fall back to
        // what the estimate supported at the load actually used.
        let best = today
            .iter()
            .filter_map(|s| Some((s.load_kg?, s.reps?)))
            .max_by(|(a_load, a_reps), (b_load, b_reps)| {
                face(*a_load, *a_reps).total_cmp(&face(*b_load, *b_reps))
            });
        if let Some((load, done)) = best {
            let raw = 30.0 * (e / load - 1.0) - rir;
            let aim = if probe {
                libm::round(raw)
            } else {
                libm::floor(raw)
            };
            let asked = whole(aim).clamp(1, rep_range(mode, true).high);
            return Some(sized(
                reps_band(done, asked),
                load * f64::from(done),
                load * f64::from(asked),
            ));
        }
    }

    // Bodyweight reps: the reserve doesn't apply (there is no load to lighten), so
    // the ask is the demonstrated best, plus one on a probe and minus one on a
    // back-off — the same three cases `prescribe` has.
    if let Some(best) = predicted.best_reps {
        let done = today
            .iter()
            .filter(|s| s.load_kg.is_none())
            .filter_map(|s| s.reps)
            .max();
        if let Some(done) = done {
            let aim = match (probe, back_off) {
                (_, true) => best - 1,
                (true, false) => best + 1,
                (false, false) => best,
            };
            let asked = aim.clamp(1, rep_range(mode, false).high);
            // Reps *are* the volume here — there is no load to weight them by.
            return Some(sized(
                reps_band(done, asked),
                f64::from(done),
                f64::from(asked),
            ));
        }
    }

    if let Some(base) = predicted.best_hold {
        let done = today
            .iter()
            .filter(|s| s.load_kg.is_none())
            .filter_map(|s| s.hold_s)
            .max();
        if let Some(done) = done {
            let secs = match (probe, back_off) {
                (_, true) => base - HOLD_STEP_S,
                (true, false) => base + HOLD_STEP_S,
                (false, false) => base,
            };
            let secs = secs.max(HOLD_STEP_S);
            return Some(sized(
                band(f64::from(done), f64::from(secs)),
                f64::from(done),
                f64::from(secs),
            ));
        }
    }
    None
}

/// A weighted set's face-value e1RM — for picking the day's best set. The session
/// is judged on its best: the estimate is a claim about what the athlete *can* do,
/// not about what the third set of a session looks like.
fn face(load: f64, reps: i32) -> f64 {
    load * (1.0 + f64::from(reps) / 30.0)
}

/// Reps are the unit the ask is written in, and they're integers — so compliance is
/// exact, with no quantisation margin to forgive.
fn reps_band(done: i32, asked: i32) -> Outcome {
    match done.cmp(&asked) {
        core::cmp::Ordering::Less => Outcome::Missed,
        core::cmp::Ordering::Equal => Outcome::Met,
        core::cmp::Ordering::Greater => Outcome::Beat,
    }
}

/// An outcome, sized by the shortfall. `done` and `asked` are volumes in the metric's
/// units (load × reps, seconds, load × seconds), not Epley figures, which make a rout
/// look survivable.
fn sized(outcome: Outcome, done: f64, asked: f64) -> Outcome {
    match outcome {
        Outcome::Missed if asked > 0.0 && done < asked * ROUT_FRACTION => Outcome::Rout,
        o => o,
    }
}

fn band(actual: f64, predicted: f64) -> Outcome {
    if actual < predicted * (1.0 - MISS_MARGIN) {
        Outcome::Missed
    } else if actual > predicted * (1.0 + BEAT_MARGIN) {
        Outcome::Beat
    } else {
        Outcome::Met
    }
}
