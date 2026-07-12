//! Property tests: invariants the pacing engine must hold for *every* input, not
//! just the hand-picked examples in `pacing_engine.rs`. proptest generates
//! thousands of random-but-bounded scenarios (arbitrary history, modes, owned
//! weights) and checks each verdict never violates a guarantee — determinism,
//! loads you actually own, sane rep ranges, a budgeted plan, and never a panic.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use std::collections::HashMap;

use coach::muscle::types::{MuscleRole, Region};
use coach::pacing::engine::evaluate;
use coach::pacing::types::{
    ExerciseInfo, GroupMeta, Kit, PacingInput, PacingSettings, SetRec, SuggestionKind,
};
use coach::settings::types::Mode;
use proptest::prelude::*;

fn base() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 7, 6)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
}

// A fixed catalog spanning the metrics + equipment the engine branches on:
//   5 barbell row   weighted, equip [3] (owned weights), back
//   2 ring row      bodyweight reps,     back
//   7 front lever   hold (skill),        chest
//   8 hanging raise bodyweight reps,     legs (core pattern)
const EQUIP_LOADED: i64 = 3;
fn catalog() -> Vec<ExerciseInfo> {
    use coach::exercise::types::{Metric, Pattern};
    let mk = |id, pat, metric, skill, equip: Vec<i64>, group| ExerciseInfo {
        id,
        name: format!("ex{id}"),
        pattern: pat,
        metric,
        is_skill: skill,
        warmup: false,
        equipment: equip,
        groups: vec![(group, MuscleRole::Primary)],
    };
    vec![
        mk(
            5,
            Pattern::Pull,
            Metric::WeightedReps,
            false,
            vec![EQUIP_LOADED],
            20,
        ),
        mk(2, Pattern::Pull, Metric::Reps, true, vec![], 20),
        mk(7, Pattern::Push, Metric::Hold, true, vec![], 10),
        mk(8, Pattern::Core, Metric::Reps, false, vec![], 30),
    ]
}
const EX_IDS: [i64; 4] = [5, 2, 7, 8];

fn groups() -> Vec<GroupMeta> {
    vec![
        GroupMeta {
            id: 10,
            name: "Chest".into(),
            region: Region::Chest,
        },
        GroupMeta {
            id: 20,
            name: "Back".into(),
            region: Region::Back,
        },
        GroupMeta {
            id: 30,
            name: "Legs".into(),
            region: Region::Legs,
        },
    ]
}

fn mode_of(i: usize) -> Mode {
    [
        Mode::Balanced,
        Mode::Strength,
        Mode::Skills,
        Mode::Conditioning,
    ][i]
}

// One logged set: (which catalog exercise, how many days ago, load, reps).
type RawSet = (usize, i64, f64, i32);

fn build_input(mode_i: usize, days_per_week: i32, raw: &[RawSet], owned: &[f64]) -> PacingInput {
    let history: Vec<SetRec> = raw
        .iter()
        .map(|&(ex_i, days_ago, load, reps)| {
            let id = EX_IDS[ex_i];
            // Match the field shape to the exercise's metric so the set is sane.
            let (load_kg, reps_v, hold_s) = match id {
                5 => (Some(load), Some(reps), None),      // weighted
                7 => (None, None, Some(reps.max(1) * 3)), // hold (seconds)
                _ => (None, Some(reps), None),            // bodyweight reps
            };
            SetRec {
                exercise_id: id,
                logged_at: base() - Duration::days(days_ago),
                reps: reps_v,
                load_kg,
                hold_s,
                rpe: None,
            }
        })
        .collect();
    let last_set_at = history.iter().map(|s| s.logged_at).max();
    let equipment_loads = if owned.is_empty() {
        HashMap::new()
    } else {
        HashMap::from([(EQUIP_LOADED, owned.to_vec())])
    };
    PacingInput {
        mode: mode_of(mode_i),
        days_per_week,
        emphasis: None,
        exercises: catalog(),
        history,
        last_set_at,
        settings: PacingSettings {
            window_start_hour: 8,
            window_end_hour: 21,
            min_rest_min: 20,
        },
        groups: groups(),
        kit: Some(Kit(catalog()
            .iter()
            .flat_map(|e| e.equipment.clone())
            .collect())),
        equipment_loads,
        equipment_names: HashMap::new(),
        readiness: None,
    }
}

// A sorted, deduped, ascending weight ladder (possibly empty).
fn owned_strategy() -> impl Strategy<Value = Vec<f64>> {
    prop::collection::vec(1u32..40, 0..6).prop_map(|v| {
        let mut w: Vec<f64> = v.into_iter().map(|x| (x as f64) * 2.5).collect();
        w.sort_by(f64::total_cmp);
        w.dedup();
        w
    })
}

fn scenario() -> impl Strategy<Value = (usize, i32, Vec<RawSet>, Vec<f64>)> {
    (
        0usize..4,
        1i32..8,
        prop::collection::vec((0usize..4, 0i64..300, 2.5f64..120.0, 1i32..20), 0..40),
        owned_strategy(),
    )
}

proptest! {
    // Same input → byte-identical verdict. Guards against HashMap iteration order
    // (or any hidden nondeterminism) leaking into the plan/balance ordering.
    #[test]
    fn evaluate_is_deterministic((m, d, raw, owned) in scenario()) {
        let a = evaluate(&build_input(m, d, &raw, &owned), base());
        let b = evaluate(&build_input(m, d, &raw, &owned), base());
        prop_assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }

    // Every prescribed load is a weight you actually own (the loaded lift's kit
    // has an owned ladder here, so nothing off-ladder may be suggested).
    #[test]
    fn loads_are_always_owned((m, d, raw, owned) in scenario()) {
        prop_assume!(!owned.is_empty());
        let out = evaluate(&build_input(m, d, &raw, &owned), base());
        for item in &out.plan {
            if let Some(load) = item.load_kg {
                // Only the loaded lift (id 5) carries a load; it uses EQUIP_LOADED.
                prop_assert!(
                    owned.iter().any(|w| (w - load).abs() < 1e-6),
                    "prescribed {load} not in owned {owned:?} (item {:?})",
                    item.exercise_id
                );
            }
        }
    }

    // Rep targets are sane: low ≤ high, both positive, within a plausible ceiling.
    #[test]
    fn rep_targets_are_sane((m, d, raw, owned) in scenario()) {
        let out = evaluate(&build_input(m, d, &raw, &owned), base());
        for item in &out.plan {
            if let (Some(lo), Some(hi)) = (item.rep_low, item.rep_high) {
                prop_assert!(lo >= 1 && lo <= hi && hi <= 25, "reps {lo}..{hi}");
            }
            prop_assert!(item.sets >= 1, "sets {}", item.sets);
        }
    }

    // The training plan is budgeted: work sets (deficit-sized, min 2 each) never
    // exceed the day target — and now *exactly* so. The cover takes one set per
    // step and at most `budget` steps, so work can't spill at all. (The old
    // deficit-share sizing could overrun by a trailing item's fixed count; that
    // slack is gone along with the heuristic that needed it.)
    #[test]
    fn work_volume_stays_within_the_day_budget((m, d, raw, owned) in scenario()) {
        let out = evaluate(&build_input(m, d, &raw, &owned), base());
        let planned: i32 = out
            .plan
            .iter()
            .filter(|s| s.kind != SuggestionKind::Warmup)
            .map(|s| s.sets)
            .sum();
        prop_assert!(
            planned <= out.day_target_sets,
            "planned {planned} > budget {}",
            out.day_target_sets
        );
    }

    // No exercise is ever planned twice. The old group-loop emitted one item per
    // muscle group, so a movement covering two in-deficit groups (dips → chest AND
    // triceps) appeared twice and read as a stutter. The cover accumulates *by
    // exercise*, so "2 × dips" is a single item with a count — a duplicate is
    // unrepresentable. This holds for every history, not just the ones we thought of.
    #[test]
    fn an_exercise_is_never_planned_twice((m, d, raw, owned) in scenario()) {
        let out = evaluate(&build_input(m, d, &raw, &owned), base());
        let mut seen = std::collections::HashSet::new();
        for item in out.plan.iter().filter(|s| s.kind != SuggestionKind::Warmup) {
            prop_assert!(
                seen.insert(item.exercise_id),
                "exercise {} planned twice in {:?}",
                item.exercise_id,
                out.plan.iter().map(|s| (s.exercise_id, s.sets)).collect::<Vec<_>>()
            );
        }
    }

    // A weighted lift is never planned without a weight. Either its inventory is
    // known — and the load is one the athlete owns — or the lift isn't selectable
    // at all. There is no third state that hands someone a barbell movement and
    // leaves them to guess: `Dose::Weighted` carries a `load: f64`, not an Option.
    #[test]
    fn a_weighted_lift_is_never_planned_without_a_load((m, d, raw, owned) in scenario()) {
        let out = evaluate(&build_input(m, d, &raw, &owned), base());
        for item in out.plan.iter().filter(|s| s.kind != SuggestionKind::Warmup) {
            if item.exercise_id == 5 {
                prop_assert!(
                    item.load_kg.is_some(),
                    "the loaded lift was planned with no load (owned {owned:?})"
                );
                prop_assert!(
                    !owned.is_empty(),
                    "the loaded lift was planned from an empty inventory"
                );
            }
        }
    }

    // A warm-up never credits training volume: its group balances are unaffected
    // by warm-up-tagged sets. (Here no catalog move is warmup-tagged, so the plan
    // carries no mobility item; this asserts the block only ever *prepends*, never
    // displaces the work — the first non-warmup item is always present when work
    // exists.)
    #[test]
    fn a_nonempty_plan_has_a_training_item((m, d, raw, owned) in scenario()) {
        let out = evaluate(&build_input(m, d, &raw, &owned), base());
        if !out.plan.is_empty() {
            prop_assert!(
                out.plan.iter().any(|s| s.kind != SuggestionKind::Warmup),
                "a plan with only warm-ups is never emitted"
            );
        }
    }
}
