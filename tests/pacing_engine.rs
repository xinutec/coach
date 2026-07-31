//! Dynamic-engine tests. Integration tests against the public `evaluate` + its
//! input/output types — the engine is a pure function, exercised through the same
//! surface `service::now` uses.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use std::collections::BTreeMap;

use coach::exercise::types::{Metric, Pattern};
use coach::muscle::types::{MuscleRole, Region};
use coach::pacing::ability::Confidence;
use coach::pacing::engine::evaluate;
use coach::pacing::types::{
    Band, Blocker, ExerciseInfo, GroupMeta, Kit, PacingInput, PacingNow, PacingSettings,
    PacingState, Readiness, SetRec, Suggestion, SuggestionKind, WindowState,
};
use coach::settings::types::Mode;
use coach_pacing::domain::{EquipmentId, ExerciseId, GroupId, SetId};

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
fn minutes_ago(m: i64) -> NaiveDateTime {
    now() - Duration::minutes(m)
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
            id: GroupId(10),
            name: "Chest".into(),
            region: Region::Chest,
        },
        GroupMeta {
            id: GroupId(20),
            name: "Lats".into(),
            region: Region::Back,
        },
        GroupMeta {
            id: GroupId(30),
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
        id: ExerciseId(id),
        name: name.into(),
        family: name.into(),
        difficulty: None,
        pattern,
        metric,
        is_skill,
        is_power: false,
        warmup: false,
        equipment: equipment.into_iter().map(EquipmentId).collect(),
        groups: grps.into_iter().map(|(g, r)| (GroupId(g), r)).collect(),
    }
}

/// A warm-up (mobility) exercise on `group`, doable anywhere.
fn warmup_ex(id: i64, name: &str, group: i64) -> ExerciseInfo {
    ExerciseInfo {
        id: ExerciseId(id),
        name: name.into(),
        family: name.into(),
        difficulty: None,
        pattern: Pattern::Core,
        metric: Metric::Reps,
        is_skill: false,
        is_power: false,
        warmup: true,
        equipment: vec![],
        groups: vec![(GroupId(group), MuscleRole::Primary)],
    }
}

/// A bodyweight set (reps only) — for volume/recovery scenarios.
fn set(exercise_id: i64, at: NaiveDateTime) -> SetRec {
    SetRec {
        id: SetId(0),
        exercise_id: ExerciseId(exercise_id),
        logged_at: at,
        reps: Some(8),
        load_kg: None,
        hold_s: None,
        distance_m: None,
        rpe: None,
    }
}

/// A bodyweight set with an explicit rep count — for scenarios where the
/// demonstrated maximum, not the volume, is the point.
fn bset(exercise_id: i64, at: NaiveDateTime, reps: i32) -> SetRec {
    SetRec {
        id: SetId(0),
        exercise_id: ExerciseId(exercise_id),
        logged_at: at,
        reps: Some(reps),
        load_kg: None,
        hold_s: None,
        distance_m: None,
        rpe: None,
    }
}

/// A weighted set (load + reps) — feeds the ability estimate that prescription
/// derives from.
fn wset(exercise_id: i64, at: NaiveDateTime, load: f64, reps: i32) -> SetRec {
    SetRec {
        id: SetId(0),
        exercise_id: ExerciseId(exercise_id),
        logged_at: at,
        reps: Some(reps),
        load_kg: Some(load),
        hold_s: None,
        distance_m: None,
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
    // `available: None` = "the kit isn't what this test is about", which now means
    // a location stocked with everything the catalog needs — not the old "no
    // filter" special case (there isn't one: absent kit means absent kit).
    let kit = Kit(match available {
        Some(v) => v.into_iter().map(EquipmentId).collect(),
        None => exercises.iter().flat_map(|e| e.equipment.clone()).collect(),
    });
    PacingInput {
        mode,
        days_per_week: 4,
        emphasis,
        exercises,
        history,
        last_set_at,
        settings: settings(),
        groups: groups(),
        kit: Some(kit),
        exercise_loads: BTreeMap::new(),
        equipment_names: BTreeMap::new(),
        notices: Vec::new(),
        readiness: None,
        readiness_history: Default::default(),
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
        id: GroupId(20),
        name: "Lats".into(),
        region: Region::Back,
    }]
}

/// Buildable loads for the barbell row (exercise id 5): 20…80 kg in 2.5 kg steps.
/// Keyed by *exercise*, not equipment — what you can build depends on how many
/// implements the movement uses, so the service resolves it per exercise. Without
/// an inventory there's no honest load, so the engine leaves the lift out.
fn owned() -> BTreeMap<ExerciseId, Vec<f64>> {
    let mut loads = Vec::new();
    let mut w = 20.0;
    while w <= 80.0 + 1e-9 {
        loads.push(w);
        w += 2.5;
    }
    BTreeMap::from([(ExerciseId(5), loads)])
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
    assert_eq!(sug.exercise_id, ExerciseId(2)); // ring row — the back exercise
    assert_eq!(sug.group, "Lats");
    // The single suggestion is just the head of the ordered plan.
    assert_eq!(out.plan.first().map(|s| s.exercise_id), Some(ExerciseId(2)));
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
fn a_started_movement_is_confirmed_even_when_its_group_is_covered() {
    // Two recent sessions of push-ups, several sets each: enough that Chest's weekly
    // volume is met and the group is still recovering, so under pure coverage the
    // engine would flee to the untouched groups and never ask for push-ups again.
    // But one or two sessions is not a trusted baseline. The coach should keep
    // asking for the movement until the estimate is solid — confirming what you've
    // *started* before broadening into new movements. This is the whole calibration
    // fix: on day two, repeat, don't scatter. (Volume sits a few days back, so the
    // group has recovered — confirmation waits on recovery, it doesn't override it.)
    let mut h = vec![];
    for _ in 0..12 {
        h.push(set(1, days_ago(4))); // push-up (chest): covered for the week, recovered
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());

    let pushup = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(1))
        .expect("the started movement is confirmed, not abandoned once its group is covered");
    assert_eq!(pushup.kind, SuggestionKind::Work);
    assert!(
        pushup.sets >= 2,
        "confirmation takes its minimum effective dose, got {}",
        pushup.sets
    );
    let e = pushup
        .explanation
        .as_ref()
        .expect("a confirmed pick still explains itself");
    assert!(
        e.confirming,
        "it earned its place by confirming a baseline — its group was already covered"
    );
    assert_eq!(
        pushup.group, "Chest",
        "a confirmation pick still labels to a real group"
    );
}

#[test]
fn a_trusted_movement_is_not_flagged_for_confirmation() {
    // The same push-up, now trained on three distinct days → High confidence. Its
    // estimate is trusted, so it is never specially *confirmed*; if it appears it's
    // on ordinary coverage, and the confirmation flag stays off.
    let h = vec![
        set(1, days_ago(1)),
        set(1, days_ago(3)),
        set(1, days_ago(5)),
    ];
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    for s in &out.plan {
        if let Some(e) = &s.explanation {
            assert!(
                !e.confirming,
                "{} is trusted or untouched — nothing to confirm",
                s.exercise_name
            );
        }
    }
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
    let skill = order.iter().position(|&id| id == ExerciseId(7));
    let compound = order.iter().position(|&id| id == ExerciseId(5));
    if let (Some(sk), Some(co)) = (skill, compound) {
        assert!(
            sk < co,
            "skill/hold (7) before heavy compound (5): {order:?}"
        );
    }
}

#[test]
fn power_work_leads_even_skill_and_compounds() {
    // A ballistic jump (tier 1), a ring skill/hold (tier 2), and a barbell
    // compound (tier 3), three different groups so all three make the plan. Power
    // leads *both*: a jump for distance is only worth doing on a fresh CNS, and
    // when it's a never-done calibration (as here) the number it produces feeds the
    // ability model — fatigue in front of it corrupts that measurement, not just a
    // rep target. The jump is patterned Core on purpose: box jumps and slams are,
    // and the finisher tier must not claim them ahead of the power check.
    let jump = ExerciseInfo {
        id: ExerciseId(9),
        name: "Broad jump".into(),
        family: "Broad jump".into(),
        difficulty: Some(2),
        pattern: Pattern::Core,
        metric: Metric::Reps,
        is_skill: false,
        is_power: true,
        warmup: false,
        equipment: vec![],
        groups: vec![(GroupId(30), MuscleRole::Primary)],
    };
    let exs = vec![
        // A bodyweight compound (breadth 3 → tier 3), doable without registered
        // loads so the kit can't quietly drop it.
        ex(
            5,
            "Pull-up",
            Pattern::Pull,
            Metric::Reps,
            false,
            vec![],
            vec![
                (20, MuscleRole::Primary),
                (10, MuscleRole::Secondary),
                (30, MuscleRole::Secondary),
            ],
        ),
        ex(
            7,
            "Front lever",
            Pattern::Pull,
            Metric::Hold,
            true,
            vec![],
            vec![(10, MuscleRole::Primary)],
        ),
        jump,
    ];
    let out = evaluate(&input(Mode::Balanced, exs, vec![], None, None), now());
    let order: Vec<_> = out.plan.iter().map(|s| s.exercise_id).collect();
    let power = order.iter().position(|&id| id == ExerciseId(9));
    let skill = order.iter().position(|&id| id == ExerciseId(7));
    let compound = order.iter().position(|&id| id == ExerciseId(5));
    if let (Some(p), Some(sk), Some(co)) = (power, skill, compound) {
        assert!(
            p < sk && p < co,
            "power (9) leads skill (7) and compound (5): {order:?}"
        );
    } else {
        panic!("expected power, skill and compound all in the plan: {order:?}");
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
    assert!(out.plan.iter().all(|s| s.exercise_id != ExerciseId(9)));
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
    assert_eq!(head.exercise_id, ExerciseId(9), "warm-up leads the session");
    // The training item (id 2, never done → an assessment) still follows.
    assert!(
        out.plan
            .iter()
            .any(|s| s.exercise_id == ExerciseId(2) && s.kind != SuggestionKind::Warmup)
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
fn a_group_with_no_mobility_move_is_named_not_silently_left_bare() {
    // The catalog only has drills for the groups someone authored drills for. A
    // session training a group with none produces an empty warm-up, which reads
    // exactly like "you don't need one" — so the coach says whose warm-up it
    // doesn't know rather than leaving a hole the athlete can't see.
    let exs = vec![ex(
        2,
        "Ring row",
        Pattern::Pull,
        Metric::Reps,
        true,
        vec![],
        vec![(20, MuscleRole::Primary)],
    )]; // no warm-up move for group 20 anywhere in the catalog
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            ..input(Mode::Balanced, exs, vec![], None, None)
        },
        now(),
    );
    assert!(
        !out.plan.iter().any(|s| s.kind == SuggestionKind::Warmup),
        "nothing to warm up with — and it must not invent one"
    );
    assert!(
        out.notices.iter().any(|n| n.contains("warm-up")),
        "the missing warm-up is said out loud, got {:?}",
        out.notices
    );
}

#[test]
fn a_heavy_lift_gets_a_ramp_in_warmup_set() {
    // A weighted work item → the warm-up block adds a light ramp-in set (~half the
    // working load) of that same lift.
    let owned: BTreeMap<ExerciseId, Vec<f64>> =
        BTreeMap::from([(ExerciseId(5), vec![20.0, 30.0, 40.0, 50.0, 60.0])]);
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            exercise_loads: owned,
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
        .find(|s| s.kind == SuggestionKind::Warmup && s.exercise_id == ExerciseId(5))
        .expect("a ramp-in warm-up of the lift");
    assert!(
        ramp.ask.load_kg().unwrap() < work.ask.load_kg().unwrap(),
        "ramp-in ({:?}) lighter than the working load ({:?})",
        ramp.ask.load_kg(),
        work.ask.load_kg()
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
    // Two back exercises: a loaded barbell row (weights registered) and a
    // bodyweight ring skill.
    let exs = vec![
        ex(
            5,
            "Barbell row",
            Pattern::Pull,
            Metric::WeightedReps,
            false,
            vec![3],
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
    // Both *trusted* (three sessions each, well in the past): confidence is High, so
    // there's no calibration confirmation in play to force a minimum dose on each —
    // the day's sets are free to pool into whichever the mode prefers. (One session
    // apiece would put both in confirmation, which deliberately splits the budget to
    // firm up each baseline; that's a different behaviour, tested elsewhere.)
    let hist = vec![
        wset(5, days_ago(10), 60.0, 5),
        wset(5, days_ago(12), 60.0, 5),
        wset(5, days_ago(14), 60.0, 5),
        set(6, days_ago(10)),
        set(6, days_ago(12)),
        set(6, days_ago(14)),
    ];
    let mk = |mode| PacingInput {
        groups: back_only(),
        exercise_loads: owned(),
        ..input(mode, exs.clone(), hist.clone(), None, None)
    };
    // The bias shows up as where the day's sets *go*, not as what leads the list:
    // session order is a separate rule (skills first, while the CNS is fresh), so
    // reading the plan's head would conflate preference with ordering.
    let sets_of = |out: &coach::pacing::types::PacingNow, id: ExerciseId| -> i32 {
        out.plan
            .iter()
            .filter(|s| s.exercise_id == id && s.kind != SuggestionKind::Warmup)
            .map(|s| s.sets)
            .sum()
    };
    let strength = evaluate(&mk(Mode::Strength), now());
    let skills = evaluate(&mk(Mode::Skills), now());
    assert!(
        sets_of(&strength, ExerciseId(5)) > sets_of(&strength, ExerciseId(6)),
        "strength spends the day on the loaded row"
    );
    assert!(
        sets_of(&skills, ExerciseId(6)) > sets_of(&skills, ExerciseId(5)),
        "skills spends the day on the ring skill"
    );
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
        equipment_names: BTreeMap::from([(EquipmentId(101), "Barbell".to_string())]),
        ..input(Mode::Strength, exs, vec![], None, Some(vec![]))
    };
    let sug = evaluate(&inp, now()).suggestion.unwrap();
    assert_eq!(sug.exercise_id, ExerciseId(2));
    let sub = sug
        .substituted_for
        .expect("the barbell row is genuinely blocked");
    assert_eq!(sub.ideal, "Barbell row");
    // And it names the kit, so the swap is actionable rather than mysterious.
    assert_eq!(sub.blocker, Blocker::Absent(vec!["Barbell".to_string()]));
}

#[test]
fn substitution_prefers_the_ideal_exercise_metric() {
    // Lat pull down (reps, machine id 101 not here) must swap to another *reps*
    // pull, not to a max hold — a hold is a different ask, not a substitute.
    // The prod bug this pins: Balanced once scored every exercise identically, so
    // a rep-out and an isometric were indistinguishable and the hold's lower id
    // won the tie — "Lat pull down" became "Pull-up (L-sit)". Balanced now rates
    // rep work above holds, so the preference decides it, not the tie-break.
    let exs = vec![
        ex(
            5,
            "Lat pull down",
            Pattern::Pull,
            Metric::Reps,
            false,
            vec![101],
            vec![(20, MuscleRole::Primary)],
        ),
        ex(
            6,
            "Pull-up (L-sit)",
            Pattern::Pull,
            Metric::Hold,
            true,
            vec![],
            vec![(20, MuscleRole::Primary)],
        ),
        ex(
            7,
            "Pull-up (bar)",
            Pattern::Pull,
            Metric::Reps,
            false,
            vec![],
            vec![(20, MuscleRole::Primary)],
        ),
    ];
    let inp = PacingInput {
        groups: back_only(),
        ..input(Mode::Balanced, exs, vec![], None, Some(vec![]))
    };
    let out = evaluate(&inp, now());
    let pull = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(7))
        .expect("a rep pull stands in for the missing machine");
    let hold = out.plan.iter().find(|s| s.exercise_id == ExerciseId(6));
    // The rep pull is the stand-in for the blocked machine — and it's the *first*
    // thing the cover reached for, which its own trace proves: the first pick pays
    // down more of the group's need than anything taken after it.
    assert_eq!(
        pull.substituted_for.as_ref().map(|s| s.ideal.as_str()),
        Some("Lat pull down")
    );
    if let Some(hold) = hold {
        let pays = |s: &coach::pacing::types::Suggestion| s.explanation.unwrap().pays;
        assert!(
            pays(pull) > pays(hold),
            "the rep pull was preferred to the hold, not the other way round"
        );
        assert!(
            hold.substituted_for.is_none(),
            "only the group's first stand-in claims to substitute"
        );
    }
}

#[test]
fn prescribes_from_demonstrated_capacity_not_a_blind_jump() {
    // One fresh top set of 6 × 60 kg (top of the Strength range). The old engine
    // blindly bumped to 62.5 kg; ability-derived prescription won't exceed what
    // the reps support — it holds 60 kg at the top of the range until a better
    // set raises the estimate.
    let inp = PacingInput {
        groups: back_only(),
        exercise_loads: owned(),
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
        sug.ask.load_kg(),
        Some(60.0),
        "no blind +2.5 the reps don't support"
    );
    assert_eq!(sug.ask.rep_high(), Some(6));
    assert!(sug.ask.rep_low().unwrap() >= 3 && sug.ask.rep_low().unwrap() <= 6);
}

// ---- a session in progress is a commitment ---------------------------------
//
// Once the first set of a session lands (a session = sets separated by no more
// than the session gap), the plan is frozen at what the engine would have said
// then; later sets only report progress against it. Without this, every logged
// set re-solved the day: calibrations were re-prescribed above the max just
// demonstrated, targets ratcheted set-over-set, and half-finished movements
// vanished as their muscles read "recovering".

#[test]
fn the_plan_remembers_what_you_did_earlier_today() {
    // A session ends after the session gap. A *day* doesn't. Progress used to be
    // scoped to the session window, so once the gap elapsed the plan forgot
    // everything: on 2026-07-25 three warm-ups logged at 16:21 read back at
    // 22:05 as `0 / 10`, with all three re-offered as still to do. No coach
    // forgets your warm-up because you broke for lunch.
    let exs = vec![
        ex(
            1,
            "Push-up",
            Pattern::Push,
            Metric::Reps,
            false,
            vec![],
            vec![(10, MuscleRole::Primary)],
        ),
        warmup_ex(9, "Arm circles", 10),
    ];
    // Five hours back: same day, well past the session gap, so nothing is "in
    // progress" — the plan is re-solved from scratch and must still credit it.
    let h = vec![set(9, hours_ago(5))];
    let out = evaluate(&input(Mode::Balanced, exs.clone(), h, None, None), now());
    let wu = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(9))
        .expect("the drill is still planned");
    assert_eq!(
        wu.done(),
        1,
        "this morning's warm-up still counts this evening"
    );
    assert!(
        out.plan
            .iter()
            .find(|s| s.done() < s.sets)
            .is_some_and(|s| s.exercise_id != ExerciseId(9)),
        "and it is no longer what to do next"
    );
}

#[test]
fn the_card_reports_what_you_lifted_not_only_how_many_sets() {
    // "1 / 2 sets" answers how many, not what. Standing over the bar on set two
    // the question is what set one was, and the only place that lived was the
    // History tab — so the item carries its logged sets, oldest first.
    let mut h = vec![
        set(1, days_ago(2)),
        set(1, days_ago(4)),
        set(1, days_ago(6)),
    ];
    // Today's two, handed in newest-first — the card must still read them in the
    // order they happened.
    h.push(bset(1, minutes_ago(9), 6));
    h.push(bset(1, minutes_ago(21), 9));
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let item = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(1))
        .expect("push-up planned");
    assert_eq!(
        item.done() as usize,
        item.logged.len(),
        "one entry per done set"
    );
    assert_eq!(item.done(), 2, "both of today's sets counted");
    assert_eq!(
        item.logged.iter().map(|d| d.reps).collect::<Vec<_>>(),
        vec![Some(9), Some(6)],
        "oldest first, in the terms they were logged in"
    );
}

#[test]
fn yesterdays_sets_are_not_todays_progress() {
    // The other half of the same rule: the day is the unit, so a set logged at
    // this hour *yesterday* pays nothing toward today's card.
    let exs = vec![
        ex(
            1,
            "Push-up",
            Pattern::Push,
            Metric::Reps,
            false,
            vec![],
            vec![(10, MuscleRole::Primary)],
        ),
        warmup_ex(9, "Arm circles", 10),
    ];
    let h = vec![set(9, days_ago(1))];
    let out = evaluate(&input(Mode::Balanced, exs, h, None, None), now());
    let wu = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(9))
        .expect("the drill is planned");
    assert_eq!(wu.done(), 0, "yesterday is not today");
}

#[test]
fn a_calibration_is_complete_after_its_measurement() {
    // A never-done movement is measured (one honest AMRAP, logged mid-session).
    // The plan keeps the card — done, one set of one — and does not turn around
    // and prescribe more of the movement the athlete just took to form breakdown.
    let h = vec![bset(2, minutes_ago(30), 6)]; // ring row: first-ever set, today
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let items: Vec<_> = out
        .plan
        .iter()
        .filter(|s| s.exercise_id == ExerciseId(2))
        .collect();
    assert_eq!(items.len(), 1, "one card for the measured movement");
    assert_eq!(
        items[0].kind,
        SuggestionKind::Assess,
        "still the measurement"
    );
    assert_eq!(items[0].sets, 1);
    assert_eq!(items[0].done(), 1, "and it's done");
    if let Some(sug) = &out.suggestion {
        assert_ne!(
            sug.exercise_id,
            ExerciseId(2),
            "next up is something unfinished, not the spent calibration"
        );
    }
}

#[test]
fn the_committed_plan_survives_a_logged_set() {
    // Mid-session, a movement with one of its sets logged stays on the plan with
    // its ask unchanged and progress shown — it must not vanish half-done (its
    // muscles read "recovering"), and untouched movements must not be dropped.
    let mut h = vec![
        set(1, days_ago(2)),
        set(1, days_ago(4)),
        set(1, days_ago(6)),
    ];
    // The commitment is what the engine said at the session's first set — so
    // that's the instant the un-started plan is read at.
    let before = evaluate(
        &input(Mode::Balanced, catalog(), h.clone(), None, None),
        minutes_ago(10),
    );
    let asked = before
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(1))
        .expect("push-up planned")
        .sets;

    h.push(set(1, minutes_ago(10)));
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let pushup = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(1))
        .expect("a half-done movement stays on the plan");
    assert_eq!(pushup.sets, asked, "the ask is the committed one");
    assert_eq!(pushup.done(), 1, "progress is reported against it");
    assert!(
        out.plan.iter().any(|s| s.exercise_id == ExerciseId(3)),
        "untouched movements stay planned too"
    );
}

#[test]
fn no_rep_ratchet_within_a_session() {
    // Trusted at best 4 with today's probe due (a steady history of quiet
    // sessions — R4-1) → today asks 5. Hitting the 5 must not raise the ask to
    // 6 before the next set — a probe is session-over-session, not set-over-set.
    let mut h: Vec<SetRec> = (1..=7).map(|i| bset(1, days_ago(2 * i), 4)).collect();
    h.push(bset(1, minutes_ago(20), 5));
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let pushup = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(1))
        .unwrap();
    assert_eq!(
        pushup.ask.rep_low(),
        Some(5),
        "the committed target holds for the whole session"
    );
}

#[test]
fn no_new_novel_movement_backfills_mid_session() {
    // Completing a calibration frees a slot under the novelty cap — but the
    // session is committed, so no new never-done movement slides in to spend it.
    let h = vec![bset(1, minutes_ago(15), 6)];
    let committed = evaluate(&input(Mode::Balanced, catalog(), vec![], None, None), now());
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let ids = |p: &coach::pacing::types::PacingNow| {
        let mut v: Vec<ExerciseId> = p.plan.iter().map(|s| s.exercise_id).collect();
        v.sort();
        v
    };
    assert_eq!(
        ids(&out),
        ids(&committed),
        "the session's movements are the ones committed at its start"
    );
}

#[test]
fn a_novel_movement_introduced_today_spends_its_slot_across_sessions() {
    // The cap is on movements *introduced today*, not on pending picks: a novel
    // movement first done this morning (session over — gap well past the session
    // window) still counts, so the evening plan may only introduce cap − 1 more.
    let exs: Vec<ExerciseInfo> = (0..5)
        .map(|i| {
            ex(
                40 + i,
                &format!("Back move {i}"),
                Pattern::Pull,
                Metric::Reps,
                false,
                vec![],
                vec![(20, MuscleRole::Primary)],
            )
        })
        .collect();
    let h = vec![bset(40, hours_ago(3), 6)]; // introduced this morning; not in-session now
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            ..input(Mode::Balanced, exs, h, None, None)
        },
        now(),
    );
    let never_done = out
        .plan
        .iter()
        .filter(|s| s.exercise_id != ExerciseId(40) && s.kind == SuggestionKind::Assess)
        .count();
    assert!(
        never_done <= 2,
        "one novelty slot is already spent today; got {never_done} new introductions"
    );
}

#[test]
fn mid_session_the_coach_says_whats_next_not_take_a_breather() {
    // Thirty seconds after a set, with work remaining, the coach names the next
    // movement — the "just trained, take a breather" line is for after sessions,
    // not between sets.
    let mut h = vec![
        set(1, days_ago(2)),
        set(1, days_ago(4)),
        set(1, days_ago(6)),
    ];
    h.push(set(1, minutes_ago(5)));
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert!(
        !out.reason.contains("breather"),
        "no breather mid-session: {}",
        out.reason
    );
    assert!(
        out.reason.starts_with("Rest") && out.reason.contains("then:"),
        "the mid-set rest names what's next: {}",
        out.reason
    );
}

#[test]
fn a_finished_session_says_so() {
    // Every committed item done → the coach closes the session instead of
    // reporting rest-day balance boilerplate.
    let exs = vec![ex(
        2,
        "Ring row",
        Pattern::Pull,
        Metric::Reps,
        true,
        vec![],
        vec![(20, MuscleRole::Primary)],
    )];
    let mut h = vec![
        set(2, days_ago(2)),
        set(2, days_ago(4)),
        set(2, days_ago(6)),
    ];
    for m in [50, 40, 30, 20] {
        h.push(set(2, minutes_ago(m)));
    }
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            ..input(Mode::Balanced, exs, h, None, None)
        },
        now(),
    );
    assert!(
        out.reason.contains("session"),
        "a finished session is closed, not glossed as a rest day: {}",
        out.reason
    );
}

#[test]
fn a_warmup_is_an_instruction_with_a_dose() {
    // A mobility drill names its dose — reps (or seconds, by its metric) —
    // rather than an undosed "loosen up". And logging it completes it: the
    // warm-up card is part of the session's progress like everything else.
    let exs = vec![
        ex(
            1,
            "Push-up",
            Pattern::Push,
            Metric::Reps,
            false,
            vec![],
            vec![(10, MuscleRole::Primary)],
        ),
        warmup_ex(9, "Arm circles", 10),
    ];
    let out = evaluate(
        &input(Mode::Balanced, exs.clone(), vec![], None, None),
        now(),
    );
    let wu = out
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Warmup && s.exercise_id == ExerciseId(9))
        .expect("the drill leads the plan");
    assert_eq!(wu.ask.rep_low(), Some(10), "a dose, not a vibe");

    // Log the drill mid-session → its card reads done.
    let h = vec![set(9, minutes_ago(10))];
    let out = evaluate(&input(Mode::Balanced, exs, h, None, None), now());
    let wu = out
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Warmup && s.exercise_id == ExerciseId(9))
        .expect("still on the plan");
    assert_eq!(wu.done(), 1, "a logged warm-up completes its card");
}

#[test]
fn a_bodyweight_target_is_one_rep_up_from_ability_not_the_mode_floor() {
    // Recent sessions ground out at 2 reps — an honest maximum, shown while
    // doing one's best. Balanced mode *likes* 8–12 reps, but a style preference is
    // a ceiling to climb toward, not a floor to demand: asking 8 from an athlete
    // who has shown 2 prescribes failure, and it silently defeats the
    // miss-response too (aim best−1, clamped straight back up to 8). A steady
    // every-other-day history long enough that today's probe is due (R4-1) and
    // this week is no spike over the baseline: the target is one rep above the
    // best demonstrated. Only the push-up is on offer — the other groups'
    // calibration cards are beside the point and would eat the small day budget.
    let h: Vec<SetRec> = (1..=7).map(|i| bset(1, days_ago(2 * i), 2)).collect();
    let out = evaluate(
        &input(Mode::Balanced, vec![catalog().remove(0)], h, None, None),
        now(),
    );
    let pushup = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(1))
        .expect("a trusted, recovered movement in deficit is planned");
    assert_eq!(pushup.kind, SuggestionKind::Work);
    assert_eq!(
        pushup.ask.rep_low(),
        Some(3),
        "one rep above the demonstrated 2 — not the mode floor of 8"
    );
    assert_eq!(
        pushup.ask.rep_high(),
        Some(12),
        "the style range still names where the climb tops out"
    );
}

#[test]
fn a_bodyweight_target_never_asks_past_the_mode_ceiling() {
    // The dual bound: an athlete showing 15 clean reps in Balanced mode isn't
    // asked for 16 — past the top of the range the answer is a harder variation,
    // not more of this one.
    let h = vec![
        bset(1, days_ago(2), 15),
        bset(1, days_ago(4), 15),
        bset(1, days_ago(6), 15),
    ];
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let pushup = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(1))
        .unwrap();
    assert_eq!(pushup.ask.rep_low(), Some(12));
}

#[test]
fn a_weighted_target_respects_ability_when_the_lightest_weight_is_heavy() {
    // e1RM ≈ 22 kg, but nothing lighter than 20 kg is owned. At 20 kg the estimate
    // supports ~3 reps; the Balanced floor of 6 must not talk that up into a set
    // the athlete has no way to finish. The rung is what you own — the rep target
    // is what you can do on it.
    let inp = PacingInput {
        groups: back_only(),
        exercise_loads: owned(),
        ..input(
            Mode::Balanced,
            vec![barbell_row()],
            vec![wset(5, days_ago(2), 20.0, 3)],
            None,
            None,
        )
    };
    let sug = evaluate(&inp, now()).suggestion.unwrap();
    assert_eq!(sug.ask.load_kg(), Some(20.0), "the lightest owned rung");
    assert_eq!(
        sug.ask.rep_low(),
        Some(3),
        "the reps the estimate supports at that weight — not the style floor of 6"
    );
}

#[test]
fn a_stronger_history_earns_a_heavier_owned_weight() {
    // Same exercise, owned 15/17.5/20 kg. A weaker recent history prescribes a
    // lighter owned weight than a stronger one — the load step is *earned* by the
    // logged sets raising the e1RM past the next weight, never a blind bump.
    let owned: BTreeMap<ExerciseId, Vec<f64>> =
        BTreeMap::from([(ExerciseId(5), vec![15.0, 17.5, 20.0])]);
    let sug = |hist: Vec<SetRec>| {
        let inp = PacingInput {
            groups: back_only(),
            exercise_loads: owned.clone(),
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
        strong.ask.load_kg().unwrap() > weak.ask.load_kg().unwrap(),
        "stronger history → heavier owned weight ({:?} > {:?})",
        strong.ask.load_kg(),
        weak.ask.load_kg()
    );
    // Every prescribed load is a weight actually owned here.
    for s in [&weak, &strong] {
        assert!(
            owned[&ExerciseId(5)].contains(&s.ask.load_kg().unwrap()),
            "prescribed {:?} must be an owned weight",
            s.ask.load_kg()
        );
    }
}

#[test]
fn a_stale_pr_is_not_prescribed_at_face_value() {
    // A 6 × 60 kg top set from 200 days ago and nothing since: the old engine
    // would prescribe ~60 kg + a rep. Staleness decays the estimate, so the
    // prescription is conservatively lighter — a returning athlete rebuilds.
    let owned: BTreeMap<ExerciseId, Vec<f64>> =
        BTreeMap::from([(ExerciseId(5), vec![40.0, 50.0, 60.0])]);
    let inp = PacingInput {
        groups: back_only(),
        exercise_loads: owned,
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
        sug.ask.load_kg().unwrap() < 60.0,
        "stale PR decayed below its old weight, got {:?}",
        sug.ask.load_kg()
    );
}

#[test]
fn a_work_item_carries_its_reasoning() {
    // A trained group in deficit → the suggestion explains itself: the group's
    // deficit + recovery, the ability confidence, and (here) an e1RM estimate.
    let owned: BTreeMap<ExerciseId, Vec<f64>> =
        BTreeMap::from([(ExerciseId(5), vec![40.0, 50.0, 60.0])]);
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            exercise_loads: owned,
            ..input(
                Mode::Strength,
                vec![barbell_row()],
                vec![
                    wset(5, days_ago(2), 50.0, 5),
                    wset(5, days_ago(5), 50.0, 5),
                    wset(5, days_ago(9), 50.0, 5),
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
    let e = work.explanation.expect("a work item explains itself");
    assert!(e.deficit > 0.0 && e.deficit <= 1.0);
    assert!(e.recovery > 0.0 && e.recovery <= 1.0);
    assert_eq!(e.confidence, Confidence::High); // three recent sessions
    assert!(e.e1rm.unwrap() > 0.0);
    // Warm-up items (the ramp-in) carry no reasoning.
    assert!(
        out.plan
            .iter()
            .filter(|s| s.kind == SuggestionKind::Warmup)
            .all(|s| s.explanation.is_none())
    );
}

#[test]
fn a_never_done_lift_is_an_assessment_at_the_lightest_owned_weight() {
    // No history for a weighted lift → the engine can't prescribe honestly, so it
    // asks you to calibrate: one build-up set at the lightest weight you own.
    let owned: BTreeMap<ExerciseId, Vec<f64>> =
        BTreeMap::from([(ExerciseId(5), vec![10.0, 15.0, 20.0])]);
    let inp = PacingInput {
        groups: back_only(),
        exercise_loads: owned,
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
    assert_eq!(sug.ask.load_kg(), Some(10.0));
}

#[test]
fn trusted_ability_prescribes_untrusted_ability_assesses() {
    // Same lift + owned inventory. Three recent sessions → High confidence → a
    // real prescription (Work). Only a 200-day-old set → Low confidence → the
    // engine re-measures (Assess) rather than trust the stale number.
    let owned: BTreeMap<ExerciseId, Vec<f64>> =
        BTreeMap::from([(ExerciseId(5), vec![40.0, 50.0, 60.0])]);
    let mk = |hist: Vec<SetRec>| {
        let inp = PacingInput {
            groups: back_only(),
            exercise_loads: owned.clone(),
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
    let owned: BTreeMap<ExerciseId, Vec<f64>> =
        BTreeMap::from([(ExerciseId(5), vec![40.0, 45.0, 50.0, 55.0, 60.0])]);
    let mk = |r: Option<Readiness>| {
        let inp = PacingInput {
            groups: back_only(),
            exercise_loads: owned.clone(),
            readiness: r,
            ..input(
                Mode::Strength,
                vec![barbell_row()],
                vec![wset(5, days_ago(2), 55.0, 6)],
                None,
                Some(vec![3]),
            )
        };
        evaluate(&inp, now())
            .suggestion
            .unwrap()
            .ask
            .load_kg()
            .unwrap()
    };
    let normal = mk(None);
    let low = mk(Some(Readiness::of(0.2)));
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
    let low = mk(Some(Readiness::of(0.15)));
    assert!(low < normal, "low readiness {low} < normal {normal}");
}

/// Every group trained hard and recently → nothing due → Rest. `at` places the
/// work, which is what decides whether today has any training in it.
fn rested_after_training_at(at: NaiveDateTime) -> PacingNow {
    let mut h = vec![];
    for _ in 0..5 {
        h.push(set(1, at));
        h.push(set(2, at));
        h.push(set(3, at));
    }
    evaluate(&input(Mode::Balanced, catalog(), h, None, None), now())
}

#[test]
fn rest_when_everything_recovered() {
    // Trained yesterday evening, nothing due today: a rest day, and the coach
    // says so.
    let out = rested_after_training_at(hours_ago(20));
    assert_eq!(out.state, PacingState::Rest);
    assert!(out.suggestion.is_none());
    assert!(out.reason.contains("rest"), "got {:?}", out.reason);
}

#[test]
fn a_day_you_trained_closes_as_a_session_not_a_rest_day() {
    // Same Rest state, but the work happened *today*. "You're balanced and
    // recovered — rest up" is the wrong sentence to read at bedtime on a day you
    // trained: nothing is due precisely *because* you did it. The session-closing
    // line used to be gated on still being inside the session window, which
    // elapses hours before the day does.
    let out = rested_after_training_at(hours_ago(10));
    assert_eq!(out.state, PacingState::Rest);
    assert!(out.suggestion.is_none());
    assert!(
        out.reason.contains("That's the session"),
        "got {:?}",
        out.reason
    );
}

/// Steady weeks behind you, then a heavy one. `spike_from` starts the recent week
/// (0 = including today, 1 = nothing logged today).
fn spike_over_a_baseline(spike_from: i64) -> Vec<SetRec> {
    let mut h = vec![];
    // The baseline: 7 weeks at a modest 7 sets a week.
    for week in 1..8 {
        for _ in 0..7 {
            h.push(set(1, days_ago(week * 7 + 1)));
        }
    }
    // This week: three times that.
    for d in spike_from..7 {
        for _ in 0..3 {
            h.push(set(1, days_ago(d)));
        }
    }
    h
}

#[test]
fn auto_deload_when_volume_spikes() {
    // This week is far above the weeks that came before it — that, and only that,
    // is a spike. (Before, *any* history concentrated in the last 7 days tripped
    // this, because the average divided by eight weeks whether or not they existed:
    // a beginner's every week read as a spike.)
    let out = evaluate(
        &input(
            Mode::Balanced,
            catalog(),
            spike_over_a_baseline(0),
            None,
            None,
        ),
        now(),
    );
    assert!(out.deload, "a recent volume spike triggers auto-deload");
}

#[test]
fn a_first_week_of_training_is_not_a_spike() {
    // The same volume, with nothing before it. There is no baseline to spike above,
    // so the coach must not claim one — it would have told a returning athlete to
    // ease off in every session of his first two months.
    let mut h = vec![];
    for d in 0..7 {
        for _ in 0..10 {
            h.push(set(1, days_ago(d)));
        }
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert!(
        !out.deload,
        "a first week of training is not a volume spike — there's nothing to spike above"
    );
}

#[test]
fn deload_notes_the_reason() {
    // The same spike, but nothing logged today: the coach is suggesting work now,
    // so its one sentence carries the deload clause — there's no separate deload
    // widget in the UI.
    let out = evaluate(
        &input(
            Mode::Balanced,
            catalog(),
            spike_over_a_baseline(1),
            None,
            None,
        ),
        now(),
    );
    assert!(out.deload, "the spike still reads as a deload");
    assert!(out.suggestion.is_some(), "work is on offer today");
    assert!(out.reason.contains("easing off"), "reason: {}", out.reason);
}

#[test]
fn nudges_when_behind_midday() {
    // A due group + nothing done today + spacing ok → behind → nudge.
    let mut h = vec![];
    for d in 2..6 {
        h.push(set(1, days_ago(d)));
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert_eq!(out.window, WindowState::Within);
    assert!(out.spacing_ok);
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
    let high = evaluate(&mk(Readiness::of(0.9)), now());
    let low = evaluate(&mk(Readiness::of(0.2)), now());
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
    assert_eq!(high.readiness.map(|r| r.band()), Some(Band::High));
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
        readiness: Some(Readiness::of(0.9)),
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
        readiness: Some(Readiness::of(0.9)),
        ..input(Mode::Balanced, catalog(), h, None, None)
    };
    let out = evaluate(&inp, now());
    assert!(out.suggestion.is_some());
    assert!(
        out.reason.contains("train well"),
        "the high-readiness clause is carried in the reason: {}",
        out.reason
    );
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
    assert_eq!(late.window, WindowState::After);
    assert!(!late.nudge);
    assert!(late.suggestion.is_some(), "still trainable any time");
    assert!(late.reason.contains("rolls to tomorrow"));

    // Before the window's start (06:00, start=8): neutral, no nudge, no defer.
    let early = evaluate(&input(Mode::Balanced, catalog(), hist(), None, None), at(6));
    assert_eq!(early.window, WindowState::Before);
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

#[test]
fn a_lift_with_no_registered_weights_is_left_out_and_said_so() {
    // The prod shape of this: the Office kettlebell (and the Home dumbbell) are
    // listed as kit but have no weights registered, so the engine had nothing to
    // snap to — and offered a "1 kg overhead press", the lightest thing in the
    // room standing in for an unknown. There is no honest load here, so there is
    // no prescription: the lift is dropped, and the athlete is told why (they can
    // fix it by registering the weights) rather than left with a silent gap.
    let exs = vec![
        barbell_row(), // weighted, equipment 3 — present, but no weights registered
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
        // The kit is here; nothing it can be loaded with is. The service works out
        // why (no weights registered / not enough handles for a pair) and says so;
        // the engine's job is simply never to prescribe what can't be built.
        exercise_loads: BTreeMap::new(),
        equipment_names: BTreeMap::new(),
        notices: vec!["No weights registered here for Barbell.".to_string()],
        ..input(Mode::Strength, exs, vec![], None, Some(vec![3]))
    };
    let out = evaluate(&inp, now());

    assert!(
        out.plan.iter().all(|s| s.exercise_id != ExerciseId(5)),
        "a weighted lift with no registered weights is never planned"
    );
    assert!(
        out.plan.iter().any(|s| s.exercise_id == ExerciseId(2)),
        "the session still happens — on what the athlete can actually load"
    );
    assert!(
        out.notices.iter().any(|n| n.contains("Barbell")),
        "the drop is surfaced, naming the kit to fix: {:?}",
        out.notices
    );
}

#[test]
fn without_a_location_it_asks_rather_than_guesses() {
    // No location → the engine doesn't know what's doable. The old spelling
    // (`Option<BTreeSet>` consulted with `is_none_or`) made that mean "everything
    // is doable", so a missing location silently switched the safety filter off.
    // Absent kit now means absent kit: the verdict narrows to a question.
    let inp = PacingInput {
        kit: None,
        ..input(Mode::Balanced, catalog(), vec![], None, None)
    };
    let out = evaluate(&inp, now());

    assert!(out.plan.is_empty(), "no kit known → nothing is suggested");
    assert!(out.suggestion.is_none());
    assert!(!out.nudge, "and it certainly doesn't nudge you to do it");
    assert!(
        out.reason.contains("where you're training"),
        "it asks for the missing input: {:?}",
        out.reason
    );
    // The balance view is history-only, so it still stands: degradation narrows
    // the claim (no plan) without discarding what we do know.
    assert_eq!(out.groups.len(), 3);
}

// ---- loaded carries (weighted_hold) ----------------------------------------
//
// A carry is a weight *and* a time. The metric taxonomy had only `hold` (no load)
// and `weighted_reps` (no clock), so all four carries in the catalog were filed as
// weighted reps and the coach prescribed "Farmers walk (suitcase) — 5 reps at
// 6 kg". Reps are not what a carry is measured in.

/// A kettlebell carry: id 7, one implement, the gym's bells.
fn waiter_walk() -> ExerciseInfo {
    ex(
        7,
        "Farmers walk (waiter)",
        Pattern::Core,
        Metric::WeightedHold,
        false,
        vec![3],
        vec![(20, MuscleRole::Primary)],
    )
}

/// The bells at the office: 6…36 kg.
fn bells() -> BTreeMap<ExerciseId, Vec<f64>> {
    BTreeMap::from([(
        ExerciseId(7),
        vec![
            6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 24.0, 28.0, 32.0, 36.0,
        ],
    )])
}

/// A carry set: a load *and* a duration, no reps.
fn cset(exercise_id: i64, at: NaiveDateTime, load: f64, secs: i32) -> SetRec {
    SetRec {
        id: SetId(0),
        exercise_id: ExerciseId(exercise_id),
        logged_at: at,
        reps: None,
        load_kg: Some(load),
        hold_s: Some(secs),
        distance_m: None,
        rpe: None,
    }
}

fn carry_plan(history: Vec<SetRec>) -> PacingNow {
    evaluate(
        &PacingInput {
            groups: back_only(),
            exercise_loads: bells(),
            ..input(
                Mode::Balanced,
                vec![waiter_walk()],
                history,
                None,
                Some(vec![3]),
            )
        },
        now(),
    )
}

#[test]
fn a_carry_is_never_prescribed_in_reps() {
    // Three sessions → the estimate is trusted, so this is a prescription, not a
    // measurement. It must carry a weight and a duration, and no rep target at all.
    let out = carry_plan(vec![
        cset(7, days_ago(2), 12.0, 30),
        cset(7, days_ago(5), 12.0, 30),
        cset(7, days_ago(9), 12.0, 30),
    ]);
    let w = out
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Work)
        .expect("a work item for a trusted carry");
    assert!(w.ask.load_kg().is_some(), "a carry has a weight");
    assert!(w.ask.hold_s().is_some(), "a carry has a duration");
    assert_eq!(w.ask.rep_low(), None, "a carry is not measured in reps");
    assert_eq!(w.ask.rep_high(), None, "a carry is not measured in reps");
}

#[test]
fn a_carry_climbs_the_clock_then_steps_the_weight() {
    // Under the ceiling: same bell, longer walk — the load is *earned* before it
    // moves, exactly as reps are on a weighted lift.
    let climbing = carry_plan(vec![
        cset(7, days_ago(2), 12.0, 30),
        cset(7, days_ago(5), 12.0, 30),
        cset(7, days_ago(9), 12.0, 30),
        cset(7, days_ago(12), 12.0, 30),
    ]);
    let w = climbing
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Work)
        .unwrap();
    assert_eq!(
        w.ask.load_kg(),
        Some(12.0),
        "the bell holds while the clock climbs"
    );
    assert!(
        w.ask.hold_s().unwrap() > 30,
        "the walk gets longer, got {:?}",
        w.ask.hold_s()
    );

    // At the ceiling: the walk is long enough, so it's asking for a heavier bell —
    // the next one actually owned (14, not 12.7) — and the clock starts again.
    let topped = carry_plan(vec![
        cset(7, days_ago(2), 12.0, 60),
        cset(7, days_ago(5), 12.0, 60),
        cset(7, days_ago(9), 12.0, 60),
        cset(7, days_ago(12), 12.0, 60),
    ]);
    let w = topped
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Work)
        .unwrap();
    assert_eq!(
        w.ask.load_kg(),
        Some(14.0),
        "the next bell up, and one he owns"
    );
    assert_eq!(
        w.ask.hold_s(),
        Some(30),
        "the clock resets at the heavier weight"
    );
}

#[test]
fn an_unmeasured_carry_is_measured_not_guessed() {
    // No history → the engine has no idea how long he can carry what, so it must
    // ask rather than invent a duration. The weight is given (the lightest owned);
    // the *time* is the open field, because the time is the measurement.
    let out = carry_plan(vec![]);
    let a = out
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Assess)
        .expect("an untrained carry is a calibration item");
    assert_eq!(
        a.ask.load_kg(),
        Some(6.0),
        "opens at the lightest bell owned"
    );
    assert_eq!(
        a.ask.hold_s(),
        None,
        "the duration is what's being measured"
    );
    assert_eq!(a.ask.rep_low(), None, "still not reps");
}

#[test]
fn a_carry_with_no_registered_weights_is_not_prescribed() {
    // The same rule as any other loaded lift: no honest load exists, so it is left
    // out and named — never carried at a weight he might not own.
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            exercise_loads: BTreeMap::new(), // the bells aren't registered
            ..input(
                Mode::Balanced,
                vec![waiter_walk()],
                vec![],
                None,
                Some(vec![3]),
            )
        },
        now(),
    );
    assert!(
        !out.plan.iter().any(|s| s.exercise_id == ExerciseId(7)),
        "a carry with no weights registered must not be prescribed"
    );
}

// ---- the weekly rate is per week *observed*, not per week *looked at* --------

#[test]
fn a_first_session_does_not_shrink_the_day_target() {
    // The estimator divided logged sets by a flat 8 weeks whether or not eight
    // weeks of history existed. So a returning athlete's first session — 14 sets in
    // one day — read as 1.75 sets/week, and the day's target *fell* from the
    // cold-start 6 to the floor of 3: logging made the coach believe he trained
    // less than logging nothing did. An estimate must not get worse as it learns.
    let cold = evaluate(&input(Mode::Balanced, catalog(), vec![], None, None), now());

    // One honest session today: 14 sets across the catalog.
    let mut h = Vec::new();
    for _ in 0..5 {
        h.push(set(1, hours_ago(3))); // push-up
        h.push(set(2, hours_ago(3))); // ring row
    }
    h.extend([set(3, hours_ago(3)), set(3, hours_ago(3))]); // squat ×2 → 12… plus
    h.push(set(1, hours_ago(3)));
    h.push(set(2, hours_ago(3)));
    assert_eq!(h.len(), 14);

    let after = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert!(
        after.day_target_sets >= cold.day_target_sets,
        "logging a session shrank the day's target from {} to {} — the estimator \
         got worse as it learned",
        cold.day_target_sets,
        after.day_target_sets
    );
}

#[test]
fn a_settled_athletes_target_tracks_their_own_rate() {
    // Eight weeks of steady training: ~20 sets/week over 4 days → the target should
    // land near their real per-day rate (5), not at a floor or a ceiling.
    let mut h = Vec::new();
    for week in 0..8 {
        for day in [0, 2, 4, 6] {
            for _ in 0..5 {
                h.push(set(1, days_ago(week * 7 + day + 1)));
            }
        }
    }
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    assert!(
        (4..=7).contains(&out.day_target_sets),
        "a 5-sets-a-day athlete should be targeted around 5, got {}",
        out.day_target_sets
    );
}

// ---- prediction-error feedback (the residual ledger driving progression) ----
//
// Ability is a max over decayed sets, so without the ledger a miss pulls nothing
// down and the athlete is re-handed the load his last sessions already failed.
// These drive the fix end to end through `evaluate`.

/// A run of identical weighted sessions on the barbell row, one per week, newest
/// `days_ago` last. Enough distinct recent days to reach `High` confidence.
fn row_sessions(loads_by_week: &[f64]) -> Vec<SetRec> {
    let mut h = Vec::new();
    for (i, &load) in loads_by_week.iter().enumerate() {
        // Oldest first; the last entry is the most recent (2 days ago).
        let d = 2 + (loads_by_week.len() - 1 - i) as i64 * 7;
        h.push(wset(5, days_ago(d), load, 5));
    }
    h
}

fn row_plan(history: Vec<SetRec>) -> PacingNow {
    evaluate(
        &PacingInput {
            groups: back_only(),
            exercise_loads: owned(),
            ..input(
                Mode::Strength,
                vec![barbell_row()],
                history,
                None,
                Some(vec![3]),
            )
        },
        now(),
    )
}

fn row_work(out: &PacingNow) -> Option<Suggestion> {
    out.plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Work && s.exercise_id == ExerciseId(5))
        .cloned()
}

#[test]
fn two_misses_prescribe_a_lighter_load_than_a_steady_history() {
    // Steady at 60 kg → prescribed around there. Then two sessions that came in well
    // under it → the next prescription must step *down*, not re-offer 60.
    let steady = row_work(&row_plan(row_sessions(&[60.0, 60.0, 60.0]))).expect("a work item");
    let after_misses =
        row_work(&row_plan(row_sessions(&[60.0, 60.0, 45.0, 45.0]))).expect("a work item");
    assert!(
        after_misses.ask.load_kg().unwrap() < steady.ask.load_kg().unwrap(),
        "two misses should back the load off: steady {:?} vs after-misses {:?}",
        steady.ask.load_kg(),
        after_misses.ask.load_kg()
    );
    // ...and it says why, so "eased off" reads as a decision rather than a glitch.
    assert_eq!(after_misses.explanation.map(|e| e.misses), Some(2));
}

#[test]
fn three_misses_send_a_trusted_lift_back_to_calibration() {
    // High confidence — normally prescribed — but the estimate has been wrong three
    // sessions running. That is a wrong number, not a bad week, so the engine stops
    // prescribing from it and measures instead.
    let out = row_plan(row_sessions(&[60.0, 60.0, 45.0, 45.0, 45.0]));
    let item = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(5))
        .expect("the row is still planned");
    assert_eq!(
        item.kind,
        SuggestionKind::Assess,
        "three misses running re-open the measurement"
    );
}

#[test]
fn a_steady_history_still_prescribes_work_at_its_level() {
    // The control: no misses, so nothing about the feedback path fires and the lift
    // is prescribed as work, near the demonstrated e1RM.
    let out = row_plan(row_sessions(&[60.0, 60.0, 60.0]));
    let w = row_work(&out).expect("a work item");
    assert!(
        w.ask.load_kg().unwrap() >= 50.0,
        "prescribed near his level, got {:?}",
        w.ask.load_kg()
    );
    assert_eq!(w.explanation.map(|e| e.misses), Some(0));
}

// ---- round 2: the coach judged against a human one (docs/field-test.md R2-*) ----

/// Custom groups for round-2 scenarios (the fixed `groups()` trio is too small
/// to shape a compound). Ids mirror nothing; names are what assertions read.
fn r2_groups() -> Vec<GroupMeta> {
    vec![
        GroupMeta {
            id: GroupId(10),
            name: "Chest".into(),
            region: Region::Chest,
        },
        GroupMeta {
            id: GroupId(20),
            name: "Lats".into(),
            region: Region::Back,
        },
        GroupMeta {
            id: GroupId(30),
            name: "Quadriceps".into(),
            region: Region::Legs,
        },
        GroupMeta {
            id: GroupId(40),
            name: "Biceps".into(),
            region: Region::Arms,
        },
        GroupMeta {
            id: GroupId(50),
            name: "Upper back".into(),
            region: Region::Back,
        },
        GroupMeta {
            id: GroupId(60),
            name: "Biceps brachii".into(),
            region: Region::Arms,
        },
        GroupMeta {
            id: GroupId(65),
            name: "Forearms".into(),
            region: Region::Forearms,
        },
    ]
}

/// A pull-up shaped like the real catalog at group level: one primary, a spread
/// of secondaries — a compound by breadth (≥3 non-stabilizer groups).
fn r2_pullup() -> ExerciseInfo {
    ex(
        7,
        "Pull-up",
        Pattern::Pull,
        Metric::Reps,
        false,
        vec![],
        vec![
            (20, MuscleRole::Primary),
            (40, MuscleRole::Secondary),
            (50, MuscleRole::Secondary),
        ],
    )
}

/// A biceps curl shaped like the real one: two non-stabilizer groups — an
/// isolation, even though it's weighted. Its groups are disjoint from
/// [`r2_pullup`]'s so the cover plans both on a small budget (a shared group
/// would leave the second pick under `MIN_PAY` — beside the ordering point).
fn r2_curl() -> ExerciseInfo {
    ex(
        9,
        "Biceps curl",
        Pattern::Pull,
        Metric::WeightedReps,
        false,
        vec![3],
        vec![(60, MuscleRole::Primary), (65, MuscleRole::Secondary)],
    )
}

// R2-2: one plan, one bookkeeping rule. The header sums the plan's own cards
// (sets and done both), so finishing exactly the plan reads N/N — not 14/13
// with two mobility drills missing from the numerator, and not 1/13 while
// three cards show Done. The engine's part of that contract: attributed
// progress over the whole plan, warm-ups included, capped per item.
#[test]
fn finishing_exactly_the_plan_reads_complete() {
    let exercises = vec![catalog().remove(0), warmup_ex(100, "Chest opener", 10)];
    let cold = evaluate(
        &input(Mode::Balanced, exercises.clone(), vec![], None, None),
        now(),
    );
    let plan_sets: i32 = cold.plan.iter().map(|s| s.sets).sum();
    assert!(plan_sets > 0, "a cold start still plans a session");
    assert!(
        cold.plan.iter().any(|s| s.kind == SuggestionKind::Warmup),
        "precondition: the plan has a warm-up to count"
    );

    // Do exactly the plan — every item, warm-ups included, minutes apart —
    // plus one extra "log another" set, which must not overflow its card.
    let mut h = Vec::new();
    let mut t = 40i64;
    for item in &cold.plan {
        for _ in 0..item.sets {
            h.push(bset(item.exercise_id.get(), minutes_ago(t), 8));
            t -= 2;
        }
    }
    h.push(bset(
        cold.plan.last().unwrap().exercise_id.get(),
        minutes_ago(t),
        8,
    ));
    let done = evaluate(&input(Mode::Balanced, exercises, h, None, None), now());
    let (sets, counted): (i32, i32) = (
        done.plan.iter().map(|s| s.sets).sum(),
        done.plan.iter().map(|s| s.done()).sum(),
    );
    assert_eq!(
        counted,
        sets,
        "finishing the plan is N of N (warm-ups counted, extras capped): {:?}",
        done.plan
            .iter()
            .map(|s| (&s.exercise_name, s.done(), s.sets))
            .collect::<Vec<_>>()
    );
}

// R2-3a: no two warm-up slots spent on the same muscle group — and each card's
// label names the group the drill is there for, not whichever primary sorts
// first (that's how two cards both read "loosen up Obliques").
#[test]
fn warmup_labels_name_distinct_groups() {
    let mut exercises = vec![
        // Work on chest and lats.
        catalog().remove(0), // Push-up (chest)
        r2_pullup(),
        // Drill A warms chest only; drill B warms chest *and* lats — under the
        // old rule B was included for lats but labelled chest, same as A.
        warmup_ex(100, "Chest opener", 10),
        ExerciseInfo {
            id: ExerciseId(101),
            name: "Reach and roll".into(),
            family: "Reach and roll".into(),
            difficulty: None,
            pattern: Pattern::Core,
            metric: Metric::Reps,
            is_skill: false,
            is_power: false,
            warmup: true,
            equipment: vec![],
            groups: vec![
                (GroupId(10), MuscleRole::Primary),
                (GroupId(20), MuscleRole::Primary),
            ],
        },
    ];
    exercises.rotate_left(0);
    let out = evaluate(
        &PacingInput {
            groups: r2_groups(),
            ..input(Mode::Balanced, exercises, vec![], None, None)
        },
        now(),
    );
    let labels: Vec<&str> = out
        .plan
        .iter()
        .filter(|s| s.kind == SuggestionKind::Warmup)
        .map(|s| s.group.as_str())
        .collect();
    assert!(
        !labels.is_empty(),
        "a session with drills available has a warm-up"
    );
    let mut dedup = labels.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(
        dedup.len(),
        labels.len(),
        "each warm-up slot preps its own group, got {labels:?}"
    );
}

// R2-3b: the warm-up preps what the session actually loads, heaviest first —
// including groups the work only hits as secondaries. Two of three slots on
// obliques while dips/pull-ups/push-ups went in cold is the bug this pins.
#[test]
fn the_warmup_preps_the_sessions_heaviest_groups_first() {
    // Chest work is trusted (2 sets); quads work is a 1-set calibration. Chest
    // carries more of the session, so its drill must lead — even though the
    // quads drill has the lower exercise id (the old order).
    let mut h = Vec::new();
    for d in [2, 4, 9] {
        h.push(bset(1, days_ago(d), 10)); // push-up: trusted chest work
    }
    let exercises = vec![
        catalog().remove(0),               // Push-up (chest, trusted)
        catalog().remove(2),               // Squat (quads, never done → calibration)
        warmup_ex(90, "Leg swings", 30),   // quads drill, lower id
        warmup_ex(95, "Chest opener", 10), // chest drill, higher id
    ];
    let out = evaluate(
        &PacingInput {
            groups: r2_groups(),
            ..input(Mode::Balanced, exercises, h, None, None)
        },
        now(),
    );
    let warmups: Vec<&Suggestion> = out
        .plan
        .iter()
        .filter(|s| s.kind == SuggestionKind::Warmup)
        .collect();
    assert!(
        warmups.len() >= 2,
        "both groups have drills: {:?}",
        out.plan
    );
    assert_eq!(
        warmups[0].group,
        "Chest",
        "the heavier-loaded group warms up first, got {:?}",
        warmups.iter().map(|s| &s.group).collect::<Vec<_>>()
    );
}

// R2-3c: a group the session hammers as a *secondary* still gets its warm-up —
// coverage follows the plan's load, not just its primary labels.
#[test]
fn a_secondary_group_under_real_load_gets_warmed_too() {
    let mut h = Vec::new();
    for d in [2, 4, 9] {
        h.push(bset(7, days_ago(d), 8)); // pull-up trusted → 2 work sets
    }
    let exercises = vec![
        r2_pullup(), // biceps secondary, 2 sets → load 1.0
        warmup_ex(90, "Lat opener", 20),
        warmup_ex(91, "Biceps opener", 40),
    ];
    let out = evaluate(
        &PacingInput {
            groups: r2_groups(),
            ..input(Mode::Balanced, exercises, h, None, None)
        },
        now(),
    );
    assert!(
        out.plan
            .iter()
            .any(|s| s.kind == SuggestionKind::Warmup && s.group == "Biceps"),
        "two sets of pull-ups load the biceps enough to deserve prep: {:?}",
        out.plan
            .iter()
            .map(|s| (&s.exercise_name, &s.group))
            .collect::<Vec<_>>()
    );
}

// The warm-up is sized to the session it precedes. A broad compound pushes five
// groups past the warm-up threshold, but two committed work sets don't earn five
// drills — the block keeps to the heaviest-loaded groups and leaves the tail to
// general movement and ramp-ins. (With a gap-free drill catalog, an unbounded
// block reached 11 drills before 9 working sets — a warm-up longer than the
// session it warmed up for.)
#[test]
fn a_short_session_earns_a_short_warmup() {
    let mut h = Vec::new();
    for d in [2, 4, 9] {
        h.push(bset(7, days_ago(d), 8)); // trusted → full work dose, no calibration
    }
    let exercises = vec![
        ex(
            7,
            "Wide row",
            Pattern::Pull,
            Metric::Reps,
            false,
            vec![],
            vec![
                (20, MuscleRole::Primary),
                (40, MuscleRole::Secondary),
                (50, MuscleRole::Secondary),
                (60, MuscleRole::Secondary),
                (65, MuscleRole::Secondary),
            ],
        ),
        warmup_ex(90, "Lat opener", 20),
        warmup_ex(91, "Biceps opener", 40),
        warmup_ex(92, "Scap opener", 50),
        warmup_ex(93, "Curl opener", 60),
        warmup_ex(94, "Wrist opener", 65),
    ];
    let out = evaluate(
        &PacingInput {
            groups: r2_groups(),
            ..input(Mode::Balanced, exercises, h, None, None)
        },
        now(),
    );
    let work_sets: i32 = out
        .plan
        .iter()
        .filter(|s| s.kind != SuggestionKind::Warmup)
        .map(|s| s.sets)
        .sum();
    assert!(work_sets <= 6, "precondition: a genuinely short session");
    let labels: Vec<&str> = out
        .plan
        .iter()
        .filter(|s| s.kind == SuggestionKind::Warmup)
        .map(|s| s.group.as_str())
        .collect();
    assert_eq!(
        labels,
        vec!["Lats", "Biceps", "Upper back"],
        "the heaviest-loaded groups get the drills; the tail is triage, not a gap"
    );
}

// R2-4: compounds run before the isolations that would pre-fatigue them. A
// weighted curl before bodyweight pull-ups is the sequencing error this pins —
// "weighted" is not what makes a movement lead a session.
#[test]
fn compounds_run_before_isolations() {
    // Four training days → a day target of 4, so both movements fit at their
    // full 2-set minimum (a 3-set budget would rightly refuse the second a
    // 1-set orphan entry — see the cover's remainder rule).
    let mut h = Vec::new();
    for d in [2, 4, 6, 9] {
        h.push(bset(7, days_ago(d), 8)); // pull-up trusted
        h.push(wset(9, days_ago(d), 8.0, 10)); // curl trusted
    }
    let out = evaluate(
        &PacingInput {
            groups: r2_groups(),
            exercise_loads: BTreeMap::from([(ExerciseId(9), vec![6.0, 8.0, 10.0])]),
            // Steady readiness: the 9-day fixture otherwise trips the
            // volume-spike deload proxy and shrinks the budget below two
            // full doses.
            readiness: Some(Readiness::of(0.5)),
            ..input(
                Mode::Balanced,
                vec![r2_pullup(), r2_curl()],
                h,
                None,
                Some(vec![3]),
            )
        },
        now(),
    );
    // Positions among the *work* items — the curl's ramp-in warm-up shares its
    // exercise id and rightly leads the whole plan.
    let work: Vec<&Suggestion> = out
        .plan
        .iter()
        .filter(|s| s.kind != SuggestionKind::Warmup)
        .collect();
    let pos = |id: ExerciseId| work.iter().position(|s| s.exercise_id == id);
    let (Some(pull), Some(curl)) = (pos(ExerciseId(7)), pos(ExerciseId(9))) else {
        panic!("both movements planned: {:?}", out.plan);
    };
    assert!(
        pull < curl,
        "the compound leads; curling to failure first makes the pull-up read weak"
    );
}

// R2-5a: mobility drills need no rest gate — "Rest a moment" straight after arm
// circles teaches the athlete to ignore the banner.
#[test]
fn no_rest_prompt_after_a_mobility_drill() {
    let exercises = vec![catalog().remove(0), warmup_ex(100, "Chest opener", 10)];
    // The drill just logged, one minute ago (min_rest_min is 20).
    let h = vec![bset(100, minutes_ago(1), 10)];
    let out = evaluate(&input(Mode::Balanced, exercises, h, None, None), now());
    assert!(
        out.spacing_ok,
        "a warm-up set doesn't start a rest clock: {:?}",
        out.reason
    );
    assert!(
        !out.reason.starts_with("Rest"),
        "no rest prompt after prep: {:?}",
        out.reason
    );
}

// R2-5b: when a rest *is* called for, it has a length — matched to how big the
// movement just done was.
#[test]
fn a_rest_prompt_says_how_long() {
    let mut trusted = Vec::new();
    for d in [2, 4, 9] {
        trusted.push(bset(7, days_ago(d), 8));
        trusted.push(wset(9, days_ago(d), 8.0, 10));
    }
    let base = |h: Vec<SetRec>| PacingInput {
        groups: r2_groups(),
        exercise_loads: BTreeMap::from([(ExerciseId(9), vec![6.0, 8.0, 10.0])]),
        ..input(
            Mode::Balanced,
            vec![r2_pullup(), r2_curl()],
            h,
            None,
            Some(vec![3]),
        )
    };
    // Just finished a compound set → the longer rest.
    let mut h = trusted.clone();
    h.push(bset(7, minutes_ago(1), 8));
    let after_compound = evaluate(&base(h), now());
    assert!(
        after_compound.reason.contains("2–3 min"),
        "a compound set earns the long rest: {:?}",
        after_compound.reason
    );
    // Just finished an isolation set → the shorter one.
    let mut h = trusted;
    h.push(wset(9, minutes_ago(1), 8.0, 10));
    let after_isolation = evaluate(&base(h), now());
    assert!(
        after_isolation.reason.contains("90 s"),
        "an isolation set needs only the short rest: {:?}",
        after_isolation.reason
    );
}

// R2-7: one notion of "next". When the next unfinished item is a warm-up, the
// banner says so — it doesn't name the dips the athlete isn't supposed to be
// doing yet while the pill points at a mobility drill.
#[test]
fn the_banner_names_the_warmup_when_thats_next() {
    let exercises = vec![catalog().remove(0), warmup_ex(100, "Chest opener", 10)];
    let out = evaluate(&input(Mode::Balanced, exercises, vec![], None, None), now());
    assert_eq!(
        out.plan.first().map(|s| s.kind),
        Some(SuggestionKind::Warmup),
        "precondition: the plan opens with a warm-up"
    );
    assert!(
        out.reason.contains("Warm up first") && out.reason.contains("Chest opener"),
        "the banner and the plan agree on what's next: {:?}",
        out.reason
    );
}

// R2-8: the card's headline muscle is a prime mover. Dips read "(Serratus)"
// because the label chased the neediest group it touched at all; a coach names
// what the movement *is*.
#[test]
fn an_items_label_is_a_prime_mover() {
    // Chest hammered via a chest-only movement (need ~0); lats untrained (max
    // need). The hybrid below trains chest as PRIMARY and lats only as
    // SECONDARY — it gets picked *for* the lats need, but its headline must
    // still be the prime mover. The old label logic said "Lats".
    let mut h = Vec::new();
    for d in [1, 2, 3] {
        for _ in 0..2 {
            h.push(bset(6, days_ago(d), 10));
        }
    }
    let hammer = ex(
        6,
        "Chest press",
        Pattern::Push,
        Metric::Reps,
        false,
        vec![],
        vec![(10, MuscleRole::Primary)],
    );
    let hybrid = ex(
        8,
        "Weird press",
        Pattern::Push,
        Metric::Reps,
        false,
        vec![],
        vec![(10, MuscleRole::Primary), (20, MuscleRole::Secondary)],
    );
    let out = evaluate(
        &PacingInput {
            groups: r2_groups(),
            ..input(Mode::Balanced, vec![hammer, hybrid], h, None, None)
        },
        now(),
    );
    let item = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(8))
        .expect("the hybrid is planned (lats need pays for it)");
    assert_eq!(
        item.group, "Chest",
        "the headline is the prime mover, not the neediest synergist"
    );
}

// ---- round 4: the simulated-athlete findings --------------------------------
//
// E3 (src/bin/simulate.rs) played a deterministic athlete against the engine for
// eight simulated weeks and surfaced three coaching failures the back-test could
// never show (they only exist in the loop the engine's own prescriptions create):
// a failing +1 re-asked verbatim every session for weeks, a topped-out or
// plateaued movement prescribed against its wall forever, and near-duplicate
// movements (hamstring-curl cousins) sharing one session. These tests pin the
// fixes.

// R4-1: the +1 ask is a probe, and a probe is earned. An athlete who matches
// their best while failing the ask keeps the same estimate (ability is a max),
// so before this the coach re-asked best+1 every single session — grinding, not
// coaching. Between probes the ask consolidates at the demonstrated best.
#[test]
fn a_failed_probe_earns_consolidation_not_a_daily_regrind() {
    let row = || catalog().remove(1); // Ring row — bodyweight, Lats
    // Three sessions at a steady 10 reps: High confidence, two quiet outcomes —
    // mid-cadence, so today consolidates.
    let history = vec![
        bset(2, days_ago(8), 10),
        bset(2, days_ago(6), 10),
        bset(2, days_ago(4), 10),
    ];
    let out = evaluate(
        &input(Mode::Balanced, vec![row()], history, None, None),
        now(),
    );
    let item = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(2))
        .expect("the row is planned");
    assert_eq!(
        item.ask.rep_low(),
        Some(10),
        "between probes the ask is the demonstrated best, not best+1"
    );

    // A fourth identical session: the third quiet outcome earns the next probe.
    let history = vec![
        bset(2, days_ago(8), 10),
        bset(2, days_ago(6), 10),
        bset(2, days_ago(4), 10),
        bset(2, days_ago(2), 10),
    ];
    let out = evaluate(
        &input(Mode::Balanced, vec![row()], history, None, None),
        now(),
    );
    let item = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(2))
        .expect("the row is planned");
    assert_eq!(
        item.ask.rep_low(),
        Some(11),
        "the periodic probe still reaches"
    );
}

/// The hamstring-curl pair from the real catalog: same family, same primaries,
/// the single-leg variant one difficulty rung up.
fn curls_pair() -> (ExerciseInfo, ExerciseInfo) {
    let mut easy = ex(
        7,
        "Hamstring curls",
        Pattern::Legs,
        Metric::Reps,
        false,
        vec![],
        vec![(30, MuscleRole::Primary)],
    );
    easy.difficulty = Some(2);
    let mut hard = ex(
        8,
        "Hamstring curls (single leg)",
        Pattern::Legs,
        Metric::Reps,
        false,
        vec![],
        vec![(30, MuscleRole::Primary)],
    );
    hard.family = "Hamstring curls".into();
    hard.difficulty = Some(3);
    (easy, hard)
}

// R4-2 (G7): a month of steady sessions with nothing beaten is a plateau — this
// movement has stopped producing progress, and prescribing it again is
// prescribing the wall. The coach steps the athlete up the variation ladder:
// the plateaued rung leaves the session, the next-harder sibling is measured
// (never guessed at), and the step is said out loud.
#[test]
fn a_plateaued_movement_hands_over_to_its_harder_sibling() {
    let (easy, hard) = curls_pair();
    // Twice-ish a week for a month, always the same 10 reps: High confidence,
    // a window full of quiet outcomes, no slump.
    let history: Vec<SetRec> = (0..10).map(|i| bset(7, days_ago(30 - i * 3), 10)).collect();
    let out = evaluate(
        &input(Mode::Balanced, vec![easy, hard], history, None, None),
        now(),
    );
    assert!(
        out.plan.iter().all(|s| s.exercise_id != ExerciseId(7)),
        "the plateaued rung steps aside"
    );
    let item = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(8))
        .expect("the harder sibling takes the slot");
    assert_eq!(
        item.kind,
        SuggestionKind::Assess,
        "a new variation is measured, not guessed at"
    );
    assert!(
        out.notices
            .iter()
            .any(|n| n.contains("Hamstring curls (single leg)")),
        "the step up is said, not implied: {:?}",
        out.notices
    );
}

// G7 down-ladder: a movement stuck below the range floor is too hard to build
// reps on — grinding 3s forever banks no volume — so it steps back to its hardest
// easier sibling, which the athlete can actually progress. The mirror of the
// hand-off above; without it, coach could only ask best−1 or hold, and never
// regress the movement the way a coach would.
#[test]
fn a_movement_too_hard_to_build_steps_down_to_an_easier_sibling() {
    // A harder rung (8, difficulty 3) and its easier sibling (7, difficulty 2),
    // same pattern + shared primary, both bodyweight. A month of sessions pinned at
    // 3 clean reps — well under the Balanced floor of 8 — is High confidence and a
    // plateau: stuck below the floor, not a bad week.
    let mut easy = ex(
        7,
        "Push-up",
        Pattern::Push,
        Metric::Reps,
        false,
        vec![],
        vec![(10, MuscleRole::Primary)],
    );
    easy.difficulty = Some(2);
    let mut hard = ex(
        8,
        "Dips",
        Pattern::Push,
        Metric::Reps,
        false,
        vec![],
        vec![(10, MuscleRole::Primary)],
    );
    hard.difficulty = Some(3);
    let history: Vec<SetRec> = (0..10).map(|i| bset(8, days_ago(30 - i * 3), 3)).collect();
    let out = evaluate(
        &input(Mode::Balanced, vec![easy, hard], history, None, None),
        now(),
    );
    assert!(
        out.plan.iter().all(|s| s.exercise_id != ExerciseId(8)),
        "the too-hard rung steps aside: {:?}",
        out.plan.iter().map(|s| s.exercise_id).collect::<Vec<_>>()
    );
    let item = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(7))
        .expect("the easier sibling takes the slot");
    assert_eq!(
        item.kind,
        SuggestionKind::Assess,
        "the regression is measured, not guessed at"
    );
    assert!(
        out.notices.iter().any(|n| n.contains("too hard")),
        "the step back is said, not implied: {:?}",
        out.notices
    );
}

// R5-3 (supersedes R4-3): a coarse rack must not manufacture a miss. With bells
// at 4 and 5 kg and an estimate measured at 5 kg, the computed working load lands
// between rungs. Round 4 answered that by rounding the *load* up, so the rep range
// could demonstrate the estimate — but that was treating a symptom: the misses came
// from the ledger judging sessions against the athlete's ceiling instead of against
// the ask. The ask is now reconstructed at the load actually used, so the nearest
// rung is judged honestly — and it is also the better prescription, because rounding
// up asked for reps below the mode's range and made the load oscillate between two
// rungs session after session. What matters is the consequence, so that is what this
// asserts: do what the card says, and the ledger holds nothing against you.
#[test]
fn a_coarse_rack_does_not_manufacture_a_miss() {
    let h = vec![wset(5, days_ago(2), 5.0, 5)];
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            exercise_loads: BTreeMap::from([(ExerciseId(5), vec![4.0, 5.0])]),
            ..input(Mode::Balanced, vec![barbell_row()], h.clone(), None, None)
        },
        now(),
    );
    let w = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(5) && s.kind == SuggestionKind::Work)
        .expect("a trusted row in deficit is prescribed");
    assert_eq!(
        w.ask.load_kg(),
        Some(4.0),
        "the nearest rung — the one whose reps land inside the mode's range"
    );
    let asked = w.ask.rep_low().expect("a weighted ask carries reps");
    assert!(
        (6..=10).contains(&asked),
        "the ask stays inside the Balanced range, got {asked}"
    );

    let mut done = h;
    done.push(wset(5, now(), w.ask.load_kg().unwrap(), asked));
    // The same rack the engine planned against — the ledger reconstructs the ask as
    // a weight off it, so handing it a different one would judge against a card that
    // was never written.
    let rack = BTreeMap::from([(ExerciseId(5), vec![4.0, 5.0])]);
    let led = coach::pacing::residual::residuals(&done, Mode::Balanced, &Default::default(), &rack)
        .remove(&ExerciseId(5))
        .unwrap_or_default();
    assert_eq!(
        led.consecutive_misses, 0,
        "doing exactly what was asked, at a weight he owns, is never a failure"
    );
}

// R4-2 (G7): topping out the rep range is the same wall reached sooner — the
// ask is clamped at the range top, so "keep doing 12s" would be forever. The
// ladder fires on the ceiling itself, without waiting a month.
#[test]
fn a_topped_out_movement_hands_over_even_while_meeting_it() {
    let (easy, hard) = curls_pair();
    // Three recent sessions at the Balanced bodyweight range top (12).
    let history = vec![
        bset(7, days_ago(8), 12),
        bset(7, days_ago(6), 12),
        bset(7, days_ago(4), 12),
    ];
    let out = evaluate(
        &input(Mode::Balanced, vec![easy, hard], history, None, None),
        now(),
    );
    assert!(
        out.plan.iter().all(|s| s.exercise_id != ExerciseId(7)),
        "the range top is a wall, not a target to re-serve"
    );
    assert!(
        out.plan.iter().any(|s| s.exercise_id == ExerciseId(8)),
        "the harder sibling takes the slot"
    );
}

// ---- round 5: the ledger judges the ask, not the ceiling --------------------
//
// Playing full sessions through `evaluate` and feeding the results back to the
// ledger caught it marking *its own easing* as the athlete's failure. Both tests
// below drive the real loop — read the card, do exactly what it says, ask the
// ledger what it made of that — because the bug only exists in the seam between
// the two, and neither side looks wrong alone.

/// Do precisely what the card says, and hand the ledger the result.
fn comply(mut h: Vec<SetRec>, inp: &PacingInput) -> coach::pacing::residual::Residual {
    let out = evaluate(inp, now());
    let w = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(5) && s.kind == SuggestionKind::Work)
        .expect("a trusted lift in deficit is prescribed, not assessed");
    h.push(wset(
        5,
        now(),
        w.ask.load_kg().expect("a weighted lift has a load"),
        w.ask.rep_low().expect("and a rep target"),
    ));
    coach::pacing::residual::residuals(&h, Mode::Strength, &Default::default(), &owned())
        .remove(&ExerciseId(5))
        .unwrap_or_default()
}

fn strength_row(h: Vec<SetRec>) -> PacingInput {
    PacingInput {
        groups: back_only(),
        exercise_loads: BTreeMap::from([(
            ExerciseId(5),
            (8..=40).map(|i| i as f64 * 2.5).collect(),
        )]),
        ..input(Mode::Strength, vec![barbell_row()], h, None, Some(vec![3]))
    }
}

// R5-1: two real misses ease the ask down a rung to rebuild from. Meeting that
// eased ask is the athlete doing exactly what was asked — it must *clear* the
// streak. Judged against the ceiling instead, it read as miss number three and
// tripped the re-measure, so "back off and rebuild" fed itself: every genuine
// slump ended in calibration, and the rung it backed off to was never given the
// chance to prove anything.
#[test]
fn meeting_the_backed_off_ask_rebuilds_instead_of_escalating() {
    let h = vec![
        wset(5, days_ago(8), 40.0, 5),
        wset(5, days_ago(6), 40.0, 5),
        wset(5, days_ago(4), 40.0, 5),
        wset(5, days_ago(2), 30.0, 5), // a real miss
        wset(5, days_ago(1), 30.0, 5), // and another — the coach eases off
    ];
    let before =
        coach::pacing::residual::residuals(&h, Mode::Strength, &Default::default(), &owned())
            .remove(&ExerciseId(5))
            .unwrap_or_default();
    assert_eq!(before.consecutive_misses, 2, "two genuine misses");
    assert!(before.wants_back_off() && !before.wants_remeasure());

    let led = comply(h.clone(), &strength_row(h));
    assert_eq!(
        led.outcomes.last().map(|(_, o)| *o),
        Some(coach::pacing::residual::Outcome::Met),
        "doing exactly what was asked is not a failure"
    );
    assert_eq!(
        led.consecutive_misses, 0,
        "the streak clears — this is the rebuild"
    );
    assert!(
        !led.wants_remeasure(),
        "the coach's own back-off is not evidence against the estimate"
    );
}

// R5-2: a genuine shortfall against an eased ask still counts. The fix must not
// buy its calm by going deaf — falling short of a *reduced* number is the
// clearest evidence yet that the estimate is wrong.
#[test]
fn falling_short_of_an_eased_ask_still_counts_against_the_estimate() {
    let mut h = vec![
        wset(5, days_ago(8), 40.0, 5),
        wset(5, days_ago(6), 40.0, 5),
        wset(5, days_ago(4), 40.0, 5),
        wset(5, days_ago(2), 30.0, 5),
        wset(5, days_ago(1), 30.0, 5),
    ];
    let out = evaluate(&strength_row(h.clone()), now());
    let w = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(5) && s.kind == SuggestionKind::Work)
        .unwrap();
    // Two reps short of the eased ask.
    h.push(wset(
        5,
        now(),
        w.ask.load_kg().unwrap(),
        w.ask.rep_low().unwrap() - 2,
    ));
    let led = coach::pacing::residual::residuals(&h, Mode::Strength, &Default::default(), &owned())
        .remove(&ExerciseId(5))
        .unwrap_or_default();
    assert_eq!(
        led.consecutive_misses, 3,
        "a real shortfall still escalates"
    );
    assert!(
        led.wants_remeasure(),
        "three real misses running is a wrong estimate — go and measure it"
    );
}

// R5-2: the other half of the ask. A low-readiness morning makes the coach ask for
// less — and until the ledger could see that, doing exactly what it asked on a
// badly-slept day was recorded as the athlete falling short, which then held their
// progression back for having slept badly. Readiness isn't in the set history (it
// lives in health-sync), so the ledger is told what the coach knew that morning.
#[test]
fn an_eased_day_is_not_recorded_as_a_failure() {
    let h = vec![
        wset(5, days_ago(8), 40.0, 5),
        wset(5, days_ago(6), 40.0, 5),
        wset(5, days_ago(4), 40.0, 5),
    ];
    let spent = Readiness::of(0.2);
    let out = evaluate(
        &PacingInput {
            readiness: Some(spent),
            ..strength_row(h.clone())
        },
        now(),
    );
    let w = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(5) && s.kind == SuggestionKind::Work)
        .expect("a trusted lift is still prescribed on a bad day, just lighter");

    // Do precisely what the eased card said.
    let mut done = h;
    done.push(wset(
        5,
        now(),
        w.ask.load_kg().unwrap(),
        w.ask.rep_low().unwrap(),
    ));

    // Judged as though it were a full-effort day, this reads as a failure...
    let blind =
        coach::pacing::residual::residuals(&done, Mode::Strength, &Default::default(), &owned())
            .remove(&ExerciseId(5))
            .unwrap_or_default();
    assert_eq!(
        blind.consecutive_misses, 1,
        "precondition: without knowing the day was eased, compliance looks like a miss"
    );

    // ...but told what the coach knew that morning, it reads as what it was.
    let known = BTreeMap::from([(now().date(), spent)]);
    let led = coach::pacing::residual::residuals(&done, Mode::Strength, &known, &owned())
        .remove(&ExerciseId(5))
        .unwrap_or_default();
    assert_eq!(
        led.outcomes.last().map(|(_, o)| *o),
        Some(coach::pacing::residual::Outcome::Met),
        "a badly-slept night is not a failure to hold against him"
    );
    assert_eq!(led.consecutive_misses, 0);
}

// The mirror: a day health can't answer for must not invent an easing that didn't
// happen. An absent reading means full-effort — the same judgment the ledger made
// before health could be asked at all.
#[test]
fn an_unknown_days_readiness_is_not_treated_as_an_easing() {
    let h = vec![
        wset(5, days_ago(8), 40.0, 5),
        wset(5, days_ago(6), 40.0, 5),
        wset(5, days_ago(4), 40.0, 5),
    ];
    let out = evaluate(&strength_row(h.clone()), now());
    let w = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(5) && s.kind == SuggestionKind::Work)
        .unwrap();
    // Two reps short of a full-effort ask, on a day health knows nothing about.
    let mut done = h;
    done.push(wset(
        5,
        now(),
        w.ask.load_kg().unwrap(),
        w.ask.rep_low().unwrap() - 2,
    ));
    let led =
        coach::pacing::residual::residuals(&done, Mode::Strength, &Default::default(), &owned())
            .remove(&ExerciseId(5))
            .unwrap_or_default();
    assert_eq!(
        led.consecutive_misses, 1,
        "no biometrics is not an excuse the coach invents on his behalf"
    );
}

// ---- carries measured in metres (weighted_distance) -------------------------
//
// The distance carry is the metre twin of the timed one, and its ladder is the
// same shape: hold the weight and climb the distance, then take the next weight
// up and start the distance again. These assert the ladder rather than the
// wording, so they read the `Ask` the engine actually produced.

/// A carry logged at `metres` under `load`, `days_ago`.
fn carry_set(exercise_id: i64, days_ago: i64, load: f64, metres: i32) -> SetRec {
    SetRec {
        id: SetId(0),
        exercise_id: ExerciseId(exercise_id),
        logged_at: days_ago_dt(days_ago),
        reps: None,
        load_kg: Some(load),
        hold_s: None,
        distance_m: Some(metres),
        rpe: None,
    }
}

fn days_ago_dt(d: i64) -> NaiveDateTime {
    days_ago(d)
}

/// A distance carry on Lats, doable with kit 3, with a rack of bells.
fn carry_catalog() -> Vec<ExerciseInfo> {
    vec![ex(
        5,
        "Farmers walk",
        Pattern::Pull,
        Metric::WeightedDistance,
        false,
        vec![3],
        vec![(20, MuscleRole::Primary)],
    )]
}

fn carry_input(history: Vec<SetRec>) -> PacingInput {
    PacingInput {
        groups: back_only(),
        exercise_loads: BTreeMap::from([(ExerciseId(5), vec![12.0, 16.0, 20.0, 24.0])]),
        ..input(
            Mode::Balanced,
            carry_catalog(),
            history,
            None,
            Some(vec![3]),
        )
    }
}

/// The ask for the carry in today's plan, whatever else is in it.
fn carry_ask(out: &PacingNow) -> coach::pacing::types::Ask {
    out.plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(5) && s.kind != SuggestionKind::Warmup)
        .expect("the carry is planned")
        .ask
}

#[test]
fn a_carry_is_asked_for_in_metres_not_seconds() {
    // Three sessions at 12 kg × 10 m — enough to trust the estimate.
    let h = vec![
        carry_set(5, 9, 12.0, 10),
        carry_set(5, 6, 12.0, 10),
        carry_set(5, 3, 12.0, 10),
    ];
    let out = evaluate(&carry_input(h), now());
    match carry_ask(&out) {
        coach::pacing::types::Ask::WeightedDistance {
            load_kg,
            distance_m,
        } => {
            assert_eq!(load_kg, 12.0, "it holds the rung it has been working");
            assert!(
                distance_m >= 10,
                "the distance climbs or holds, never shrinks on a good day: {distance_m}"
            );
        }
        other => panic!("a distance carry must be asked for in metres, got {other:?}"),
    }
}

#[test]
fn topping_the_distance_takes_the_next_bell_and_restarts_it() {
    // At the ceiling (30 m) on 12 kg: the weight steps and the distance resets,
    // which is the whole point of a double progression — you do not carry ever
    // further at a weight that has stopped being hard.
    let h = vec![
        carry_set(5, 9, 12.0, 30),
        carry_set(5, 6, 12.0, 30),
        carry_set(5, 3, 12.0, 30),
    ];
    let out = evaluate(&carry_input(h), now());
    match carry_ask(&out) {
        coach::pacing::types::Ask::WeightedDistance {
            load_kg,
            distance_m,
        } => {
            assert!(
                load_kg > 12.0,
                "the weight steps once the distance tops out"
            );
            assert!(
                distance_m < 30,
                "and the distance starts again: {distance_m}"
            );
        }
        other => panic!("expected a distance ask, got {other:?}"),
    }
}

#[test]
fn an_unknown_carry_is_measured_in_metres_rather_than_guessed() {
    // No history: the engine must not invent a distance to prescribe. It asks
    // for a measurement, in the movement's own unit.
    let out = evaluate(&carry_input(vec![]), now());
    let item = out
        .plan
        .iter()
        .find(|s| s.exercise_id == ExerciseId(5))
        .expect("the carry is planned");
    assert_eq!(item.kind, SuggestionKind::Assess);
    match item.ask {
        coach::pacing::types::Ask::LoadedDistance { start_kg } => {
            assert_eq!(start_kg, 12.0, "it opens at the lightest bell owned");
        }
        other => panic!("expected a distance calibration, got {other:?}"),
    }
}

// ---- R6-5: readiness has to be able to say "not today" ----------------------

/// Yesterday's session, so the body is carrying something to recover from.
fn trained_yesterday() -> Vec<SetRec> {
    (0..8).map(|i| bset(1, days_ago(1), 8 + i)).collect()
}

#[test]
fn readiness_at_the_floor_books_no_session() {
    let out = evaluate(
        &PacingInput {
            readiness: Some(Readiness::of(0.0)),
            ..input(Mode::Balanced, catalog(), trained_yesterday(), None, None)
        },
        now(),
    );
    assert!(
        out.plan.is_empty(),
        "a floor reading on a fatigued body is a rest day, not a smaller one: {:?}",
        out.plan
            .iter()
            .map(|s| &s.exercise_name)
            .collect::<Vec<_>>()
    );
    assert!(out.suggestion.is_none());
    assert!(!out.nudge, "and it must not nudge him out to train");
    assert!(
        out.reason.contains("recovery") || out.reason.contains("day off"),
        "the coach has to say why, or a vanished plan reads as a broken app: {:?}",
        out.reason
    );
}

#[test]
fn the_same_morning_with_a_rested_body_still_plans() {
    // Floor readiness, but nothing unrecovered — a bad night, not accumulated
    // fatigue. Answering that with silence would stand the athlete down
    // indefinitely, and the one who most needs a plan is the one not training.
    let out = evaluate(
        &PacingInput {
            readiness: Some(Readiness::of(0.0)),
            ..input(Mode::Balanced, catalog(), vec![], None, None)
        },
        now(),
    );
    assert!(
        !out.plan.is_empty(),
        "a floor reading with nothing to recover from is not a reason to stop training"
    );
}

#[test]
fn a_normal_morning_is_untouched_by_the_rest_rule() {
    let out = evaluate(
        &PacingInput {
            readiness: Some(Readiness::of(0.8)),
            ..input(Mode::Balanced, catalog(), trained_yesterday(), None, None)
        },
        now(),
    );
    assert!(!out.plan.is_empty(), "a recovered athlete trains");
}
