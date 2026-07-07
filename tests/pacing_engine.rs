//! Dynamic-engine tests. Integration tests against the public `evaluate` + its
//! input/output types — the engine is a pure function, exercised through the same
//! surface `service::now` uses.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use std::collections::HashMap;

use coach::exercise::types::{Metric, Pattern};
use coach::muscle::types::{MuscleRole, Region};
use coach::pacing::engine::evaluate;
use coach::pacing::types::{
    Band, ExerciseInfo, GroupMeta, PacingInput, PacingSettings, PacingState, Readiness, SetRec,
    SuggestionKind,
};
use coach::settings::types::Mode;

// Fixed "now": Mon 2026-07-06 12:00 (inside an 08:00–21:00 window).
fn now() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 7, 6)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
}
fn days_ago(d: i64) -> NaiveDateTime {
    now() - Duration::days(d)
}
fn hours_ago(h: i64) -> NaiveDateTime {
    now() - Duration::hours(h)
}

fn settings() -> PacingSettings {
    PacingSettings {
        window_start_hour: 8,
        window_end_hour: 21,
        min_rest_min: 20,
    }
}

// Group ids/meta: 10 Chest(chest), 20 Lats(back), 30 Quads(legs).
fn groups() -> Vec<GroupMeta> {
    vec![
        GroupMeta {
            id: 10,
            name: "Chest".into(),
            region: Region::Chest,
        },
        GroupMeta {
            id: 20,
            name: "Lats".into(),
            region: Region::Back,
        },
        GroupMeta {
            id: 30,
            name: "Quadriceps".into(),
            region: Region::Legs,
        },
    ]
}

#[allow(clippy::too_many_arguments)]
fn ex(
    id: i64,
    name: &str,
    pattern: Pattern,
    metric: Metric,
    is_skill: bool,
    equipment: Vec<i64>,
    grps: Vec<(i64, MuscleRole)>,
) -> ExerciseInfo {
    ExerciseInfo {
        id,
        name: name.into(),
        pattern,
        metric,
        is_skill,
        warmup: false,
        equipment,
        groups: grps,
    }
}

/// A warm-up (mobility) exercise on `group`, doable anywhere.
fn warmup_ex(id: i64, name: &str, group: i64) -> ExerciseInfo {
    ExerciseInfo {
        id,
        name: name.into(),
        pattern: Pattern::Core,
        metric: Metric::Reps,
        is_skill: false,
        warmup: true,
        equipment: vec![],
        groups: vec![(group, MuscleRole::Primary)],
    }
}

/// A bodyweight set (reps only) — for volume/recovery scenarios.
fn set(exercise_id: i64, at: NaiveDateTime) -> SetRec {
    SetRec {
        exercise_id,
        logged_at: at,
        reps: Some(8),
        load_kg: None,
        hold_s: None,
        rpe: None,
    }
}

/// A weighted set (load + reps) — feeds the ability estimate that prescription
/// derives from.
fn wset(exercise_id: i64, at: NaiveDateTime, load: f64, reps: i32) -> SetRec {
    SetRec {
        exercise_id,
        logged_at: at,
        reps: Some(reps),
        load_kg: Some(load),
        hold_s: None,
        rpe: None,
    }
}

fn input(
    mode: Mode,
    exercises: Vec<ExerciseInfo>,
    history: Vec<SetRec>,
    emphasis: Option<Region>,
    available: Option<Vec<i64>>,
) -> PacingInput {
    let last_set_at = history.iter().map(|s| s.logged_at).max();
    PacingInput {
        mode,
        days_per_week: 4,
        emphasis,
        exercises,
        history,
        last_set_at,
        settings: settings(),
        groups: groups(),
        available_equipment: available.map(|v| v.into_iter().collect()),
        equipment_loads: HashMap::new(),
        readiness: None,
    }
}

// A catalog covering all three groups, bodyweight (doable anywhere).
fn catalog() -> Vec<ExerciseInfo> {
    vec![
        ex(
            1,
            "Push-up",
            Pattern::Push,
            Metric::Reps,
            false,
            vec![],
            vec![(10, MuscleRole::Primary)],
        ),
        ex(
            2,
            "Ring row",
            Pattern::Pull,
            Metric::Reps,
            true,
            vec![],
            vec![(20, MuscleRole::Primary)],
        ),
        ex(
            3,
            "Squat",
            Pattern::Legs,
            Metric::Reps,
            false,
            vec![],
            vec![(30, MuscleRole::Primary)],
        ),
    ]
}

// A single barbell-row exercise (weighted) on the back group, for prescription
// tests. `loads` is the owned inventory at equipment id 3.
fn barbell_row() -> ExerciseInfo {
    ex(
        5,
        "Barbell row",
        Pattern::Pull,
        Metric::WeightedReps,
        false,
        vec![3],
        vec![(20, MuscleRole::Primary)],
    )
}
fn back_only() -> Vec<GroupMeta> {
    vec![GroupMeta {
        id: 20,
        name: "Lats".into(),
        region: Region::Back,
    }]
}

#[test]
fn fresh_when_no_history() {
    let out = evaluate(&input(Mode::Balanced, catalog(), vec![], None, None), now());
    assert_eq!(out.state, PacingState::Fresh);
    assert!(
        out.suggestion.is_some(),
        "cold start still suggests something"
    );
    assert_eq!(out.groups.len(), 3);
}

#[test]
fn surfaces_the_lagging_group() {
    // Chest + legs trained a lot this week; back untouched → back is the focus.
    let mut h = vec![];
    for d in 1..6 {
        h.push(set(1, days_ago(d))); // push-up (chest)
        h.push(set(3, days_ago(d))); // squat (legs)
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert_eq!(out.state, PacingState::Active);
    let sug = out.suggestion.unwrap();
    assert_eq!(sug.exercise_id, 2); // ring row — the back exercise
    assert_eq!(sug.group, "Lats");
    // The single suggestion is just the head of the ordered plan.
    assert_eq!(out.plan.first().map(|s| s.exercise_id), Some(2));
}

#[test]
fn the_plan_is_ordered_and_sized_to_the_day_budget() {
    // Nothing trained this week → all three groups are in deficit. The plan should
    // cover them, ordered by training tier, sized within the day's set budget.
    let out = evaluate(&input(Mode::Balanced, catalog(), vec![], None, None), now());
    assert!(
        out.plan.len() >= 2,
        "a fresh week plans multiple groups, got {}",
        out.plan.len()
    );
    // Each group appears once; total sets don't exceed the day target.
    let total: i32 = out.plan.iter().map(|s| s.sets).sum();
    assert!(
        total <= out.day_target_sets,
        "planned {total} sets over budget {}",
        out.day_target_sets
    );
    // A weighted compound (tier 3) never precedes a skill/hold (tier 2), etc. —
    // here all three are bodyweight accessories, so order falls to deficit/id and
    // the plan stays deterministic across calls.
    let again = evaluate(&input(Mode::Balanced, catalog(), vec![], None, None), now());
    let ids: Vec<_> = out.plan.iter().map(|s| s.exercise_id).collect();
    let ids2: Vec<_> = again.plan.iter().map(|s| s.exercise_id).collect();
    assert_eq!(ids, ids2, "the plan is deterministic");
}

#[test]
fn skill_and_hold_work_is_ordered_before_heavy_compounds() {
    // A ring skill (tier 2) and a barbell compound (tier 3), both back, both in
    // deficit. The skill leads — fresh CNS first.
    let exs = vec![
        ex(
            5,
            "Barbell row",
            Pattern::Pull,
            Metric::WeightedReps,
            false,
            vec![],
            vec![(20, MuscleRole::Primary)],
        ),
        ex(
            7,
            "Front lever",
            Pattern::Pull,
            Metric::Hold,
            true,
            vec![],
            vec![(10, MuscleRole::Primary)], // chest group, different focus
        ),
    ];
    let out = evaluate(&input(Mode::Balanced, exs, vec![], None, None), now());
    let order: Vec<_> = out.plan.iter().map(|s| s.exercise_id).collect();
    let skill = order.iter().position(|&id| id == 7);
    let compound = order.iter().position(|&id| id == 5);
    if let (Some(sk), Some(co)) = (skill, compound) {
        assert!(
            sk < co,
            "skill/hold (7) before heavy compound (5): {order:?}"
        );
    }
}

#[test]
fn warmups_are_never_picked_as_work_and_credit_no_volume() {
    // A back group with only a warm-up move available → no work suggestion for it
    // (warm-ups belong to the warm-up block, not the work plan). And logging the
    // warm-up leaves the group's deficit untouched — it credits no volume.
    let exs = vec![warmup_ex(9, "Band pull-apart", 20)];
    let mut h = vec![];
    for _ in 0..10 {
        h.push(set(9, hours_ago(2))); // ten warm-up sets on the back group
    }
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            ..input(Mode::Balanced, exs, h, None, None)
        },
        now(),
    );
    // No plan item is the warm-up move; the back group still reads in deficit.
    assert!(out.plan.iter().all(|s| s.exercise_id != 9));
    let back = out.groups.iter().find(|g| g.group == "Lats").unwrap();
    assert_eq!(back.current, 0.0, "warm-up volume didn't credit the group");
    assert_eq!(out.day_done_sets, 0, "warm-ups don't count toward the day");
}

#[test]
fn the_warmup_block_leads_and_covers_the_session_groups() {
    // A back work exercise + a warm-up mobility move for the back group. The plan
    // should open with the warm-up (tier 1), covering the group we're training.
    let exs = vec![
        ex(
            2,
            "Ring row",
            Pattern::Pull,
            Metric::Reps,
            true,
            vec![],
            vec![(20, MuscleRole::Primary)],
        ),
        warmup_ex(9, "Band pull-apart", 20), // warms the back group
    ];
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            ..input(Mode::Balanced, exs, vec![], None, None)
        },
        now(),
    );
    let head = out.plan.first().unwrap();
    assert_eq!(head.kind, SuggestionKind::Warmup);
    assert_eq!(head.exercise_id, 9, "warm-up leads the session");
    // The training item (id 2, never done → an assessment) still follows.
    assert!(
        out.plan
            .iter()
            .any(|s| s.exercise_id == 2 && s.kind != SuggestionKind::Warmup)
    );
    // No warm-up is offered for a group we're not training.
    assert_eq!(
        out.plan
            .iter()
            .filter(|s| s.kind == SuggestionKind::Warmup)
            .count(),
        1
    );
}

#[test]
fn a_heavy_lift_gets_a_ramp_in_warmup_set() {
    // A weighted work item → the warm-up block adds a light ramp-in set (~half the
    // working load) of that same lift.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(3, vec![20.0, 30.0, 40.0, 50.0, 60.0])]);
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            equipment_loads: owned,
            ..input(
                Mode::Strength,
                vec![barbell_row()],
                vec![
                    wset(5, days_ago(2), 60.0, 5),
                    wset(5, days_ago(5), 60.0, 5),
                    wset(5, days_ago(9), 60.0, 5),
                ],
                None,
                Some(vec![3]),
            )
        },
        now(),
    );
    let work = out
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Work)
        .unwrap();
    let ramp = out
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Warmup && s.exercise_id == 5)
        .expect("a ramp-in warm-up of the lift");
    assert!(
        ramp.load_kg.unwrap() < work.load_kg.unwrap(),
        "ramp-in ({:?}) lighter than the working load ({:?})",
        ramp.load_kg,
        work.load_kg
    );
}

#[test]
fn recovery_gate_skips_a_just_worked_group() {
    // Back hammered 6h ago (recovering); chest untouched → chest surfaces.
    let mut h = vec![];
    for _ in 0..4 {
        h.push(set(2, hours_ago(6))); // ring row (back), recent
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let sug = out.suggestion.unwrap();
    assert_ne!(sug.group, "Lats", "the just-worked group is gated out");
    let back = out.groups.iter().find(|g| g.group == "Lats").unwrap();
    assert!(back.recovering);
}

#[test]
fn mode_changes_the_bias() {
    // Two back exercises: a loaded barbell row and a bodyweight ring skill.
    let exs = vec![
        ex(
            5,
            "Barbell row",
            Pattern::Pull,
            Metric::WeightedReps,
            false,
            vec![],
            vec![(20, MuscleRole::Primary)],
        ),
        ex(
            6,
            "Front lever row",
            Pattern::Pull,
            Metric::Reps,
            true,
            vec![],
            vec![(20, MuscleRole::Primary)],
        ),
    ];
    let mk = |mode| PacingInput {
        groups: back_only(),
        ..input(mode, exs.clone(), vec![], None, None)
    };
    let strength = evaluate(&mk(Mode::Strength), now()).suggestion.unwrap();
    let skills = evaluate(&mk(Mode::Skills), now()).suggestion.unwrap();
    assert_eq!(strength.exercise_id, 5, "strength favours the loaded row");
    assert_eq!(skills.exercise_id, 6, "skills favours the ring skill");
}

#[test]
fn location_substitutes_the_ideal() {
    // Strength → barbell row is ideal, but the barbell (id 101) isn't here; the
    // ring row (bodyweight) is swapped in.
    let exs = vec![
        ex(
            5,
            "Barbell row",
            Pattern::Pull,
            Metric::WeightedReps,
            false,
            vec![101],
            vec![(20, MuscleRole::Primary)],
        ),
        ex(
            2,
            "Ring row",
            Pattern::Pull,
            Metric::Reps,
            true,
            vec![],
            vec![(20, MuscleRole::Primary)],
        ),
    ];
    let inp = PacingInput {
        groups: back_only(),
        ..input(Mode::Strength, exs, vec![], None, Some(vec![]))
    };
    let sug = evaluate(&inp, now()).suggestion.unwrap();
    assert_eq!(sug.exercise_id, 2);
    assert_eq!(sug.substituted_for.as_deref(), Some("Barbell row"));
}

#[test]
fn prescribes_from_demonstrated_capacity_not_a_blind_jump() {
    // One fresh top set of 6 × 60 kg (top of the Strength range). The old engine
    // blindly bumped to 62.5 kg; ability-derived prescription won't exceed what
    // the reps support — it holds 60 kg at the top of the range until a better
    // set raises the estimate.
    let inp = PacingInput {
        groups: back_only(),
        ..input(
            Mode::Strength,
            vec![barbell_row()],
            vec![wset(5, days_ago(2), 60.0, 6)],
            None,
            None,
        )
    };
    let sug = evaluate(&inp, now()).suggestion.unwrap();
    assert_eq!(
        sug.load_kg,
        Some(60.0),
        "no blind +2.5 the reps don't support"
    );
    assert_eq!(sug.rep_high, Some(6));
    assert!(sug.rep_low.unwrap() >= 3 && sug.rep_low.unwrap() <= 6);
}

#[test]
fn a_stronger_history_earns_a_heavier_owned_weight() {
    // Same exercise, owned 15/17.5/20 kg. A weaker recent history prescribes a
    // lighter owned weight than a stronger one — the load step is *earned* by the
    // logged sets raising the e1RM past the next weight, never a blind bump.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(3, vec![15.0, 17.5, 20.0])]);
    let sug = |hist: Vec<SetRec>| {
        let inp = PacingInput {
            groups: back_only(),
            equipment_loads: owned.clone(),
            ..input(
                Mode::Strength,
                vec![barbell_row()],
                hist,
                None,
                Some(vec![3]),
            )
        };
        evaluate(&inp, now()).suggestion.unwrap()
    };
    let weak = sug(vec![wset(5, days_ago(2), 15.0, 8)]); // e1RM ≈ 19
    let strong = sug(vec![wset(5, days_ago(2), 20.0, 5)]); // e1RM ≈ 23.3
    assert!(
        strong.load_kg.unwrap() > weak.load_kg.unwrap(),
        "stronger history → heavier owned weight ({:?} > {:?})",
        strong.load_kg,
        weak.load_kg
    );
    // Every prescribed load is a weight actually owned here.
    for s in [&weak, &strong] {
        assert!(
            owned[&3].contains(&s.load_kg.unwrap()),
            "prescribed {:?} must be an owned weight",
            s.load_kg
        );
    }
}

#[test]
fn a_stale_pr_is_not_prescribed_at_face_value() {
    // A 6 × 60 kg top set from 200 days ago and nothing since: the old engine
    // would prescribe ~60 kg + a rep. Staleness decays the estimate, so the
    // prescription is conservatively lighter — a returning athlete rebuilds.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(3, vec![40.0, 50.0, 60.0])]);
    let inp = PacingInput {
        groups: back_only(),
        equipment_loads: owned,
        ..input(
            Mode::Strength,
            vec![barbell_row()],
            vec![wset(5, days_ago(200), 60.0, 6)],
            None,
            Some(vec![3]),
        )
    };
    let sug = evaluate(&inp, now()).suggestion.unwrap();
    assert!(
        sug.load_kg.unwrap() < 60.0,
        "stale PR decayed below its old weight, got {:?}",
        sug.load_kg
    );
}

#[test]
fn a_never_done_lift_is_an_assessment_at_the_lightest_owned_weight() {
    // No history for a weighted lift → the engine can't prescribe honestly, so it
    // asks you to calibrate: one build-up set at the lightest weight you own.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(3, vec![10.0, 15.0, 20.0])]);
    let inp = PacingInput {
        groups: back_only(),
        equipment_loads: owned,
        ..input(
            Mode::Strength,
            vec![barbell_row()],
            vec![],
            None,
            Some(vec![3]),
        )
    };
    let sug = evaluate(&inp, now()).suggestion.unwrap();
    assert_eq!(sug.kind, SuggestionKind::Assess);
    assert_eq!(sug.sets, 1, "a single calibration set");
    assert_eq!(sug.load_kg, Some(10.0));
}

#[test]
fn trusted_ability_prescribes_untrusted_ability_assesses() {
    // Same lift + owned inventory. Three recent sessions → High confidence → a
    // real prescription (Work). Only a 200-day-old set → Low confidence → the
    // engine re-measures (Assess) rather than trust the stale number.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(3, vec![40.0, 50.0, 60.0])]);
    let mk = |hist: Vec<SetRec>| {
        let inp = PacingInput {
            groups: back_only(),
            equipment_loads: owned.clone(),
            ..input(
                Mode::Strength,
                vec![barbell_row()],
                hist,
                None,
                Some(vec![3]),
            )
        };
        evaluate(&inp, now()).suggestion.unwrap().kind
    };
    let trusted = mk(vec![
        wset(5, days_ago(2), 50.0, 5),
        wset(5, days_ago(5), 50.0, 5),
        wset(5, days_ago(9), 50.0, 5),
    ]);
    let stale = mk(vec![wset(5, days_ago(200), 60.0, 6)]);
    assert_eq!(trusted, SuggestionKind::Work);
    assert_eq!(stale, SuggestionKind::Assess);
}

#[test]
fn low_readiness_prescribes_lighter_than_a_good_day() {
    // Identical history + inventory; a low-readiness day leaves more in reserve,
    // so the working load is lighter (never heavier) than a normal day.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(3, vec![40.0, 45.0, 50.0, 55.0, 60.0])]);
    let mk = |r: Option<Readiness>| {
        let inp = PacingInput {
            groups: back_only(),
            equipment_loads: owned.clone(),
            readiness: r,
            ..input(
                Mode::Strength,
                vec![barbell_row()],
                vec![wset(5, days_ago(2), 55.0, 6)],
                None,
                Some(vec![3]),
            )
        };
        evaluate(&inp, now()).suggestion.unwrap().load_kg.unwrap()
    };
    let normal = mk(None);
    let low = mk(Some(Readiness {
        score: 0.2,
        band: Band::Low,
    }));
    assert!(
        low <= normal,
        "low readiness ({low}) not heavier than normal ({normal})"
    );
    assert!(low < normal, "low readiness should ease the load off");
}

#[test]
fn recovery_is_graded_over_a_region_horizon() {
    // Two sets on the back group. Freshly done → the group reads as recovering;
    // well past the back region's recovery horizon → recovered again.
    let recovering_at = |hours: i64| {
        let mut h = vec![];
        for _ in 0..2 {
            h.push(set(2, hours_ago(hours))); // ring row → back group
        }
        let out = evaluate(
            &PacingInput {
                groups: back_only(),
                ..input(Mode::Balanced, vec![catalog()[1].clone()], h, None, None)
            },
            now(),
        );
        out.groups
            .iter()
            .find(|g| g.group == "Lats")
            .unwrap()
            .recovering
    };
    assert!(recovering_at(6), "just trained → still recovering");
    assert!(!recovering_at(80), "past the horizon → recovered");
}

#[test]
fn low_readiness_reduces_the_day_target() {
    // Same history; a low-readiness day prescribes fewer sets, not just lighter
    // ones (the recovery factor now reaches the day's set count). Dense history +
    // 1 day/week keeps the target above its floor so the scaling is visible.
    let mut h = vec![];
    for d in 8..40 {
        for _ in 0..2 {
            h.push(set(1, days_ago(d))); // ~64 sets, none in the last week (no deload)
        }
    }
    let mk = |r: Option<Readiness>| {
        evaluate(
            &PacingInput {
                days_per_week: 1,
                readiness: r,
                ..input(Mode::Balanced, catalog(), h.clone(), None, None)
            },
            now(),
        )
        .day_target_sets
    };
    let normal = mk(None);
    let low = mk(Some(Readiness {
        score: 0.15,
        band: Band::Low,
    }));
    assert!(low < normal, "low readiness {low} < normal {normal}");
}

#[test]
fn rest_when_everything_recovered() {
    // Every group trained hard in the last day → nothing due → Rest.
    let mut h = vec![];
    for _ in 0..5 {
        h.push(set(1, hours_ago(10)));
        h.push(set(2, hours_ago(10)));
        h.push(set(3, hours_ago(10)));
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert_eq!(out.state, PacingState::Rest);
    assert!(out.suggestion.is_none());
    assert!(out.reason.contains("rest"));
}

#[test]
fn auto_deload_when_volume_spikes() {
    // Almost all volume is in the last 7 days (far above the 8-week average).
    let mut h = vec![];
    for d in 0..7 {
        for _ in 0..10 {
            h.push(set(1, days_ago(d)));
        }
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert!(out.deload, "a recent volume spike triggers auto-deload");
}

#[test]
fn nudges_when_behind_midday() {
    // A due group + nothing done today + spacing ok → behind → nudge.
    let mut h = vec![];
    for d in 2..6 {
        h.push(set(1, days_ago(d)));
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert!(out.within_window && !out.after_window && out.spacing_ok);
    assert!(out.nudge);
    assert!(out.day_target_sets >= 3);
}

#[test]
fn readiness_scales_the_target() {
    // Same state, high vs low biometric readiness → higher vs lower group target.
    let mk = |r: Readiness| PacingInput {
        readiness: Some(r),
        ..input(Mode::Balanced, catalog(), vec![], None, None)
    };
    let high = evaluate(
        &mk(Readiness {
            score: 0.9,
            band: Band::High,
        }),
        now(),
    );
    let low = evaluate(
        &mk(Readiness {
            score: 0.2,
            band: Band::Low,
        }),
        now(),
    );
    let ht = high
        .groups
        .iter()
        .find(|g| g.group == "Chest")
        .unwrap()
        .target;
    let lt = low
        .groups
        .iter()
        .find(|g| g.group == "Chest")
        .unwrap()
        .target;
    assert!(
        ht > lt,
        "recovered → higher target ({ht}) than spent ({lt})"
    );
    assert_eq!(high.readiness.map(|r| r.band), Some(Band::High));
}

#[test]
fn readiness_suppresses_volume_deload() {
    // The volume-spike deload scenario, but with biometric readiness present: the
    // real recovery signal supersedes the crude proxy, so `deload` stays off.
    let mut h = vec![];
    for d in 0..7 {
        for _ in 0..10 {
            h.push(set(1, days_ago(d)));
        }
    }
    let inp = PacingInput {
        readiness: Some(Readiness {
            score: 0.9,
            band: Band::High,
        }),
        ..input(Mode::Balanced, catalog(), h, None, None)
    };
    let out = evaluate(&inp, now());
    assert!(!out.deload, "readiness supersedes the volume-spike deload");
    assert!(out.readiness.is_some());
}

#[test]
fn high_readiness_notes_the_reason() {
    // A due group + recovered → the reason carries the readiness clause.
    let mut h = vec![];
    for d in 2..6 {
        h.push(set(1, days_ago(d)));
    }
    let inp = PacingInput {
        readiness: Some(Readiness {
            score: 0.9,
            band: Band::High,
        }),
        ..input(Mode::Balanced, catalog(), h, None, None)
    };
    let out = evaluate(&inp, now());
    assert!(out.suggestion.is_some());
    assert!(out.reason.contains("push"), "reason: {}", out.reason);
}

#[test]
fn outside_the_window_suggests_but_never_nudges() {
    // A due back group; you can still train + get a suggestion outside the window,
    // coach just won't nudge — and past the end it defers to tomorrow.
    let hist = || (2..6).map(|d| set(1, days_ago(d))).collect::<Vec<_>>();
    let at = |hh| {
        NaiveDate::from_ymd_opt(2026, 7, 6)
            .unwrap()
            .and_hms_opt(hh, 0, 0)
            .unwrap()
    };

    // After the window's end (22:00, end=21): defers to tomorrow, no nudge.
    let late = evaluate(
        &input(Mode::Balanced, catalog(), hist(), None, None),
        at(22),
    );
    assert!(late.after_window && !late.within_window);
    assert!(!late.nudge);
    assert!(late.suggestion.is_some(), "still trainable any time");
    assert!(late.reason.contains("rolls to tomorrow"));

    // Before the window's start (06:00, start=8): neutral, no nudge, no defer.
    let early = evaluate(&input(Mode::Balanced, catalog(), hist(), None, None), at(6));
    assert!(!early.within_window && !early.after_window);
    assert!(!early.nudge);
    assert!(early.suggestion.is_some());
    assert!(early.reason.contains("Outside your training window"));
}

#[test]
fn emphasis_biases_a_region() {
    // Nothing done; legs emphasis pushes the quads target up so legs leads.
    let inp = input(Mode::Balanced, catalog(), vec![], Some(Region::Legs), None);
    let out = evaluate(&inp, now());
    let quads = out.groups.iter().find(|g| g.group == "Quadriceps").unwrap();
    let chest = out.groups.iter().find(|g| g.group == "Chest").unwrap();
    assert!(
        quads.target > chest.target,
        "emphasised region has a higher target"
    );
}
