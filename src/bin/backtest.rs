//! E1 — back-test the pacing engine against real logged history.
//!
//! Replays `engine::evaluate` walk-forward over the user's actual sets in a
//! **dev** DB (seeded from a prod dump — see `scripts/backtest.sh`): for each
//! training day it prints the verdict the coach *would* have given that morning,
//! knowing only the prior days. The output is deterministic, so diffing two runs
//! (before/after an engine change) shows exactly what the change did to real
//! prescriptions — the regression signal an engine of pure functions makes free.
//!
//! It reads `DATABASE_URL` and never touches prod. The real training data lives
//! only in the gitignored dev DB; nothing private is committed.
//!
//! Usage:
//!   DATABASE_URL=mysql://coach:coach@127.0.0.1:3308/coach cargo run --bin backtest
//!   BACKTEST_USER overrides the user id (default: the single imported user).

use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};

use coach::pacing::ability::{self, Confidence};
use coach::pacing::types::SetRec;
use coach::pacing::{engine, service};
use coach::workout::repo as workout_repo;

/// The morning we evaluate each training day at (local). Inside a default
/// training window, and before the imported sets (stamped midday), so a day's
/// own sets aren't counted as already-done when we ask "what's the plan today?".
const MORNING_HOUR: u32 = 6;

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("DATABASE_URL").context(
        "set DATABASE_URL to a dev DB seeded with a prod dump (see scripts/backtest.sh)",
    )?;
    let user = std::env::var("BACKTEST_USER").unwrap_or_else(|_| "pippijn".into());

    let pool = coach::db::connect(&url).await?;
    // Reconcile the *committed* catalog into the dev DB first (hash-gated, same as
    // boot), so the back-test always reflects the current catalog — flags,
    // difficulty, and the muscle model — against the prod-dump history, not
    // whatever catalog the dump happened to carry.
    let catalog_dir = std::env::var("CATALOG_DIR").unwrap_or_else(|_| "data/catalog".into());
    coach::seed::run(&pool, &catalog_dir).await?;

    // Same assembly as the live verdict, location-agnostic ("Anywhere") so the
    // back-test isn't tied to one gym's inventory.
    let ctx = service::context(&pool, &user, None, None).await?;

    // All of the user's history (a floor in the distant past = everything).
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
    let mut sets: Vec<SetRec> = raw
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
    sets.sort_by_key(|s| s.logged_at);

    if sets.is_empty() {
        println!("# no history for user {user} — nothing to back-test");
        return Ok(());
    }

    let name_of: HashMap<i64, String> = ctx
        .exercises
        .iter()
        .map(|e| (e.id, e.name.clone()))
        .collect();
    let days: BTreeSet<NaiveDate> = sets.iter().map(|s| s.logged_at.date()).collect();

    println!(
        "# coach back-test — user {user}: {} sets over {} training days ({} .. {})",
        sets.len(),
        days.len(),
        days.iter().next().unwrap(),
        days.iter().last().unwrap()
    );
    println!("# each block: the morning verdict for that day, given only prior days\n");

    // Roll-ups across the walk, to show the "gets finer over time" trajectory.
    let mut total_assess = 0usize;
    let mut total_work = 0usize;
    let mut ever_high: BTreeSet<i64> = BTreeSet::new();

    for day in &days {
        let now_local = day.and_hms_opt(MORNING_HOUR, 0, 0).unwrap();
        // Only prior days are known when we plan this morning.
        let hist: Vec<SetRec> = sets
            .iter()
            .filter(|s| s.logged_at.date() < *day)
            .cloned()
            .collect();
        let last_set_at = hist.iter().map(|s| s.logged_at).max();
        let inp = service::input_from(&ctx, hist.clone(), last_set_at, None);
        let verdict = engine::evaluate(&inp, now_local);

        // Confidence the estimate carries into this morning's plan.
        let abil = ability::abilities(&hist, now_local);
        for (id, a) in &abil {
            if a.confidence == Confidence::High {
                ever_high.insert(*id);
            }
        }

        println!(
            "{day}  state={:?}  known_sets={}  plan={} item(s)",
            verdict.state,
            hist.len(),
            verdict.plan.len()
        );
        for s in &verdict.plan {
            match s.kind {
                coach::pacing::types::SuggestionKind::Assess => total_assess += 1,
                coach::pacing::types::SuggestionKind::Work => total_work += 1,
                coach::pacing::types::SuggestionKind::Warmup => {}
            }
            let name = name_of
                .get(&s.exercise_id)
                .cloned()
                .unwrap_or_else(|| s.exercise_name.clone());
            let reps = match (s.rep_low, s.rep_high) {
                (Some(a), Some(b)) if a != b => format!(" {a}-{b} reps"),
                (Some(a), _) => format!(" {a} reps"),
                _ => String::new(),
            };
            let load = s.load_kg.map(|l| format!(" @ {l} kg")).unwrap_or_default();
            let hold = s.hold_s.map(|h| format!(" {h}s hold")).unwrap_or_default();
            let conf = s
                .explanation
                .map(|e| format!("  [{:?}]", e.confidence))
                .unwrap_or_default();
            println!(
                "    {:<7} {} ({})  {} set(s){}{}{}{}",
                format!("{:?}", s.kind),
                name,
                s.group,
                s.sets,
                reps,
                load,
                hold,
                conf
            );
        }
        println!();
    }

    println!(
        "# summary: {total_work} work + {total_assess} assess prescriptions across the walk; \
         {} exercise(s) reached High confidence by the end",
        ever_high.len()
    );
    Ok(())
}
