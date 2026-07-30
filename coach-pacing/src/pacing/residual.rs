//! The prediction-error ledger: how well the engine's estimate has been describing
//! the athlete lately.
//!
//! Every prescription is a **prediction** — "you can do 8 × 40 kg". Until now the
//! engine never checked. Ability is a *max* over decayed sets, so a session that
//! went badly pulled nothing down: a bad day was ignored rather than answered, and
//! the athlete kept being handed a number the sets had already contradicted.
//!
//! Nothing is stored to fix that. The residual is **recomputable from history
//! alone**, which keeps the engine stateless: for each training day, ask what the
//! ability estimate was *before* it (the same [`ability::estimate`] the engine would
//! have used that morning, over the strictly-earlier sets), and compare it against
//! what the day actually produced.
//!
//! Two things follow, and they are the point of the ledger:
//!
//! - **A miss is answered.** One → hold the load rather than bump it. Two in a row →
//!   step *down* the owned-weights ladder and rebuild.
//! - **Persistent misses re-open the measurement.** If the estimate keeps being
//!   wrong, it is not a bad day, it is a wrong estimate — so the exercise goes back
//!   to being *measured* rather than prescribed. That is the same rule as everywhere
//!   else in this engine: when it doesn't know, it measures.
//!
//! It compares **sessions, not sets**. The third set of a session is expected to be
//! worse than the first — that's fatigue, not a miss — so a day is judged on its best
//! set, which is what the estimate is a claim about.

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
/// Quiet sessions (nothing beaten) between attempts at more. Asking best+1 is a
/// **probe**, and a probe is earned: by a session that actually beat the
/// estimate, or periodically after this much consolidation. Without the cadence
/// the coach re-asked the same failing +1 every session — the estimate never
/// moves when the athlete matches their best while failing the ask (ability is
/// a max), so nothing ever answered it. (R4-1, from the athlete simulation.)
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
/// How little of the asked work makes a session a [`Outcome::Rout`] rather than an
/// ordinary miss — measured as *volume* (load × reps, seconds, reps), which is
/// linear and so actually discriminates. An Epley ratio does not: one rep at a
/// weight versus ten of it is a 22 % difference in implied 1RM but a 90 % one in
/// what the athlete managed, and it is the second number a coach reacts to.
///
/// A third, and the number has two real constraints either side of it. Below: the
/// case this exists for is a novice handed someone else's history — asked for ten
/// reps of a weight and managing one, about a tenth of the work. Above: dropping
/// from 40 kg × 5 to 30 kg × 5 against a ten-rep ask is a little over *four* tenths,
/// and that has to stay an ordinary miss, because it is exactly the shape the
/// hold → back-off → re-measure ladder was built to walk. A third sits between them
/// with room on both sides, which is as precise as this wants to be.
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
    /// Came in *far* under it — less than [`ROUT_FRACTION`] of the work asked for.
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
    /// The estimate has been wrong often enough — or wrong *badly* enough once —
    /// that it should be re-measured rather than prescribed from.
    ///
    /// The count alone was blind to magnitude: a session at a tenth of the ask and
    /// a session one rep short were the same event, so a new user carrying someone
    /// else's history (or an athlete who has genuinely lost strength) was asked for
    /// a weight they could lift *once*, three sessions running, before anything
    /// re-opened the question. A rout is its own evidence.
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
        self.outcomes
            .iter()
            .rev()
            .take_while(|(_, o)| *o != Outcome::Beat)
            .count() as i32
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

/// The ledger for every exercise in `history`.
///
/// Takes no `now`: every session is judged at *its own* moment, against what was
/// known *then*. The ledger is a fact about the past and does not change with the
/// clock — which is also what makes it cheap to recompute on every verdict.
/// `loads` is the weights each exercise can be built with here — the same map the
/// engine plans against. The ledger needs it because the ask it reconstructs is a
/// weight off the rack ([`dose::weighted_ask`]), and a rung it cannot name is a
/// rung it cannot judge against. An exercise absent from the map simply never
/// grows a [`Rung`].
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
        let recovered = readiness_advances(readiness.get(&day.date()).map(|r| r.score));

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

/// Where the coach stands on this lift after the session it just judged.
///
/// The rung *moved* (or didn't) inside [`dose::weighted_ask`] when the ask was
/// written; this only records where that left things. The standing position is
/// the **ask itself** — that is what "the weight the coach sent you to" means —
/// and the athlete can push it further only by doing more at that weight.
///
/// The baseline deliberately does not fall when a session comes in short. Letting
/// it follow the athlete down is what sank the first attempt at R6-1: every
/// shortfall silently became the next target, so a decline registered one miss and
/// read as compliance ever after, and `two misses → back off` /
/// `three → re-measure` became unreachable. Holding it means a short session is
/// re-asked once (the hold), and a second one steps the rung down — the ladder,
/// working as designed.
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

/// How the session compared with **what the engine asked that morning** — not with
/// the athlete's ceiling.
///
/// That distinction is the whole point of this function. The engine does not always
/// ask for everything the estimate supports: whenever the miss-response is holding
/// or backing off, it deliberately asks for *less* ([`dose::reserve`]). Judging
/// those sessions against the ceiling scored full compliance as failure — and the
/// back-off was the worst case, because it fed itself: two real misses eased the
/// ask, the eased session then read as miss number three, and a perfectly good
/// estimate was sent back to calibration. "Back off and rebuild" could never
/// rebuild. So the ask is reconstructed here from the same numbers `prescribe`
/// used, and the question the ledger answers is "did you do what I asked?".
///
/// The rack never has to be reconstructed: the athlete's set records the load they
/// actually used, so the ask is recomputed *at that load*. Which also means an
/// improvised weight is judged honestly rather than as a miss.
///
/// `None` when the session says nothing about the ask (no shared metric) — it is
/// not evidence either way, and must not be recorded as a miss, which would have
/// the engine back off from silence.
///
/// `recovered` is the other half of the ask: a low-readiness morning eases it too,
/// and that fact lives in health-sync rather than in the set history, so it is
/// reconstructed by asking health what it knew that day
/// ([`PacingInput::readiness_history`]). A day health can't answer for is judged
/// full-effort — a missing signal must never invent an easing that didn't happen.
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
                band(done as f64, asked as f64),
                load * done as f64,
                load * asked as f64,
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
                load * done as f64,
                ask_load * ask_reps as f64,
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
            let asked = (aim as i32).clamp(1, rep_range(mode, true).high);
            return Some(sized(
                reps_band(done, asked),
                load * done as f64,
                load * asked as f64,
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
            return Some(sized(reps_band(done, asked), done as f64, asked as f64));
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
                band(done as f64, secs as f64),
                done as f64,
                secs as f64,
            ));
        }
    }
    None
}

/// A weighted set's face-value e1RM — for picking the day's best set. The session
/// is judged on its best: the estimate is a claim about what the athlete *can* do,
/// not about what the third set of a session looks like.
fn face(load: f64, reps: i32) -> f64 {
    load * (1.0 + reps as f64 / 30.0)
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

/// An outcome, with the size of the shortfall taken into account.
///
/// `done` and `asked` are **volumes** in the metric's own units — load × reps for a
/// lift, seconds for a hold, load × seconds for a carry — not the Epley figures the
/// bands are computed from. That difference is the point: Epley compresses a rout
/// into something that looks survivable (one rep of ten reads as a 22 % shortfall),
/// and volume doesn't (it reads as 90 %).
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
