//! E3 — simulate an athlete into the future and watch the coach adapt.
//!
//! The back-test (E1) replays history that already happened; it can never show
//! how the engine responds to *its own* prescriptions. This does: starting from
//! the real logged history in a **dev** DB, a deterministic simulated athlete
//! reads each day's verdict exactly as the UI presents it (the `Suggestion`
//! cards), performs what was asked as well as their *true* ability allows, and
//! logs the results — instruct → try → record, never reporting an RPE. The walk
//! then continues on the grown history, so the loop the athlete actually lives
//! in (prescribe → perform → re-estimate → prescribe) runs for weeks in
//! seconds.
//!
//! The athlete's true ability is initialised from the real history's own
//! estimates and then evolves along a **temperament** curve:
//!
//! - `improver`   — steady gains, week on week
//! - `plateauer`  — two weeks of gains, then flat forever
//! - `badweek`    — an improver whose week 3 goes badly and recovers
//! - `novice`     — opens well *below* what the history says, and climbs fast
//! - `strong`     — opens well *above* it (trained elsewhere, only just logging)
//! - `injured`    — an improver who hurts one muscle group in week 2 and stays hurt
//!
//! Temperament moves the hidden *ability*. How the athlete **behaves** towards
//! the coach is a separate axis (`SIM_BEHAVIOUR`), because the two break
//! different things: the ledger reads logged sets, and a set that never happened
//! is a different signal from a set that fell short.
//!
//! - `compliant`    — does exactly what each card says, every day it says to
//! - `skipper`      — trains three days a week whatever the plan says
//! - `partial`      — leaves after the first 60 % of the work cards
//! - `overachiever` — doesn't stop at the ask when the reps are there
//! - `improviser`   — grabs the bell below the one on the card
//! - `layoff`       — trains a fortnight, vanishes for three weeks, comes back
//!
//! Everything is deterministic (no randomness, no wall clock), so a trace diffs
//! cleanly across engine changes — the same regression signal as the back-test,
//! but over futures the history doesn't contain. The model's absolute numbers
//! don't need to be right; they need to make the *coaching* visible: does a miss
//! get answered, does a plateau get noticed, does progression step when earned?
//!
//! Usage (dev DB seeded from a prod dump — see scripts/simulate.sh):
//!   DATABASE_URL=mysql://coach:coach@127.0.0.1:3308/coach cargo run --bin simulate
//!   SIM_WEEKS     — how many weeks to walk forward (default 8)
//!   SIM_ATHLETE   — improver | plateauer | badweek | novice | strong | injured
//!   SIM_BEHAVIOUR — compliant | skipper | partial | overachiever | improviser | layoff
//!   SIM_RECOVERY  — untracked | rested | roughweek (default untracked)
//!   SIM_USER      — user id (default pippijn)
//!   SIM_LOCATION  — location by name (default: the user's default)
//!
//! `SIM_RECOVERY` drives the biometric readiness the coach reads each morning —
//! the axis the temperament can't reach. `untracked` (the default) hands the
//! engine no readiness, so a run reproduces the pre-readiness behaviour exactly.
//! `roughweek` sleeps the athlete poorly through the third week: the coach eases
//! the ask on those Low mornings, and — because it now reconstructs the readiness
//! each session was written under (R5-2) — a compliant eased session must *not*
//! turn up as a miss in the ledger. That is the loop the unit tests can't run.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result, bail};
use chrono::{Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};

use coach::exercise::types::Metric;
use coach::health::Recovery as RawRecovery;
use coach::location::repo as location_repo;
use coach::muscle::types::MuscleRole;
use coach::pacing::types::{Ask, PacingState, Readiness, SetRec, Suggestion, SuggestionKind};
use coach::pacing::{ability, engine, readiness, residual, service};
use coach::workout::repo as workout_repo;
use coach_pacing::domain::{ExerciseId, SetId};

/// When the athlete checks the app and (if told to) trains. Inside a default
/// training window; sets are logged from shortly after.
const SESSION_HOUR: u32 = 9;
/// Minutes between logged sets — enough to keep timestamps ordered and honest.
const SET_GAP_MIN: i64 = 4;

// ---- the athlete's true (hidden) ability -----------------------------------

/// What a never-before-trained exercise is truly worth. Arbitrary but
/// deterministic: the point is to expose coaching behaviour on a fresh
/// movement (assess → prescribe), not to model this athlete exactly.
const DEFAULT_E1RM: f64 = 28.0;
const DEFAULT_REPS: i32 = 8;
const DEFAULT_HOLD_S: i32 = 25;
const DEFAULT_CARRY: (f64, i32) = (16.0, 40);

/// The group the `injured` temperament hurts. A shoulder, because it is the
/// joint that quietly limits the most of a session — pressing, hanging, carrying
/// and rowing all route through it.
const INJURED_GROUP: &str = "Deltoids";

/// Weekly strength gain for an improving athlete (fractional, on e1RM and
/// carry loads) and the rep/hold analogues. Deliberately modest — real novice
/// gains on light kit, not a montage.
const GAIN_PER_WEEK: f64 = 0.015;
const REPS_PER_WEEK: f64 = 0.75;
const HOLD_S_PER_WEEK: f64 = 4.0;

/// A novice opens at a little over half what the logged history claims and
/// climbs at roughly twice the improver's rate — a deconditioned return, or the
/// app changing hands. The engine starts out believing the *old* numbers, so
/// this is the athlete the coach is most at risk of hurting.
const NOVICE_START: f64 = 0.55;
const NOVICE_GAIN_MULT: f64 = 2.0;
/// The mirror case: someone who trained elsewhere for a year and only started
/// logging last week. Nothing in the history says how strong they are, so every
/// number the coach holds is an underestimate it has to climb out of.
const STRONG_START: f64 = 2.5;
/// What an injured group is worth once it goes: a tweaked shoulder doesn't take
/// a fortnight off, it takes most of your press away and keeps it.
const INJURY_MULT: f64 = 0.4;
/// The sim week the injury lands in.
const INJURY_WEEK: i64 = 2;

/// Detraining: what *not* training costs.
///
/// ⚠ This is deliberately NOT a [`Temperament`], which is the modelling gap
/// round 6 named. Every temperament banks progress as a function of the sim
/// week, so an athlete who disappears for three weeks comes back *stronger* —
/// and the 21-day layoff therefore tested re-entry after silence rather than
/// after decline. Ability and compliance were separated on the grounds that they
/// move independently, and disuse is exactly where they do not: it is caused by
/// the behaviour and it moves the ability.
///
/// So it follows from the days actually trained rather than from the layoff
/// window. A skipper's two-day gaps cost nothing; three weeks away costs real
/// strength; and nothing has to know which temperament is running.
///
/// The numbers are the modest end of the literature, matching the rest of this
/// file's refusal to make a montage of it: a trained person loses little in the
/// first week off, then roughly half a percent a day, and regains it about twice
/// as fast on return — the asymmetry anyone who has come back from a layoff
/// recognises. Endurance goes first and further, which is why holds and reps
/// carry their own multiple.
const DETRAIN_GRACE_DAYS: i64 = 7;
const DETRAIN_PER_DAY: f64 = 0.005;
const DETRAIN_MAX: f64 = 0.20;
const REGAIN_PER_DAY: f64 = 0.010;
const DETRAIN_ENDURANCE_MULT: f64 = 2.0;

/// Parse an axis from its env-var spelling, and say what the spellings are.
///
/// Both halves come from one list because they drifted apart when they didn't:
/// round 6 added `novice`, `strong` and `injured` to `Temperament::parse` and
/// left `SIM_ATHLETE`'s rejection message naming the original three, so the one
/// output a reader consults *after* getting it wrong told them the new athletes
/// did not exist. Same shape as `db_str!` in `coach-pacing`.
macro_rules! sim_axis {
    ($name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        impl $name {
            fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some(Self::$variant),)+ _ => None }
            }
            /// The accepted spellings, for the message shown when parsing fails.
            fn names() -> String {
                [$($s),+].join(" | ")
            }
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Temperament {
    Improver,
    Plateauer,
    BadWeek,
    Novice,
    Strong,
    Injured,
}

sim_axis!(Temperament {
    Improver => "improver",
    Plateauer => "plateauer",
    BadWeek => "badweek",
    Novice => "novice",
    Strong => "strong",
    Injured => "injured",
});

impl Temperament {
    /// Where true ability *opens*, as a fraction of what the real history's own
    /// estimates say. Everyone but the novice and the strong athlete starts
    /// where the engine believes they are — for those two the opening gap is
    /// the whole point of the run.
    fn start_scale(self) -> f64 {
        match self {
            Self::Novice => NOVICE_START,
            Self::Strong => STRONG_START,
            _ => 1.0,
        }
    }

    /// Weeks of progress banked by sim week `w` — the plateauer stops banking
    /// after two; the novice banks at double rate.
    fn banked(self, w: i64) -> f64 {
        match self {
            Self::Plateauer => w.min(2) as f64,
            Self::Novice => w as f64 * NOVICE_GAIN_MULT,
            Self::Improver | Self::BadWeek | Self::Strong | Self::Injured => w as f64,
        }
    }

    /// Multiplier on strength-like numbers (e1RM, carry load) at sim week `w`.
    fn strength(self, w: i64) -> f64 {
        let dip = match (self, w) {
            (Self::BadWeek, 3) => 0.88,
            (Self::BadWeek, 4) => 0.96,
            _ => 1.0,
        };
        (1.0 + GAIN_PER_WEEK * self.banked(w)) * dip
    }

    /// Added reps on rep work at sim week `w`.
    fn reps(self, w: i64) -> f64 {
        let dip = match (self, w) {
            (Self::BadWeek, 3) => 2.0,
            (Self::BadWeek, 4) => 1.0,
            _ => 0.0,
        };
        REPS_PER_WEEK * self.banked(w) - dip
    }

    /// Added seconds on holds at sim week `w`.
    fn hold(self, w: i64) -> f64 {
        let dip = match (self, w) {
            (Self::BadWeek, 3) => 8.0,
            (Self::BadWeek, 4) => 4.0,
            _ => 0.0,
        };
        HOLD_S_PER_WEEK * self.banked(w) - dip
    }

    /// What an exercise on the injured group is worth at sim week `w`. It
    /// applies to that one group: the point of the case is that the rest of the
    /// athlete keeps improving, so a coach reading the body as a single number
    /// cannot see it.
    fn injury(self, w: i64, on_injured_group: bool) -> f64 {
        match self {
            Self::Injured if on_injured_group && w >= INJURY_WEEK => INJURY_MULT,
            _ => 1.0,
        }
    }
}

// ---- how the athlete behaves towards the coach -------------------------------

/// Compliance, which [`Temperament`] deliberately doesn't model. A missed set and
/// a set that never happened reach the engine as different signals — one is
/// evidence, the other is silence — and the coach has to read both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Behaviour {
    /// Does exactly what each card says, on every day the coach says train.
    Compliant,
    /// Three days a week, whatever the plan says.
    Skipper,
    /// Leaves partway through: the last cards never get done.
    Partial,
    /// Doesn't stop at the ask when the reps are there.
    Overachiever,
    /// Reaches for the bell below the one on the card.
    Improviser,
    /// Trains a fortnight, disappears for three weeks, comes back.
    Layoff,
}

/// Reps the overachiever adds to an ask they could beat, and the seconds they
/// add to a hold. Small on purpose — this is "one more because it felt light",
/// not a different athlete.
const OVER_REPS: i32 = 2;
const OVER_HOLD_S: i32 = 5;
/// How much of the plan the quitter gets through before life intervenes.
const PARTIAL_FRACTION: f64 = 0.6;
/// The layoff: away from `LAYOFF_FROM` until `LAYOFF_TO` (sim days, 0-based).
const LAYOFF_FROM: i64 = 14;
const LAYOFF_TO: i64 = 35;

sim_axis!(Behaviour {
    Compliant => "compliant",
    Skipper => "skipper",
    Partial => "partial",
    Overachiever => "overachiever",
    Improviser => "improviser",
    Layoff => "layoff",
});

impl Behaviour {
    /// Does the athlete turn up on sim day `d` at all? The coach still computes
    /// the day's verdict either way — what changes is whether any sets come back.
    fn attends(self, d: i64) -> bool {
        match self {
            Self::Skipper => matches!(d % 7, 0 | 2 | 4),
            Self::Layoff => !(LAYOFF_FROM..LAYOFF_TO).contains(&d),
            _ => true,
        }
    }

    /// How many of the day's work cards actually get done.
    fn cards_done(self, offered: usize) -> usize {
        match self {
            Self::Partial => ((offered as f64 * PARTIAL_FRACTION).ceil() as usize).max(1),
            _ => offered,
        }
    }

    /// The reps the athlete *aims* for, given the card's ask. Everyone but the
    /// overachiever stops where they were told.
    fn rep_target(self, ask: i32) -> i32 {
        match self {
            Self::Overachiever => ask + OVER_REPS,
            _ => ask,
        }
    }

    /// The same, for a hold or a carry.
    fn hold_target(self, ask: i32) -> i32 {
        match self {
            Self::Overachiever => ask + OVER_HOLD_S,
            _ => ask,
        }
    }

    /// The bell actually picked up for an ask of `asked` kg. The ledger judges
    /// the ask at the load *logged* (R5-1), so an improvised weight must come
    /// out honest rather than as a shortfall — this is the case that proves it.
    fn load_used(self, asked: f64, owned: Option<&Vec<f64>>) -> f64 {
        if self != Self::Improviser {
            return asked;
        }
        let mut ws: Vec<f64> = owned.cloned().unwrap_or_default();
        ws.sort_by(f64::total_cmp);
        ws.iter()
            .copied()
            .rfind(|w| *w < asked - 1e-9)
            .unwrap_or(asked)
    }
}

// ---- the athlete's recovery (readiness), the axis temperament can't reach -----

/// How the athlete slept — the biometric readiness the coach reads each morning.
/// Separate from [`Temperament`] because ability and recovery move independently:
/// a well-trained athlete still has a bad night, and the coach must ease *that*
/// morning without recording it against them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sleep {
    /// Health has nothing — readiness absent. Reproduces the pre-readiness run.
    Untracked,
    /// Always well-rested — readiness stays high.
    Rested,
    /// A poor-sleep stretch through the third sim week, easing back after.
    RoughWeek,
}

sim_axis!(Sleep {
    Untracked => "untracked",
    Rested => "rested",
    RoughWeek => "roughweek",
});

impl Sleep {
    /// Hours slept before sim day `d` (0-based), or `None` when the night wasn't
    /// tracked at all. Feeds the readiness score via the real compose function.
    fn hours(self, d: i64) -> Option<f64> {
        match self {
            Self::Untracked => None,
            Self::Rested => Some(8.0),
            Self::RoughWeek => Some(match d {
                14..=20 => 5.0, // the rough week — Low band, the coach eases
                21..=24 => 6.5, // climbing back to Normal
                _ => 8.0,       // rested either side of it
            }),
        }
    }

    /// The readiness the coach would compute that morning — through the *same*
    /// pure function prod uses (health hands over raw sleep, coach scores it), so
    /// the sim exercises the real readiness path, not a stand-in for it.
    fn readiness_on(self, d: i64) -> Option<Readiness> {
        let hours = self.hours(d)?;
        readiness::readiness(&RawRecovery {
            sleep_hours: Some(hours),
            hrv: None,
            resting_hr: None,
        })
    }
}

/// The athlete's true ability on one exercise at sim start — hidden from the
/// engine, which only ever sees the sets it produces.
#[derive(Clone, Copy)]
struct Base {
    e1rm: f64,
    reps: i32,
    hold_s: i32,
    carry: (f64, i32),
}

struct Athlete {
    temperament: Temperament,
    base: HashMap<ExerciseId, Base>,
    /// Exercises whose primary group is the injured one. Empty unless the
    /// temperament is [`Temperament::Injured`].
    injured: BTreeSet<ExerciseId>,
    /// Consecutive days without training, and the ability lost to them — see the
    /// detraining constants. State rather than a function of the day, because
    /// coming back does not undo a layoff instantly: the loss has to persist and
    /// then be regained.
    idle_days: i64,
    detrained: f64,
}

impl Athlete {
    /// Spend one sim day. `trained` means sets actually reached the log — not
    /// that the coach offered a session, and not that the athlete turned up and
    /// left, both of which are days the body spends idle.
    fn spend_day(&mut self, trained: bool) {
        if trained {
            self.idle_days = 0;
            self.detrained = (self.detrained - REGAIN_PER_DAY).max(0.0);
        } else {
            self.idle_days += 1;
            if self.idle_days > DETRAIN_GRACE_DAYS {
                self.detrained = (self.detrained + DETRAIN_PER_DAY).min(DETRAIN_MAX);
            }
        }
    }

    /// True ability at sim week `w`, seeding a deterministic default the first
    /// time an exercise is asked about.
    fn truth(&mut self, exercise_id: ExerciseId, seed: Option<&ability::Ability>, w: i64) -> Base {
        let t = self.temperament;
        let start = t.start_scale();
        let b = self.base.entry(exercise_id).or_insert_with(|| Base {
            e1rm: seed.and_then(|a| a.e1rm).unwrap_or(DEFAULT_E1RM) * start,
            reps: ((seed.and_then(|a| a.best_reps).unwrap_or(DEFAULT_REPS) as f64 * start).round()
                as i32)
                .max(1),
            hold_s: ((seed.and_then(|a| a.best_hold).unwrap_or(DEFAULT_HOLD_S) as f64 * start)
                .round() as i32)
                .max(5),
            carry: seed
                .and_then(|a| a.carry)
                .map(|c| (c.load, c.secs))
                .map(|(l, s)| (l * start, s))
                .unwrap_or(DEFAULT_CARRY),
        });
        let hurt = t.injury(w, self.injured.contains(&exercise_id));
        // Disuse multiplies what banked progress produced rather than unbanking
        // it: the weeks were still trained, the body has just let some of it go.
        let idle = 1.0 - self.detrained;
        let idle_endurance = (1.0 - self.detrained * DETRAIN_ENDURANCE_MULT).max(0.5);
        Base {
            e1rm: b.e1rm * t.strength(w) * hurt * idle,
            reps: ((b.reps as f64 + t.reps(w)) * hurt * idle_endurance)
                .round()
                .max(1.0) as i32,
            hold_s: ((b.hold_s as f64 + t.hold(w)) * hurt * idle_endurance)
                .round()
                .max(5.0) as i32,
            carry: (
                b.carry.0 * t.strength(w) * hurt * idle,
                (((b.carry.1 as f64 + t.hold(w) / 2.0) * hurt * idle_endurance).round()).max(5.0)
                    as i32,
            ),
        }
    }
}

// ---- performing a card -------------------------------------------------------

/// Reps to failure at `load` given a true 1RM — inverse Epley, floored at zero.
fn reps_at(e1rm: f64, load: f64) -> i32 {
    if load <= 0.0 {
        return 0;
    }
    (30.0 * (e1rm / load - 1.0)).floor().max(0.0) as i32
}

/// One performed set: what gets logged, and how to describe it in the trace.
struct Performed {
    reps: Option<i32>,
    load_kg: Option<f64>,
    hold_s: Option<i32>,
    distance_m: Option<i32>,
    note: String,
    missed: bool,
}

/// Do what the card asks, as well as true ability allows, in the manner of the
/// given [`Behaviour`] — and never report an RPE.
fn perform(
    s: &Suggestion,
    truth: Base,
    loads: Option<&Vec<f64>>,
    behaviour: Behaviour,
) -> Performed {
    /// "…and the card said 5 kg" — only when the athlete used something else.
    fn swapped(asked: f64, used: f64) -> String {
        if (asked - used).abs() > 1e-9 {
            format!("  (card said {asked} kg)")
        } else {
            String::new()
        }
    }
    // One match over what was actually asked. This used to be two nested matches
    // over `s.kind` and a tuple of Options, with a catch-all arm reading
    // "unintelligible card" — the simulator guessing at a prescription the engine
    // had computed exactly. It also had to consult the exercise's `metric` to tell
    // an AMRAP from a max hold, because the flat fields could not say; the ask now
    // says it, so the parameter is gone. Warm-ups are skipped by the caller.
    match s.ask {
        // Weighted reps: attempt the asked reps at the given load.
        Ask::Weighted {
            load_kg: asked_load,
            rep_low: ask,
            ..
        } => {
            let load = behaviour.load_used(asked_load, loads);
            let can = reps_at(truth.e1rm, load).max(1);
            let did = behaviour.rep_target(ask).min(can).max(1);
            Performed {
                reps: Some(did),
                load_kg: Some(load),
                hold_s: None,
                distance_m: None,
                note: format!(
                    "asked {ask} @ {load} kg, did {did}{}",
                    swapped(asked_load, load)
                ),
                missed: did < ask,
            }
        }
        // Bodyweight reps.
        Ask::Bodyweight { rep_low: ask, .. } => {
            let did = behaviour.rep_target(ask).min(truth.reps).max(1);
            Performed {
                reps: Some(did),
                load_kg: None,
                hold_s: None,
                distance_m: None,
                note: format!("asked {ask}, did {did}"),
                missed: did < ask,
            }
        }
        // Loaded carry: the asked seconds at the given weight, capacity scaling
        // with how far the weight is from the true one.
        Ask::WeightedHold {
            load_kg: asked_load,
            hold_s: ask,
        } => {
            let load = behaviour.load_used(asked_load, loads);
            let cap = ((truth.carry.1 as f64 * truth.carry.0 / load).floor() as i32).max(5);
            let did = behaviour.hold_target(ask).min(cap);
            Performed {
                reps: None,
                load_kg: Some(load),
                hold_s: Some(did),
                distance_m: None,
                note: format!(
                    "asked {ask}s @ {load} kg, did {did}s{}",
                    swapped(asked_load, load)
                ),
                missed: did < ask,
            }
        }
        // Unloaded hold.
        Ask::Hold { hold_s: ask } => {
            let did = behaviour.hold_target(ask).min(truth.hold_s).max(5);
            Performed {
                reps: None,
                load_kg: None,
                hold_s: Some(did),
                distance_m: None,
                note: format!("asked {ask}s, did {did}s"),
                missed: did < ask,
            }
        }
        // Build-up: work up to a hard-but-clean set of the asked reps. The athlete
        // lands on the heaviest owned weight that still leaves ~1 rep in reserve at
        // that count.
        Ask::BuildUp { reps, .. } => {
            let target = truth.e1rm / (1.0 + (reps as f64 + 1.0) / 30.0);
            let mut owned: Vec<f64> = loads.cloned().unwrap_or_default();
            owned.sort_by(f64::total_cmp);
            let load = owned
                .iter()
                .copied()
                .rfind(|w| *w <= target + 1e-9)
                .or_else(|| owned.first().copied())
                .unwrap_or(target);
            Performed {
                reps: Some(reps),
                load_kg: Some(load),
                hold_s: None,
                distance_m: None,
                note: format!("built up to {reps} @ {load} kg"),
                missed: false,
            }
        }
        // Loaded carry assessment: carry the given start for as long as form holds.
        Ask::LoadedCarry { start_kg: start } => {
            let secs =
                ((truth.carry.1 as f64 * truth.carry.0 / start).floor() as i32).clamp(5, 120);
            Performed {
                reps: None,
                load_kg: Some(start),
                hold_s: Some(secs),
                distance_m: None,
                note: format!("carried {start} kg for {secs}s"),
                missed: false,
            }
        }
        // A distance carry: the asked metres at the given weight, capacity
        // scaling with how far the weight is from the true one — the metre twin
        // of the timed carry above.
        Ask::WeightedDistance {
            load_kg: asked_load,
            distance_m: ask,
        } => {
            let load = behaviour.load_used(asked_load, loads);
            let cap = ((truth.carry.1 as f64 * truth.carry.0 / load / 3.0).floor() as i32).max(5);
            let did = behaviour.hold_target(ask).min(cap);
            Performed {
                reps: None,
                load_kg: Some(load),
                hold_s: None,
                distance_m: Some(did),
                note: format!(
                    "asked {ask} m @ {load} kg, did {did} m{}",
                    swapped(asked_load, load)
                ),
                missed: did < ask,
            }
        }
        Ask::LoadedDistance { start_kg: start } => {
            let metres =
                ((truth.carry.1 as f64 * truth.carry.0 / start / 3.0).floor() as i32).clamp(5, 60);
            Performed {
                reps: None,
                load_kg: Some(start),
                hold_s: None,
                distance_m: Some(metres),
                note: format!("carried {start} kg for {metres} m"),
                missed: false,
            }
        }
        Ask::MaxHold => Performed {
            reps: None,
            load_kg: None,
            hold_s: Some(truth.hold_s),
            distance_m: None,
            note: format!("max hold {}s", truth.hold_s),
            missed: false,
        },
        Ask::Amrap => Performed {
            reps: Some(truth.reps),
            load_kg: None,
            hold_s: None,
            distance_m: None,
            note: format!("AMRAP {}", truth.reps),
            missed: false,
        },
    }
}

// ---- the walk ----------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("DATABASE_URL").context(
        "set DATABASE_URL to a dev DB seeded with a prod dump (see scripts/simulate.sh)",
    )?;
    let user = std::env::var("SIM_USER").unwrap_or_else(|_| "pippijn".into());
    let weeks: i64 = std::env::var("SIM_WEEKS")
        .ok()
        .map(|w| w.parse())
        .transpose()
        .context("SIM_WEEKS must be a number")?
        .unwrap_or(8);
    let temperament = {
        let raw = std::env::var("SIM_ATHLETE").unwrap_or_else(|_| "improver".into());
        match Temperament::parse(&raw) {
            Some(t) => t,
            None => bail!("SIM_ATHLETE must be {}, got {raw:?}", Temperament::names()),
        }
    };
    let sleep = {
        let raw = std::env::var("SIM_RECOVERY").unwrap_or_else(|_| "untracked".into());
        match Sleep::parse(&raw) {
            Some(s) => s,
            None => bail!("SIM_RECOVERY must be {}, got {raw:?}", Sleep::names()),
        }
    };
    let behaviour = {
        let raw = std::env::var("SIM_BEHAVIOUR").unwrap_or_else(|_| "compliant".into());
        match Behaviour::parse(&raw) {
            Some(b) => b,
            None => bail!("SIM_BEHAVIOUR must be {}, got {raw:?}", Behaviour::names()),
        }
    };

    let pool = coach::db::connect(&url).await?;
    let catalog_dir = std::env::var("CATALOG_DIR").unwrap_or_else(|_| "data/catalog".into());
    coach::seed::run(&pool, &catalog_dir).await?;

    let locations = location_repo::list(&pool, &user).await?;
    let wanted = std::env::var("SIM_LOCATION").ok();
    let location = match &wanted {
        Some(name) => locations
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
            .with_context(|| format!("no location named {name:?}"))?,
        None => locations
            .iter()
            .find(|l| l.is_default)
            .context("the user has no default location")?,
    };
    let ctx = service::context(&pool, &user, Some(location.id)).await?;

    // Real history, in local time — the soil the simulation grows from.
    let floor = NaiveDate::from_ymd_opt(2000, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let raw = workout_repo::list_since(&pool, &user, floor).await?;
    let to_local = |utc: NaiveDateTime| {
        Utc.from_utc_datetime(&utc)
            .with_timezone(&ctx.tz)
            .naive_local()
    };
    let mut hist: Vec<SetRec> = raw
        .iter()
        .map(|w| SetRec {
            id: SetId(w.id),
            exercise_id: ExerciseId(w.exercise_id),
            logged_at: to_local(w.logged_at),
            reps: w.reps,
            load_kg: w.load_kg,
            hold_s: w.hold_s,
            distance_m: w.distance_m,
            rpe: w.rpe,
        })
        .collect();
    hist.sort_by_key(|s| s.logged_at);
    if hist.is_empty() {
        bail!("no history for user {user} — nothing to grow the simulation from");
    }

    let metric_of: HashMap<ExerciseId, Metric> =
        ctx.exercises.iter().map(|e| (e.id, e.metric)).collect();
    let name_of: HashMap<ExerciseId, String> = ctx
        .exercises
        .iter()
        .map(|e| (e.id, e.name.clone()))
        .collect();

    let sim_start = hist.last().unwrap().logged_at.date() + Duration::days(1);
    let sim_start_dt = sim_start.and_hms_opt(SESSION_HOUR, 0, 0).unwrap();

    // The athlete's true ability opens at what the history says they can do
    // today — the engine and the athlete agree at t0, then the temperament
    // takes over.
    let opening = ability::abilities(&hist, sim_start_dt);

    // The injury lands on one muscle group, and hits every movement that group
    // is the prime mover for. Shoulders by preference — a tweaked shoulder is
    // the injury that quietly poisons the most of a session — falling back to
    // whichever group the catalog leads with so the case still runs elsewhere.
    let injured_group = ctx
        .groups
        .iter()
        .find(|g| g.name.eq_ignore_ascii_case(INJURED_GROUP))
        .or_else(|| ctx.groups.first());
    let injured: BTreeSet<ExerciseId> = match (temperament, injured_group) {
        (Temperament::Injured, Some(g)) => ctx
            .exercises
            .iter()
            .filter(|e| {
                e.groups
                    .iter()
                    .any(|(gid, role)| *gid == g.id && *role == MuscleRole::Primary)
            })
            .map(|e| e.id)
            .collect(),
        _ => BTreeSet::new(),
    };
    let mut athlete = Athlete {
        temperament,
        base: HashMap::new(),
        injured,
        idle_days: 0,
        detrained: 0.0,
    };

    println!(
        "# coach simulation — user {user}, {temperament:?} athlete, {behaviour:?} behaviour, \
         {sleep:?} recovery, {weeks} weeks from {sim_start}"
    );
    if temperament == Temperament::Injured {
        println!(
            "# injury: {} goes at week {INJURY_WEEK} ({} movements at {:.0}% for good)",
            injured_group.map(|g| g.name.as_str()).unwrap_or("?"),
            athlete.injured.len(),
            INJURY_MULT * 100.0
        );
    }
    println!(
        "# at location {:?}; real history: {} sets through {}",
        location.name,
        hist.len(),
        hist.last().unwrap().logged_at.date()
    );
    println!("# each Active day: the committed plan, and what the athlete actually did\n");

    let mut sessions = 0usize;
    let mut sets_logged = 0usize;
    let mut misses = 0usize;
    let mut assess_cards = 0usize;
    // Days the coach planned a session and the athlete didn't turn up, and cards
    // offered but never performed because the athlete left early.
    let mut away = 0usize;
    let mut abandoned = 0usize;
    let mut touched: BTreeSet<ExerciseId> = BTreeSet::new();
    // Readiness as it stood each morning, accumulated as the walk grows — exactly
    // what the ledger reconstructs the ask under, so an eased session isn't judged
    // as though it had been full-effort.
    let mut readiness_history: BTreeMap<NaiveDate, Readiness> = BTreeMap::new();
    let mut offers: BTreeMap<ExerciseId, Vec<NaiveDate>> = BTreeMap::new();

    for d in 0..weeks * 7 {
        let date = sim_start + Duration::days(d);
        let week = d / 7;
        let now = date.and_hms_opt(SESSION_HOUR, 0, 0).unwrap();
        let today_readiness = sleep.readiness_on(d);
        if let Some(r) = today_readiness {
            readiness_history.insert(date, r);
        }
        let ready_tag = today_readiness
            .map(|r| format!(" [readiness {:?} {:.2}]", r.band(), r.score()))
            .unwrap_or_default();
        let sets_at_dawn = sets_logged;
        let last_set_at = hist.last().map(|s| s.logged_at);
        let inp = service::input_from(
            &ctx,
            hist.clone(),
            last_set_at,
            today_readiness,
            readiness_history.clone(),
            offers.clone(),
        );
        let verdict = engine::evaluate(&inp, now);

        // Keep the offer ledger the prod service keeps, so the R6-4 loop is
        // actually exercised: an athlete who leaves early has to be able to grow
        // a history of cards he was shown and did not take. Warm-ups excluded,
        // matching `service::now`.
        for s in verdict
            .plan
            .iter()
            .filter(|s| s.kind != SuggestionKind::Warmup)
        {
            let days = offers.entry(s.exercise_id).or_default();
            if !days.contains(&date) {
                days.push(date);
            }
        }

        // A labelled block rather than `continue`, so that a rest day or a no-show
        // skips the *session* without also skipping the end-of-week report below.
        // What the engine believes is true of the week whether or not the athlete
        // turned up — and with `continue` here, an athlete whose skip pattern
        // happened to land on the reporting day produced a trace with no accuracy
        // rows at all, which is how both `skipper` cells went unmeasured.
        'session: {
            let train = verdict.state == PacingState::Active
                && verdict
                    .plan
                    .iter()
                    .any(|s| s.kind != SuggestionKind::Warmup);
            if !train {
                println!("{date}  w{week}  {:?} — rest{ready_tag}", verdict.state);
                break 'session;
            }
            if !behaviour.attends(d) {
                away += 1;
                println!("{date}  w{week}  Active — but the athlete didn't come in{ready_tag}");
                break 'session;
            }

            sessions += 1;
            let work: Vec<&Suggestion> = verdict
                .plan
                .iter()
                .filter(|s| s.kind != SuggestionKind::Warmup)
                .collect();
            let doing = behaviour.cards_done(work.len());
            println!(
                "{date}  w{week}  Active — training ({} warm-up items, {} work/assess{}){ready_tag}",
                verdict.plan.len() - work.len(),
                work.len(),
                if doing < work.len() {
                    format!(", leaving after {doing}")
                } else {
                    String::new()
                }
            );

            let mut t = date.and_hms_opt(SESSION_HOUR, 10, 0).unwrap();
            for s in work.iter().take(doing) {
                if s.kind == SuggestionKind::Assess {
                    assess_cards += 1;
                }
                touched.insert(s.exercise_id);
                let truth = athlete.truth(s.exercise_id, opening.get(&s.exercise_id), week);
                let p = perform(s, truth, inp.exercise_loads.get(&s.exercise_id), behaviour);
                for _ in 0..s.sets {
                    hist.push(SetRec {
                        // Simulated sets are never written back, so a real row id
                        // would be a fiction; they only need to not collide.
                        id: SetId(-(sets_logged as i64 + 1)),
                        exercise_id: s.exercise_id,
                        logged_at: t,
                        reps: p.reps,
                        load_kg: p.load_kg,
                        hold_s: p.hold_s,
                        distance_m: p.distance_m,
                        rpe: None,
                    });
                    t += Duration::minutes(SET_GAP_MIN);
                    sets_logged += 1;
                }
                if p.missed {
                    misses += 1;
                }
                let name = name_of
                    .get(&s.exercise_id)
                    .cloned()
                    .unwrap_or_else(|| s.exercise_name.clone());
                println!(
                    "    {:<7} {} ({})  {} set(s): {}{}",
                    format!("{:?}", s.kind),
                    name,
                    s.group,
                    s.sets,
                    p.note,
                    if p.missed { "  MISS" } else { "" }
                );
            }
            for s in work.iter().skip(doing) {
                abandoned += 1;
                println!(
                    "    {:<7} {} ({})  {} set(s): not done — athlete left",
                    format!("{:?}", s.kind),
                    name_of
                        .get(&s.exercise_id)
                        .cloned()
                        .unwrap_or_else(|| s.exercise_name.clone()),
                    s.group,
                    s.sets
                );
            }
            for n in &verdict.notices {
                println!("    (note) {n}");
            }
        }

        // The body spends the day whatever the coach and the athlete decided.
        // Measured by sets reaching the log rather than by attendance: a rest
        // day, a day the athlete stayed away and a day they turned up and left
        // before the first card are the same day to a muscle.
        let trained_today = sets_logged > sets_at_dawn;
        athlete.spend_day(trained_today);
        if athlete.detrained > 0.0 && (trained_today || athlete.idle_days % 7 == 0) {
            println!(
                "{date}  w{week}  (body) {:.0}% of trained strength, {} day(s) idle",
                (1.0 - athlete.detrained) * 100.0,
                athlete.idle_days
            );
        }

        // End of a sim week: how far apart are the engine's belief and the truth?
        if d % 7 == 6 {
            let eow = date.and_hms_opt(23, 0, 0).unwrap();
            let est = ability::abilities(&hist, eow);
            let res = residual::residuals(&hist, ctx.mode, &readiness_history, &inp.exercise_loads);
            let mut rows: BTreeMap<String, String> = BTreeMap::new();
            for id in &touched {
                let name = name_of.get(id).cloned().unwrap_or_else(|| id.to_string());
                let a = est.get(id);
                let truth = athlete.truth(*id, opening.get(id), week);
                let conf = a.map(|a| format!("{:?}", a.confidence)).unwrap_or_default();
                let miss_streak = res.get(id).map(|r| r.consecutive_misses).unwrap_or(0);
                let belief = match metric_of.get(id) {
                    Some(Metric::WeightedReps) => format!(
                        "e1rm {:.1} (true {:.1})",
                        a.and_then(|a| a.e1rm).unwrap_or(0.0),
                        truth.e1rm
                    ),
                    Some(Metric::Reps) => format!(
                        "reps {} (true {})",
                        a.and_then(|a| a.best_reps).unwrap_or(0),
                        truth.reps
                    ),
                    Some(Metric::Hold) => format!(
                        "hold {}s (true {}s)",
                        a.and_then(|a| a.best_hold).unwrap_or(0),
                        truth.hold_s
                    ),
                    Some(Metric::WeightedHold) => format!(
                        "carry {:.1} kg x {}s (true {:.1} x {}s)",
                        a.and_then(|a| a.carry).map(|c| c.load).unwrap_or(0.0),
                        a.and_then(|a| a.carry).map(|c| c.secs).unwrap_or(0),
                        truth.carry.0,
                        truth.carry.1
                    ),
                    Some(Metric::WeightedDistance) => format!(
                        "carry {:.1} kg x {} m (true {:.1})",
                        a.and_then(|a| a.carry_m).map(|c| c.load).unwrap_or(0.0),
                        a.and_then(|a| a.carry_m).map(|c| c.metres).unwrap_or(0),
                        truth.carry.0
                    ),
                    None => String::new(),
                };
                rows.insert(
                    name.clone(),
                    format!("{belief}  [{conf}]  miss-streak {miss_streak}"),
                );
            }
            println!("  -- end of week {week}:");
            for (name, row) in rows {
                println!("     {name}: {row}");
            }
        }
        println!();
    }

    println!(
        "# summary: {sessions} sessions, {sets_logged} sets, {misses} missed cards, \
         {assess_cards} assess cards, {away} days away, {abandoned} cards abandoned; \
         {} distinct exercises trained",
        touched.len()
    );
    Ok(())
}
