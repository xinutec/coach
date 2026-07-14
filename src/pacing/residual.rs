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

use std::collections::HashMap;

use chrono::NaiveDateTime;

use super::ability::{self, Ability};
use super::types::SetRec;

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
    /// Sessions, oldest first — the ledger itself.
    pub outcomes: Vec<Outcome>,
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
}

/// The ledger for every exercise in `history`.
///
/// Takes no `now`: every session is judged at *its own* moment, against what was
/// known *then*. The ledger is a fact about the past and does not change with the
/// clock — which is also what makes it cheap to recompute on every verdict.
pub fn residuals(history: &[SetRec]) -> HashMap<i64, Residual> {
    let mut by_ex: HashMap<i64, Vec<&SetRec>> = HashMap::new();
    for s in history {
        by_ex.entry(s.exercise_id).or_default().push(s);
    }
    by_ex
        .into_iter()
        .map(|(id, sets)| (id, ledger(&sets)))
        .collect()
}

fn ledger(sets: &[&SetRec]) -> Residual {
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

    let mut outcomes = Vec::new();
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
        // The day is judged on its best set — the estimate is a claim about what the
        // athlete *can* do, not about what the third set of a session looks like.
        let actual = ability::estimate(&today, *day);

        if let Some(o) = compare(&predicted, &actual) {
            outcomes.push(o);
        }
    }

    let consecutive_misses = outcomes
        .iter()
        .rev()
        .take_while(|o| **o == Outcome::Missed)
        .count() as i32;
    Residual {
        outcomes,
        consecutive_misses,
    }
}

/// Compare a session against the estimate that preceded it, on whichever metric they
/// share. `None` when they share none — the session says nothing about the estimate,
/// so it is not evidence either way (and must not be recorded as a miss, which would
/// have the engine back off from silence).
fn compare(predicted: &Ability, actual: &Ability) -> Option<Outcome> {
    // Weighted work: compare the estimated 1RM the session demonstrated.
    if let (Some(p), Some(a)) = (predicted.e1rm, actual.e1rm) {
        return Some(band(a, p));
    }
    if let (Some(p), Some(a)) = (predicted.best_reps, actual.best_reps) {
        return Some(band(a as f64, p as f64));
    }
    if let (Some(p), Some(a)) = (predicted.best_hold, actual.best_hold) {
        return Some(band(a as f64, p as f64));
    }
    // A carry is two numbers, so "worse" needs saying: less weight is a miss
    // whatever the clock says, and at the same weight it comes down to the clock.
    if let (Some(p), Some(a)) = (predicted.carry, actual.carry) {
        return Some(if a.load < p.load * (1.0 - MISS_MARGIN) {
            Outcome::Missed
        } else {
            band(a.secs as f64, p.secs as f64)
        });
    }
    None
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
