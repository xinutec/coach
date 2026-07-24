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
use super::dose::{CARRY_BASE_S, CARRY_TOP_S, HOLD_STEP_S, readiness_advances, rep_range, reserve};
use super::types::{Readiness, SetRec};
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

/// How the athlete's session compared with what the engine believed beforehand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Beat the estimate — it was too cautious.
    Beat,
    /// Landed where the estimate said, within the quantisation margin.
    Met,
    /// Came in under the estimate.
    Missed,
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
}

impl Residual {
    /// The estimate has been wrong often enough that it should be re-measured rather
    /// than prescribed from.
    pub fn wants_remeasure(&self) -> bool {
        self.consecutive_misses >= REMEASURE_AFTER
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
pub fn residuals(
    history: &[SetRec],
    mode: Mode,
    readiness: &BTreeMap<NaiveDate, Readiness>,
) -> BTreeMap<i64, Residual> {
    let mut by_ex: BTreeMap<i64, Vec<&SetRec>> = BTreeMap::new();
    for s in history {
        by_ex.entry(s.exercise_id).or_default().push(s);
    }
    by_ex
        .into_iter()
        .map(|(id, sets)| (id, ledger(&sets, mode, readiness)))
        .collect()
}

fn ledger(sets: &[&SetRec], mode: Mode, readiness: &BTreeMap<NaiveDate, Readiness>) -> Residual {
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
        if let Some(o) = judge(&predicted, &today, &led, mode, recovered) {
            led.consecutive_misses = if o == Outcome::Missed {
                led.consecutive_misses + 1
            } else {
                0
            };
            led.outcomes.push((day.date(), o));
        }
    }
    led
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
        let best = today
            .iter()
            .filter(|s| s.load_kg.is_some() && s.hold_s.is_some())
            // total_cmp, not partial_cmp: a NaN load would make the tuple
            // comparison return None and panic the whole pacing pass mid-sort.
            .max_by(|a, b| {
                a.load_kg
                    .unwrap()
                    .total_cmp(&b.load_kg.unwrap())
                    .then(a.hold_s.unwrap().cmp(&b.hold_s.unwrap()))
            });
        if let Some(bs) = best {
            let (load, done) = (bs.load_kg.unwrap(), bs.hold_s.unwrap());
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
            return Some(band(done as f64, asked as f64));
        }
    }

    // Weighted work: how many reps did the estimate support at the load actually
    // used, leaving the reserve the coach asked for?
    if let Some(e) = predicted.e1rm {
        let best = today
            .iter()
            .filter(|s| s.load_kg.is_some() && s.reps.is_some())
            .max_by(|a, b| face(a).total_cmp(&face(b)));
        if let Some(bs) = best {
            let (load, done) = (bs.load_kg.unwrap(), bs.reps.unwrap());
            let raw = 30.0 * (e / load - 1.0) - rir;
            let aim = if probe {
                libm::round(raw)
            } else {
                libm::floor(raw)
            };
            let asked = (aim as i32).clamp(1, rep_range(mode, true).high);
            return Some(reps_band(done, asked));
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
            return Some(reps_band(done, asked));
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
            return Some(band(done as f64, secs.max(HOLD_STEP_S) as f64));
        }
    }
    None
}

/// A weighted set's face-value e1RM — for picking the day's best set. The session
/// is judged on its best: the estimate is a claim about what the athlete *can* do,
/// not about what the third set of a session looks like.
fn face(s: &SetRec) -> f64 {
    s.load_kg.unwrap_or(0.0) * (1.0 + s.reps.unwrap_or(0) as f64 / 30.0)
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

fn band(actual: f64, predicted: f64) -> Outcome {
    if actual < predicted * (1.0 - MISS_MARGIN) {
        Outcome::Missed
    } else if actual > predicted * (1.0 + BEAT_MARGIN) {
        Outcome::Beat
    } else {
        Outcome::Met
    }
}
