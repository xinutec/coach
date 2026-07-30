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
//!   * **A ceiling from recent work** — the max is then held under a multiple of
//!     the best of the last few sessions (`CAP_MULTIPLE`, `CAP_SESSIONS`). A max
//!     can only ever be argued *upwards*, so without this a decline — injury,
//!     illness, a worse year — is unrepresentable: the honest low measurement
//!     comes back and is discarded by the same max that protects the old PR. The
//!     cap only ever lowers, so both guarantees above survive it.
//!
//! Confidence is separate from the estimate: it counts *recent* sessions, and
//! (in later stages) decides whether the engine prescribes from the estimate or
//! asks for a fresh assessment.

use crate::prelude::*;

use crate::domain::{ExerciseId, SetId};
use alloc::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDate, NaiveDateTime};
use serde::Serialize;

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
/// A break in an exercise's history longer than this splits it into a new
/// training block. **Only the most-recent block estimates ability** — so after a
/// real interruption (a long layoff, a health setback), your current level is
/// read from your *return*, not from a pre-break PR that no longer describes you.
/// Continuous training leaves everything in one block (the former behaviour). Set
/// beyond normal rotation/rest so an ordinary week off never resets you, but well
/// under the detraining timescale so a genuine break does.
const BLOCK_GAP_WEEKS: i64 = 8;
/// Recent sessions (distinct days) needed for `High` / `Medium` confidence.
/// `pub` so the engine's confirmation-need can measure "sessions still owed before
/// this is trusted" against the *same* bar that grants the trust — the two must not
/// drift.
pub const HIGH_SESSIONS: i32 = 3;
const MEDIUM_SESSIONS: i32 = 1;
/// Ability may not exceed this multiple of what the athlete has actually shown
/// across their last `CAP_SESSIONS` sessions.
///
/// Ability is a **max**, which is what lets a real PR survive a quiet fortnight —
/// and is also why a genuine decline was otherwise unrepresentable. An injury, an
/// illness, or simply a worse year produces an honest low measurement, and the max
/// discards it in favour of a number that no longer describes the athlete. Decay
/// can't rescue that (it floors at `DECAY_FLOOR`, well above a real setback) and
/// neither can the block reset (it needs a gap the athlete never takes, because
/// they keep turning up). What's left is a closed loop: the coach prescribes what
/// it wrongly believes, the athlete misses, the miss re-opens the measurement, the
/// measurement comes back low, the max throws it away, and the next prescription
/// is nearly as heavy again.
///
/// A ceiling drawn from recent work is the cheapest cut in that loop that is still
/// a **pure function of set history** — it reads no clock beyond `now` and nothing
/// about what the coach *asked*, so the residual ledger can go on replaying this
/// estimator without the two ending up calling each other.
///
/// The multiple is headroom for the easing the coach itself prescribes: a
/// low-readiness day asks two reps fewer (`dose::LOW_READINESS_EXTRA_RIR`), and a
/// complying athlete then logs a set that understates them. Too tight and the
/// coach follows its own easing downwards; too loose and a real decline never gets
/// caught.
const CAP_MULTIPLE: f64 = 1.15;
/// Sessions the ceiling reads. Deliberately the same bar as [`HIGH_SESSIONS`]: the
/// cap should bite exactly when the engine trusts the estimate enough to prescribe
/// from it, and not before. Below that bar there isn't enough recent evidence to
/// overrule a max, and the engine is measuring rather than prescribing anyway.
///
/// Reading several sessions rather than the latest one is what separates a decline
/// from a bad day. Easing and off-days are intermittent, so a full-effort session
/// usually survives somewhere in the window; a real decline is present in every
/// one of them.
const CAP_SESSIONS: usize = HIGH_SESSIONS as usize;

/// How much the engine trusts an exercise's estimate — the gate between
/// prescribing (from the estimate) and assessing (measuring afresh, G3). Also
/// surfaced in the explanation trace, so it's a wire type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
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
    /// Decayed best loaded carry — a weight *and* a time, because a carry is both
    /// and neither number means anything alone. `None` for an exercise never
    /// carried under load.
    pub carry: Option<Carry>,
    pub confidence: Confidence,
    /// Distinct recent days the exercise was trained (drives confidence).
    pub sessions_recent: i32,
    /// The set that actually set this estimate — the max is one real set, and
    /// this is it.
    ///
    /// Ability is a max, so a single wrong number lingers: it decays only to
    /// `DECAY_FLOOR`, `BLOCK_GAP_WEEKS` never fires while training continues, and
    /// an honest re-measurement is *lower* and loses. `CAP_MULTIPLE` now bounds
    /// how far it can hold out — but only once `CAP_SESSIONS` sessions have
    /// accumulated to bound it with, and only to within that multiple. The
    /// estimate is properly correctable only if the athlete can be shown which set
    /// produced it — otherwise "the coach is asking for something absurd" is an
    /// archaeology problem, and the offending set is usually weeks back, out of
    /// reach of anything that only offers the latest one.
    ///
    /// When the ceiling binds, this names the *recent* set that set the ceiling:
    /// that is the set the number now comes from, and the old high one has already
    /// been overruled.
    pub source: Option<Source>,
}

/// The set behind an estimate: enough to recognise it, and its row id so it can
/// be corrected. Which metric it set is implied by the estimate it accompanies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub set_id: SetId,
    pub logged_at: NaiveDateTime,
    pub load_kg: Option<f64>,
    pub reps: Option<i32>,
    pub hold_s: Option<i32>,
}

/// What a loaded carry demonstrated: this weight, for this long.
///
/// The two travel together on purpose. "12 kg" says nothing without the duration
/// and "30 s" says nothing without the weight, so an `Option<f64>` apiece would
/// let a caller read one and prescribe from it — which is how the carries ended up
/// being prescribed in reps in the first place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carry {
    pub load: f64,
    pub secs: i32,
}

/// The better of two carries: the heavier weight wins, and at equal weight the
/// longer time. Weight first because that is the direction progression runs —
/// time climbs to a ceiling, then the load steps and the clock resets (see
/// `engine::prescribe`), so a longer carry at a lighter weight is not an
/// improvement on a shorter one at a heavier.
fn better_carry(cur: Option<Carry>, c: Carry) -> Option<Carry> {
    Some(match cur {
        Some(b) if (b.load, b.secs) >= (c.load, c.secs) => b,
        Some(_) | None => c,
    })
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

fn source_of(s: &SetRec) -> Source {
    Source {
        set_id: s.id,
        logged_at: s.logged_at,
        load_kg: s.load_kg,
        reps: s.reps,
        hold_s: s.hold_s,
    }
}

/// The best decayed estimate in each metric over some window of an exercise's
/// sets, and the set behind each one.
///
/// It is a type so that the recent **ceiling** is computed by the same code as the
/// estimate it caps, differing only in which sets are fed to it. Two hand-written
/// copies of "the best set in here" would be two chances to disagree about what
/// *best* means, and a ceiling that measures something slightly different from the
/// estimate it bounds is a permanent quiet bias rather than a visible bug.
#[derive(Default)]
struct Bests {
    e1rm: Option<f64>,
    reps: Option<f64>,
    hold: Option<f64>,
    carry: Option<Carry>,
    e1rm_src: Option<Source>,
    reps_src: Option<Source>,
    hold_src: Option<Source>,
}

impl Bests {
    /// Fold in one set, already scaled by `d` — its own staleness.
    fn feed(&mut self, s: &SetRec, d: f64) {
        match (s.load_kg, s.reps) {
            // Weighted: load + reps → an e1RM estimate.
            (Some(load), Some(reps)) => {
                let v = epley(load, reps, s.rpe) * d;
                if self.e1rm.is_none_or(|m: f64| v > m) {
                    self.e1rm_src = Some(source_of(s));
                }
                self.e1rm = max_opt(self.e1rm, v);
            }
            // Bodyweight reps: reps, no load → effective-rep estimate.
            (None, Some(reps)) => {
                let v = (reps as f64 + rir(s.rpe)) * d;
                if self.reps.is_none_or(|m: f64| v > m) {
                    self.reps_src = Some(source_of(s));
                }
                self.reps = max_opt(self.reps, v);
            }
            _ => {}
        }
        // A hold set (isometric) carries hold_s regardless of the above.
        if let Some(h) = s.hold_s {
            let v = h as f64 * d;
            if self.hold.is_none_or(|m: f64| v > m) {
                self.hold_src = Some(source_of(s));
            }
            self.hold = max_opt(self.hold, v);
        }
        // A loaded carry: weight *and* time. Both decay, so idleness pulls the
        // estimate down as one — it can't quietly keep the weight while forgetting
        // the duration, or the reverse.
        if let (Some(load), Some(h)) = (s.load_kg, s.hold_s) {
            self.carry = better_carry(
                self.carry,
                Carry {
                    load: load * d,
                    secs: libm::floor(h as f64 * d) as i32,
                },
            );
        }
    }
}

/// Hold an estimate under the recent ceiling, and hand back the set that explains
/// whichever number survives.
///
/// The source moves with the number deliberately. `source` answers "which set
/// produced this?", and once the ceiling binds, the old high set is no longer the
/// answer — nor is it still worth correcting, since it has already been overruled
/// by more recent work.
fn under_ceiling(
    est: Option<f64>,
    src: Option<Source>,
    ceiling: Option<f64>,
    ceiling_src: Option<Source>,
) -> (Option<f64>, Option<Source>) {
    match (est, ceiling) {
        (Some(e), Some(c)) if e > CAP_MULTIPLE * c => (Some(CAP_MULTIPLE * c), ceiling_src),
        _ => (est, src),
    }
}

/// The same ceiling for a carry, applied to each half. Both are capped because
/// both are prescribed from: a carry held to its recent weight but not its recent
/// duration would still ask for a walk nobody has taken.
fn carry_under_ceiling(carry: Option<Carry>, ceiling: Option<Carry>) -> Option<Carry> {
    match (carry, ceiling) {
        (Some(c), Some(k)) => Some(Carry {
            load: c.load.min(CAP_MULTIPLE * k.load),
            secs: c.secs.min(libm::floor(CAP_MULTIPLE * k.secs as f64) as i32),
        }),
        _ => carry,
    }
}

/// Estimate ability for every exercise present in `history`. Exercises absent
/// from the returned map have never been trained → treat as `Confidence::None`.
pub fn abilities(history: &[SetRec], now: NaiveDateTime) -> BTreeMap<ExerciseId, Ability> {
    let mut by_ex: BTreeMap<ExerciseId, Vec<&SetRec>> = BTreeMap::new();
    for s in history {
        by_ex.entry(s.exercise_id).or_default().push(s);
    }
    by_ex
        .into_iter()
        .map(|(id, sets)| (id, estimate(&sets, now)))
        .collect()
}

/// Ability from one exercise's sets. Separate from [`abilities`] because the
/// residual ledger asks the same question of a *prefix* of history — "what did the
/// engine believe before this session?" — and must get the identical answer the
/// engine would have given at the time, not an approximation of it.
pub fn estimate(sets: &[&SetRec], now: NaiveDateTime) -> Ability {
    let window_cut = now - Duration::weeks(CONFIDENCE_WEEKS);
    let block_gap = Duration::weeks(BLOCK_GAP_WEEKS);

    // The most-recent contiguous training block: walk back from the newest set until
    // a gap longer than `BLOCK_GAP_WEEKS`. Only this block estimates ability, so a
    // pre-break PR can't raise the estimate — or a prescription — above what the
    // return has actually shown. Continuous training is one block, and sets on the
    // same day never split (they're one session, so the chimera guard still holds).
    // Confidence still counts recent days across *all* sets.
    let mut sets: Vec<&SetRec> = sets.to_vec();
    sets.sort_by_key(|s| core::cmp::Reverse(s.logged_at)); // newest first
    let block_cut = {
        let mut cut = sets.first().map(|s| s.logged_at);
        let mut prev: Option<NaiveDateTime> = None;
        for s in &sets {
            if let Some(p) = prev
                && p - s.logged_at > block_gap
            {
                break; // this set is on the far side of a real break
            }
            cut = Some(s.logged_at);
            prev = Some(s.logged_at);
        }
        cut
    };

    // The newest `CAP_SESSIONS` training days inside the block — the window the
    // ceiling reads. Days rather than sets: five sets in one session are one piece
    // of evidence about today's ceiling, not five, and counting sets would let a
    // single high-volume day stand in for the run of sessions this is meant to see.
    let cap_cut: Option<NaiveDate> = {
        let mut days: Vec<NaiveDate> = Vec::new();
        for s in &sets {
            if block_cut.is_some_and(|c| s.logged_at < c) {
                break; // the block edge — older sets describe a different athlete
            }
            let day = s.logged_at.date();
            if days.last() != Some(&day) {
                days.push(day); // sets run newest-first, so days do too
            }
            if days.len() == CAP_SESSIONS {
                break;
            }
        }
        if days.len() == CAP_SESSIONS {
            days.last().copied() // the oldest of them — the window's far edge
        } else {
            None // fewer sessions is not enough recent evidence to overrule a max
        }
    };

    let mut all = Bests::default();
    let mut recent = Bests::default();
    let mut recent_days: BTreeSet<_> = BTreeSet::new();

    for s in &sets {
        // Confidence sees every recent set; the estimate only the block.
        if s.logged_at >= window_cut {
            recent_days.insert(s.logged_at.date());
        }
        if block_cut.is_some_and(|c| s.logged_at < c) {
            continue; // pre-break history — doesn't estimate today's ability
        }
        let age = (now - s.logged_at).num_seconds().max(0) as f64 / 86_400.0;
        let d = decay(age);
        all.feed(s, d);
        if cap_cut.is_some_and(|c| s.logged_at.date() >= c) {
            recent.feed(s, d);
        }
    }

    // The ceiling is built from the *decayed* estimates, same as the max it caps.
    // That is what keeps ability monotone under idleness: as an exercise sits, every
    // term on both sides falls, so neither the estimate nor its ceiling can rise,
    // and a `min` of two non-increasing numbers is non-increasing.
    let (e1rm, e1rm_src) = under_ceiling(all.e1rm, all.e1rm_src, recent.e1rm, recent.e1rm_src);
    let (best_reps, reps_src) = under_ceiling(all.reps, all.reps_src, recent.reps, recent.reps_src);
    let (best_hold, hold_src) = under_ceiling(all.hold, all.hold_src, recent.hold, recent.hold_src);
    let carry = carry_under_ceiling(all.carry, recent.carry);

    let sessions_recent = recent_days.len() as i32;
    let confidence = if sessions_recent >= HIGH_SESSIONS {
        Confidence::High
    } else if sessions_recent >= MEDIUM_SESSIONS {
        Confidence::Medium
    } else {
        Confidence::Low // present in history, but no recent session
    };

    Ability {
        e1rm,
        // Floor reps (conservative — never claim a rep you can't show).
        best_reps: best_reps.map(|r| libm::floor(r) as i32),
        best_hold: best_hold.map(|h| libm::round(h) as i32),
        carry,
        confidence,
        sessions_recent,
        // An exercise is measured in one metric, so at most one of these is the
        // number the athlete is shown; prefer them in the order the prescription
        // reads them.
        source: e1rm_src.or(reps_src).or(hold_src),
    }
}

/// Confidence for an exercise given the ability map — `None` when it's absent
/// (never trained).
pub fn confidence_of(
    abilities: &BTreeMap<ExerciseId, Ability>,
    exercise_id: ExerciseId,
) -> Confidence {
    abilities
        .get(&exercise_id)
        .map(|a| a.confidence)
        .unwrap_or(Confidence::None)
}
