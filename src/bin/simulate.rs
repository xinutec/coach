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
//!   SIM_ATHLETE   — improver | plateauer | badweek (default improver)
//!   SIM_USER      — user id (default pippijn)
//!   SIM_LOCATION  — location by name (default: the user's default)

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result, bail};
use chrono::{Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};

use coach::exercise::types::Metric;
use coach::location::repo as location_repo;
use coach::pacing::ability;
use coach::pacing::residual;
use coach::pacing::types::{PacingState, SetRec, Suggestion, SuggestionKind};
use coach::pacing::{engine, service};
use coach::workout::repo as workout_repo;

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

/// Weekly strength gain for an improving athlete (fractional, on e1RM and
/// carry loads) and the rep/hold analogues. Deliberately modest — real novice
/// gains on light kit, not a montage.
const GAIN_PER_WEEK: f64 = 0.015;
const REPS_PER_WEEK: f64 = 0.75;
const HOLD_S_PER_WEEK: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Temperament {
    Improver,
    Plateauer,
    BadWeek,
}

impl Temperament {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "improver" => Some(Self::Improver),
            "plateauer" => Some(Self::Plateauer),
            "badweek" => Some(Self::BadWeek),
            _ => None,
        }
    }

    /// Weeks of progress banked by sim week `w` — the plateauer stops banking
    /// after two.
    fn banked(self, w: i64) -> f64 {
        match self {
            Self::Plateauer => w.min(2) as f64,
            Self::Improver | Self::BadWeek => w as f64,
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
    base: HashMap<i64, Base>,
}

impl Athlete {
    /// True ability at sim week `w`, seeding a deterministic default the first
    /// time an exercise is asked about.
    fn truth(&mut self, exercise_id: i64, seed: Option<&ability::Ability>, w: i64) -> Base {
        let b = self.base.entry(exercise_id).or_insert_with(|| Base {
            e1rm: seed.and_then(|a| a.e1rm).unwrap_or(DEFAULT_E1RM),
            reps: seed.and_then(|a| a.best_reps).unwrap_or(DEFAULT_REPS),
            hold_s: seed.and_then(|a| a.best_hold).unwrap_or(DEFAULT_HOLD_S),
            carry: seed
                .and_then(|a| a.carry)
                .map(|c| (c.load, c.secs))
                .unwrap_or(DEFAULT_CARRY),
        });
        let t = self.temperament;
        Base {
            e1rm: b.e1rm * t.strength(w),
            reps: ((b.reps as f64 + t.reps(w)).round() as i32).max(1),
            hold_s: ((b.hold_s as f64 + t.hold(w)).round() as i32).max(5),
            carry: (
                b.carry.0 * t.strength(w),
                ((b.carry.1 as f64 + t.hold(w) / 2.0).round() as i32).max(5),
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
    note: String,
    missed: bool,
}

/// Do what the card asks, as well as true ability allows — the athlete follows
/// instructions (they stop at the ask even when they could do more) and never
/// reports an RPE.
fn perform(s: &Suggestion, truth: Base, metric: Metric, loads: Option<&Vec<f64>>) -> Performed {
    match s.kind {
        SuggestionKind::Warmup => unreachable!("warm-ups are skipped by the caller"),
        SuggestionKind::Work => match (s.load_kg, s.rep_low, s.hold_s) {
            // Weighted reps: attempt the asked reps at the given load.
            (Some(load), Some(ask), _) => {
                let can = reps_at(truth.e1rm, load).max(1);
                let did = ask.min(can);
                Performed {
                    reps: Some(did),
                    load_kg: Some(load),
                    hold_s: None,
                    note: format!("asked {ask} @ {load} kg, did {did}"),
                    missed: did < ask,
                }
            }
            // Bodyweight reps.
            (None, Some(ask), _) => {
                let did = ask.min(truth.reps).max(1);
                Performed {
                    reps: Some(did),
                    load_kg: None,
                    hold_s: None,
                    note: format!("asked {ask}, did {did}"),
                    missed: did < ask,
                }
            }
            // Loaded carry: the asked seconds at the given weight, capacity
            // scaling with how far the weight is from the true one.
            (Some(load), None, Some(ask)) => {
                let cap = ((truth.carry.1 as f64 * truth.carry.0 / load).floor() as i32).max(5);
                let did = ask.min(cap);
                Performed {
                    reps: None,
                    load_kg: Some(load),
                    hold_s: Some(did),
                    note: format!("asked {ask}s @ {load} kg, did {did}s"),
                    missed: did < ask,
                }
            }
            // Unloaded hold.
            (None, None, Some(ask)) => {
                let did = ask.min(truth.hold_s).max(5);
                Performed {
                    reps: None,
                    load_kg: None,
                    hold_s: Some(did),
                    note: format!("asked {ask}s, did {did}s"),
                    missed: did < ask,
                }
            }
            _ => Performed {
                reps: None,
                load_kg: None,
                hold_s: None,
                note: "unintelligible card".into(),
                missed: false,
            },
        },
        SuggestionKind::Assess => match (s.load_kg, s.rep_low) {
            // Build-up: work up to a hard-but-clean set of the asked reps. The
            // athlete lands on the heaviest owned weight that still leaves ~1
            // rep in reserve at that count.
            (Some(_start), Some(reps)) => {
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
                    note: format!("built up to {reps} @ {load} kg"),
                    missed: false,
                }
            }
            // Loaded carry assessment: carry the given start for as long as
            // form holds.
            (Some(start), None) => {
                let secs =
                    ((truth.carry.1 as f64 * truth.carry.0 / start).floor() as i32).clamp(5, 120);
                Performed {
                    reps: None,
                    load_kg: Some(start),
                    hold_s: Some(secs),
                    note: format!("carried {start} kg for {secs}s"),
                    missed: false,
                }
            }
            // AMRAP / max hold — the metric says which.
            (None, _) => match metric {
                Metric::Hold => Performed {
                    reps: None,
                    load_kg: None,
                    hold_s: Some(truth.hold_s),
                    note: format!("max hold {}s", truth.hold_s),
                    missed: false,
                },
                _ => Performed {
                    reps: Some(truth.reps),
                    load_kg: None,
                    hold_s: None,
                    note: format!("AMRAP {}", truth.reps),
                    missed: false,
                },
            },
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
            None => bail!("SIM_ATHLETE must be improver | plateauer | badweek, got {raw:?}"),
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
            exercise_id: w.exercise_id,
            logged_at: to_local(w.logged_at),
            reps: w.reps,
            load_kg: w.load_kg,
            hold_s: w.hold_s,
            rpe: w.rpe,
        })
        .collect();
    hist.sort_by_key(|s| s.logged_at);
    if hist.is_empty() {
        bail!("no history for user {user} — nothing to grow the simulation from");
    }

    let metric_of: HashMap<i64, Metric> = ctx.exercises.iter().map(|e| (e.id, e.metric)).collect();
    let name_of: HashMap<i64, String> = ctx
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
    let mut athlete = Athlete {
        temperament,
        base: HashMap::new(),
    };

    println!(
        "# coach simulation — user {user}, {temperament:?} athlete, {weeks} weeks from {sim_start}"
    );
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
    let mut touched: BTreeSet<i64> = BTreeSet::new();

    for d in 0..weeks * 7 {
        let date = sim_start + Duration::days(d);
        let week = d / 7;
        let now = date.and_hms_opt(SESSION_HOUR, 0, 0).unwrap();
        let last_set_at = hist.last().map(|s| s.logged_at);
        let inp = service::input_from(&ctx, hist.clone(), last_set_at, None);
        let verdict = engine::evaluate(&inp, now);

        let train = verdict.state == PacingState::Active
            && verdict
                .plan
                .iter()
                .any(|s| s.kind != SuggestionKind::Warmup);
        if !train {
            println!("{date}  w{week}  {:?} — rest", verdict.state);
            continue;
        }

        sessions += 1;
        let warmups = verdict
            .plan
            .iter()
            .filter(|s| s.kind == SuggestionKind::Warmup)
            .count();
        println!(
            "{date}  w{week}  Active — training ({} warm-up items, {} work/assess)",
            warmups,
            verdict.plan.len() - warmups
        );

        let mut t = date.and_hms_opt(SESSION_HOUR, 10, 0).unwrap();
        for s in &verdict.plan {
            if s.kind == SuggestionKind::Warmup {
                continue;
            }
            if s.kind == SuggestionKind::Assess {
                assess_cards += 1;
            }
            touched.insert(s.exercise_id);
            let metric = metric_of
                .get(&s.exercise_id)
                .copied()
                .unwrap_or(Metric::Reps);
            let truth = athlete.truth(s.exercise_id, opening.get(&s.exercise_id), week);
            let p = perform(s, truth, metric, inp.exercise_loads.get(&s.exercise_id));
            for _ in 0..s.sets {
                hist.push(SetRec {
                    exercise_id: s.exercise_id,
                    logged_at: t,
                    reps: p.reps,
                    load_kg: p.load_kg,
                    hold_s: p.hold_s,
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
        for n in &verdict.notices {
            println!("    (note) {n}");
        }

        // End of a sim week: how far apart are the engine's belief and the truth?
        if d % 7 == 6 {
            let eow = date.and_hms_opt(23, 0, 0).unwrap();
            let est = ability::abilities(&hist, eow);
            let res = residual::residuals(&hist);
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
         {assess_cards} assess cards; {} distinct exercises trained",
        touched.len()
    );
    Ok(())
}
