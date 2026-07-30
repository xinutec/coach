//! Property tests: invariants the pacing engine must hold for *every* input, not
//! just the hand-picked examples in `pacing_engine.rs`. proptest generates
//! thousands of random-but-bounded scenarios (arbitrary history, modes, owned
//! weights) and checks each verdict never violates a guarantee — determinism,
//! loads you actually own, sane rep ranges, a budgeted plan, and never a panic.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use std::collections::BTreeMap;

use coach::muscle::types::{MuscleRole, Region};
use coach::pacing::engine::evaluate;
use coach::pacing::types::{
    ExerciseInfo, GroupMeta, Kit, PacingInput, PacingSettings, Readiness, SetRec, SuggestionKind,
};
use coach::settings::types::Mode;
use coach_pacing::domain::{EquipmentId, ExerciseId, GroupId, SetId};
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
    let mk = |id: i64, pat, metric, skill, equip: Vec<i64>, group: i64| ExerciseInfo {
        id: ExerciseId(id),
        name: format!("ex{id}"),
        family: format!("ex{id}"),
        difficulty: None,
        pattern: pat,
        metric,
        is_skill: skill,
        is_power: false,
        warmup: false,
        equipment: equip.into_iter().map(EquipmentId).collect(),
        groups: vec![(GroupId(group), MuscleRole::Primary)],
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
            id: GroupId(10),
            name: "Chest".into(),
            region: Region::Chest,
        },
        GroupMeta {
            id: GroupId(20),
            name: "Back".into(),
            region: Region::Back,
        },
        GroupMeta {
            id: GroupId(30),
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
                id: SetId(0),
                exercise_id: ExerciseId(id),
                logged_at: base() - Duration::days(days_ago),
                reps: reps_v,
                load_kg,
                hold_s,
                distance_m: None,
                rpe: None,
            }
        })
        .collect();
    let last_set_at = history.iter().map(|s| s.logged_at).max();
    // Buildable loads for the one weighted lift (exercise id 5). Empty inventory =
    // not loadable, so the lift simply isn't selectable.
    let exercise_loads = if owned.is_empty() {
        BTreeMap::new()
    } else {
        BTreeMap::from([(ExerciseId(5), owned.to_vec())])
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
        exercise_loads,
        equipment_names: BTreeMap::new(),
        notices: Vec::new(),
        readiness: None,
        readiness_history: Default::default(),
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

/// The same scenario judged at a given readiness.
///
/// This used to build `Readiness { score, band }` with the thresholds copied
/// inline, which let the property generate a band that didn't follow its score —
/// a day the engine cannot produce, so any failure it found was unreachable.
fn with_readiness(mut input: PacingInput, score: f64) -> PacingInput {
    input.readiness = Some(Readiness::of(score));
    input
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
    // Same input → byte-identical verdict. Guards against BTreeMap iteration order
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
            if let Some(load) = item.ask.load_kg() {
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
            if let (Some(lo), Some(hi)) = (item.ask.rep_low(), item.ask.rep_high()) {
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
        let mut seen = std::collections::BTreeSet::new();
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
            if item.exercise_id == ExerciseId(5) {
                prop_assert!(
                    item.ask.load_kg().is_some(),
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

    // Permutation-invariance: the verdict must not depend on the order the
    // exercises and groups happen to arrive in. Selection and cover tie-breaks are
    // meant to be keyed on ids, not positions — so reversing both input lists is a
    // no-op. A verdict that changes means a silent order-dependence (e.g. `max_by`
    // keeping the *last* of equally-ranked candidates), where the plan the athlete
    // gets would hinge on the incidental order a repo query returned rows in.
    #[test]
    fn evaluate_is_order_independent_over_catalog_and_groups((m, d, raw, owned) in scenario()) {
        let canon = evaluate(&build_input(m, d, &raw, &owned), base());
        let mut reordered = build_input(m, d, &raw, &owned);
        reordered.exercises.reverse();
        reordered.groups.reverse();
        let flipped = evaluate(&reordered, base());
        prop_assert_eq!(
            serde_json::to_string(&canon).unwrap(),
            serde_json::to_string(&flipped).unwrap(),
            "the verdict changed when the catalog/groups were reordered"
        );
    }

    // A wiped-out day never asks for a heavier load than a fresh one. Readiness
    // gates progression on a threshold (`readiness_advances`, 0.40), and holding
    // progression leaves a larger rep reserve — which can only make the ask
    // lighter. Same history, same kit, so the load is the only thing that moved.
    //
    // Only exercises BOTH days planned are compared: a low-readiness day is
    // entitled to pick a different session entirely, and that is not a regression.
    // Volume is deliberately not asserted — the day target scales with readiness
    // but is clamped at both ends, and the cover may stop early, so the two plans'
    // set counts are not ordered by anything the engine promises.
    #[test]
    fn a_tired_day_never_asks_for_more_load_than_a_fresh_one((m, d, raw, owned) in scenario()) {
        let tired = evaluate(&with_readiness(build_input(m, d, &raw, &owned), 0.10), base());
        let fresh = evaluate(&with_readiness(build_input(m, d, &raw, &owned), 0.95), base());

        for t in &tired.plan {
            if t.kind != SuggestionKind::Work {
                continue;
            }
            let Some(f) = fresh.plan.iter().find(|f| {
                f.exercise_id == t.exercise_id && f.kind == SuggestionKind::Work
            }) else {
                continue;
            };
            if let (Some(tired_load), Some(fresh_load)) = (t.ask.load_kg(), f.ask.load_kg()) {
                prop_assert!(
                    tired_load <= fresh_load + 1e-9,
                    "exercise {}: tired day asked {tired_load} kg, fresh day {fresh_load} kg",
                    t.exercise_id
                );
            }
        }
    }

    // Doing the session doesn't rewrite it. Logging exactly what the plan's first
    // work item asked for changes the verdict — the day's done count, the balance
    // view, the coach's line — but it must not change *which* movements the
    // session is made of, or their order. A plan that reshuffles itself as you
    // execute it isn't a plan: you'd finish the first movement and be handed a
    // different session for the rest of the hour.
    //
    // Measured before it was written: over ~290 generated scenarios the verdict
    // moved every time and the work sequence never did, so this pins observed
    // behaviour rather than a hoped-for one.
    #[test]
    fn committing_the_first_item_does_not_rewrite_the_session((m, d, raw, owned) in scenario()) {
        let before = evaluate(&build_input(m, d, &raw, &owned), base());
        let Some(first) = before.plan.iter().find(|i| i.kind == SuggestionKind::Work) else {
            return Ok(());
        };

        // Log the prescribed sets, as prescribed, right now.
        let mut committed = build_input(m, d, &raw, &owned);
        for _ in 0..first.sets.max(1) {
            committed.history.push(SetRec {
                id: SetId(0),
                exercise_id: first.exercise_id,
                logged_at: base(),
                reps: first.ask.rep_low(),
                load_kg: first.ask.load_kg(),
                hold_s: first.ask.hold_s(),
                distance_m: None,
                rpe: None,
            });
        }
        committed.last_set_at = Some(base());
        let after = evaluate(&committed, base());

        let work_ids = |out: &coach::pacing::types::PacingNow| -> Vec<ExerciseId> {
            out.plan
                .iter()
                .filter(|i| i.kind == SuggestionKind::Work)
                .map(|i| i.exercise_id)
                .collect()
        };
        prop_assert_eq!(
            work_ids(&before),
            work_ids(&after),
            "committing exercise {} rewrote the rest of the session",
            first.exercise_id
        );
    }
}
