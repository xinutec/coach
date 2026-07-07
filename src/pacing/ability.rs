//! The ability model: a pure estimate of what the athlete can do *today* per
//! exercise, derived from logged set history. This is the foundation the
//! prescription derives from (see `engine`) — replacing "bump the last set",
//! which is blind to how old that set is and how hard it went.
//!
//! Every number is derivable from history by a pure function; no clock is read
//! (the caller passes `now`), so it's fully unit-testable and back-testable.
//!
//! Two ideas do the work:
//!   * **RPE-aware e1RM** — a set of `reps` at `load` with `rir` reps in reserve
//!     is worth an estimated 1-rep-max of `load × (1 + (reps + rir)/30)` (Epley,
//!     extended for reserve). Missing RPE → `rir = 0` (the set at face value).
//!   * **Per-set staleness decay** — each set's estimate is scaled down by *its
//!     own* age (full trust for two weeks, then the detraining slope to a
//!     floor), and the exercise's ability is the **max of these decayed
//!     estimates**. Decaying per set, then maxing, makes ability provably
//!     monotone under idleness (more time off never *raises* it) while still
//!     trusting a genuine old PR down to the floor rather than forgetting it.
//!
//! Confidence is separate from the estimate: it counts *recent* sessions, and
//! (in later stages) decides whether the engine prescribes from the estimate or
//! asks for a fresh assessment.

use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDateTime};

use super::types::SetRec;

// ---- tunable heuristics ----------------------------------------------------

/// An exercise idle longer than this (days) starts losing trusted ability.
const DECAY_GRACE_DAYS: f64 = 14.0;
/// Ability lost per week of idleness past the grace period — the detraining
/// slope. Strength holds for a couple of weeks, then erodes gradually.
const DECAY_PER_WEEK: f64 = 0.015;
/// Ability never decays below this fraction of its raw value: strength doesn't
/// vanish over a layoff, it regresses to a floor you re-reach quickly.
const DECAY_FLOOR: f64 = 0.60;
/// A set left of this window no longer counts toward *confidence* (it still
/// contributes a decayed estimate — see the module note).
const CONFIDENCE_WEEKS: i64 = 6;
/// Recent sessions (distinct days) needed for `High` / `Medium` confidence.
const HIGH_SESSIONS: i32 = 3;
const MEDIUM_SESSIONS: i32 = 1;

/// How much the engine trusts an exercise's estimate — the gate between
/// prescribing (from the estimate) and assessing (measuring afresh, G3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    /// ≥ `HIGH_SESSIONS` recent sessions — prescribe with a full progression.
    High,
    /// 1–2 recent sessions — prescribe, but conservatively.
    Medium,
    /// Only stale data (no recent sessions) — an estimate exists but is old.
    Low,
    /// Never done — no estimate at all.
    None,
}

/// What the athlete can do on an exercise today, estimated from history.
/// The `Option`s are `None` for a metric the logged sets never carried (a
/// bodyweight move has no `e1rm`; a barbell lift no `best_reps`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ability {
    /// Decayed, RPE-aware estimated 1-rep-max (kg) — weighted work.
    pub e1rm: Option<f64>,
    /// Decayed best effective reps (reps + reserve) — bodyweight rep work.
    pub best_reps: Option<i32>,
    /// Decayed best hold (seconds) — isometric work.
    pub best_hold: Option<i32>,
    pub confidence: Confidence,
    /// Distinct recent days the exercise was trained (drives confidence).
    pub sessions_recent: i32,
}

/// Reps left in reserve implied by an RPE (rir = 10 − rpe, floored at 0). A
/// missing RPE is taken at face value (0 reserve).
fn rir(rpe: Option<i32>) -> f64 {
    rpe.map(|r| (10 - r).max(0) as f64).unwrap_or(0.0)
}

/// Epley 1RM extended for reps-in-reserve: what the set implies you could lift
/// once. `reps + rir` is the effective rep count taken to failure.
fn epley(load: f64, reps: i32, rpe: Option<i32>) -> f64 {
    load * (1.0 + (reps as f64 + rir(rpe)) / 30.0)
}

/// Staleness multiplier for a set `age_days` old: 1.0 within the grace window,
/// then the detraining slope down to `DECAY_FLOOR`.
fn decay(age_days: f64) -> f64 {
    let weeks_past = ((age_days - DECAY_GRACE_DAYS) / 7.0).max(0.0);
    (1.0 - DECAY_PER_WEEK * weeks_past).max(DECAY_FLOOR)
}

fn max_opt(cur: Option<f64>, v: f64) -> Option<f64> {
    Some(cur.map_or(v, |m| m.max(v)))
}

/// Estimate ability for every exercise present in `history`. Exercises absent
/// from the returned map have never been trained → treat as `Confidence::None`.
pub fn abilities(history: &[SetRec], now: NaiveDateTime) -> HashMap<i64, Ability> {
    let mut by_ex: HashMap<i64, Vec<&SetRec>> = HashMap::new();
    for s in history {
        by_ex.entry(s.exercise_id).or_default().push(s);
    }
    let window_cut = now - Duration::weeks(CONFIDENCE_WEEKS);

    by_ex
        .into_iter()
        .map(|(id, sets)| {
            let mut e1rm = None;
            let mut best_reps = None;
            let mut best_hold = None;
            let mut recent_days: HashSet<_> = HashSet::new();

            for s in &sets {
                let age = (now - s.logged_at).num_seconds().max(0) as f64 / 86_400.0;
                let d = decay(age);
                match (s.load_kg, s.reps, s.hold_s) {
                    // Weighted: load + reps → an e1RM estimate.
                    (Some(load), Some(reps), _) => {
                        e1rm = max_opt(e1rm, epley(load, reps, s.rpe) * d);
                    }
                    // Bodyweight reps: reps, no load → effective-rep estimate.
                    (None, Some(reps), _) => {
                        best_reps = max_opt(best_reps, (reps as f64 + rir(s.rpe)) * d);
                    }
                    _ => {}
                }
                // A hold set (isometric) carries hold_s regardless of the above.
                if let Some(h) = s.hold_s {
                    best_hold = max_opt(best_hold, h as f64 * d);
                }
                if s.logged_at >= window_cut {
                    recent_days.insert(s.logged_at.date());
                }
            }

            let sessions_recent = recent_days.len() as i32;
            let confidence = if sessions_recent >= HIGH_SESSIONS {
                Confidence::High
            } else if sessions_recent >= MEDIUM_SESSIONS {
                Confidence::Medium
            } else {
                Confidence::Low // present in history, but no recent session
            };

            (
                id,
                Ability {
                    e1rm,
                    // Floor reps (conservative — never claim a rep you can't show).
                    best_reps: best_reps.map(|r| r.floor() as i32),
                    best_hold: best_hold.map(|h| h.round() as i32),
                    confidence,
                    sessions_recent,
                },
            )
        })
        .collect()
}

/// Confidence for an exercise given the ability map — `None` when it's absent
/// (never trained).
pub fn confidence_of(abilities: &HashMap<i64, Ability>, exercise_id: i64) -> Confidence {
    abilities
        .get(&exercise_id)
        .map(|a| a.confidence)
        .unwrap_or(Confidence::None)
}
