//! The ability model: what the athlete can do *today* per exercise, as a pure function
//! of logged sets and `now`.
//!
//! - **RPE-aware e1RM:** `reps` at `load` with `rir` in reserve is worth `load × (1 +
//!   (reps + rir)/30)` (Epley, extended for reserve); no RPE means `rir = 0`.
//! - **Per-set staleness decay, then max:** each set decays by its own age (full trust
//!   for two weeks, then down to a floor), so ability never rises with idleness, yet an
//!   old PR is not forgotten.
//! - **A ceiling from recent work** (`CAP_MULTIPLE` × the best of the last
//!   `CAP_SESSIONS`): a max alone can never register a decline. The cap only lowers,
//!   so both guarantees above survive it.
//!
//! Confidence counts *recent* sessions and decides between prescribing and measuring.

use crate::num::{count, whole};
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
/// A gap longer than this splits an exercise's history into blocks, and only the latest
/// block estimates ability: after a real break the level is read from the return, not
/// from a pre-break PR. Longer than an ordinary week off, shorter than the detraining
/// timescale.
const BLOCK_GAP_WEEKS: i64 = 8;
/// Recent sessions (distinct days) needed for `High` / `Medium` confidence.
/// `pub` so the engine's confirmation-need can measure "sessions still owed before
/// this is trusted" against the *same* bar that grants the trust — the two must not
/// drift.
pub const HIGH_SESSIONS: i32 = 3;
const MEDIUM_SESSIONS: i32 = 1;
/// Ability may not exceed this multiple of what the athlete has shown in their last
/// [`CAP_SESSIONS`] sessions.
///
/// Ability is a max, so on its own it discards an honest low measurement (injury,
/// illness, a worse year), and neither decay nor the block reset can rescue that while
/// the athlete keeps training. The ceiling is a pure function of set history, so the
/// ledger can keep replaying this estimator. The multiple is headroom for the coach's
/// own easing: too tight and the coach follows its easing down, too loose and a decline
/// is never caught.
const CAP_MULTIPLE: f64 = 1.15;
/// Sessions the ceiling reads: the same bar as [`HIGH_SESSIONS`], so the cap bites
/// exactly when the estimate is trusted enough to prescribe from. Several sessions, not
/// the latest, separate a decline (present in every one) from a bad or eased day
/// (intermittent).
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
    /// Decayed best distance carry — the metre-measured half.
    pub carry_m: Option<CarryDistance>,
    pub confidence: Confidence,
    /// Distinct recent days the exercise was trained (drives confidence).
    pub sessions_recent: i32,
    /// The one set this estimate comes from, shown so a wrong number can be corrected:
    /// a mistyped max lingers for weeks, and the offending set is usually too old for
    /// anything that only offers the latest one. When the ceiling binds, this is the
    /// recent set that set it.
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

/// What a loaded carry demonstrated: this weight, for this long. The two travel
/// together because neither means anything alone; separate options would let a caller
/// prescribe from one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carry {
    pub load: f64,
    pub secs: i32,
}

/// What a distance carry demonstrated: this weight, for this far. The metre twin
/// of [`Carry`], kept separate because the two are not interchangeable — you
/// cannot convert one to the other without inventing a walking pace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarryDistance {
    pub load: f64,
    pub metres: i32,
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
    rpe.map(|r| f64::from((10 - r).max(0))).unwrap_or(0.0)
}

/// Epley 1RM extended for reps-in-reserve: what the set implies you could lift
/// once. `reps + rir` is the effective rep count taken to failure.
fn epley(load: f64, reps: i32, rpe: Option<i32>) -> f64 {
    load * (1.0 + (f64::from(reps) + rir(rpe)) / 30.0)
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

/// The best decayed estimate per metric over a window of sets, and the set behind each.
/// A type, so the recent ceiling is computed by the same code as the estimate it caps,
/// and the two cannot mean different things by "best".
#[derive(Default)]
struct Bests {
    e1rm: Option<f64>,
    reps: Option<f64>,
    hold: Option<f64>,
    carry: Option<Carry>,
    carry_m: Option<CarryDistance>,
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
                let v = (f64::from(reps) + rir(s.rpe)) * d;
                if self.reps.is_none_or(|m: f64| v > m) {
                    self.reps_src = Some(source_of(s));
                }
                self.reps = max_opt(self.reps, v);
            }
            _ => {}
        }
        // A hold set (isometric) carries hold_s regardless of the above.
        if let Some(h) = s.hold_s {
            let v = f64::from(h) * d;
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
                    secs: whole(libm::floor(f64::from(h) * d)),
                },
            );
        }
        // The metre-measured carry, decayed the same way and ranked the same way:
        // heavier first, then further.
        if let (Some(load), Some(m)) = (s.load_kg, s.distance_m) {
            let c = CarryDistance {
                load: load * d,
                metres: whole(libm::floor(f64::from(m) * d)),
            };
            self.carry_m = Some(match self.carry_m {
                Some(b) if (b.load, b.metres) >= (c.load, c.metres) => b,
                Some(_) | None => c,
            });
        }
    }
}

/// Hold an estimate under the recent ceiling, and return the set that explains
/// whichever number survives: once the ceiling binds, the old high set is no longer the
/// answer.
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

/// The distance carry's twin of [`carry_under_ceiling`] — the same R6-3 rule, so
/// a returning athlete's old 30 m cannot be prescribed off a recent 10 m.
fn carry_m_under_ceiling(
    carry: Option<CarryDistance>,
    ceiling: Option<CarryDistance>,
) -> Option<CarryDistance> {
    match (carry, ceiling) {
        (Some(c), Some(k)) => Some(CarryDistance {
            load: c.load.min(CAP_MULTIPLE * k.load),
            metres: c
                .metres
                .min(whole(libm::floor(CAP_MULTIPLE * f64::from(k.metres)))),
        }),
        _ => carry,
    }
}

/// The same ceiling for a carry, applied to each half. Both are capped because
/// both are prescribed from: a carry held to its recent weight but not its recent
/// duration would still ask for a walk nobody has taken.
fn carry_under_ceiling(carry: Option<Carry>, ceiling: Option<Carry>) -> Option<Carry> {
    match (carry, ceiling) {
        (Some(c), Some(k)) => Some(Carry {
            load: c.load.min(CAP_MULTIPLE * k.load),
            secs: c
                .secs
                .min(whole(libm::floor(CAP_MULTIPLE * f64::from(k.secs)))),
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

    // The latest training block: walk back until a gap longer than `BLOCK_GAP_WEEKS`,
    // so a pre-break PR cannot raise the estimate. Same-day sets never split.
    // Confidence still counts recent days across all sets.
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
    let carry_m = carry_m_under_ceiling(all.carry_m, recent.carry_m);

    let sessions_recent = count(recent_days.len());
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
        best_reps: best_reps.map(|r| whole(libm::floor(r))),
        best_hold: best_hold.map(|h| whole(libm::round(h))),
        carry,
        carry_m,
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
