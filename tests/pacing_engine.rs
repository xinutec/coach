//! Dynamic-engine tests. Integration tests against the public `evaluate` + its
//! input/output types — the engine is a pure function, exercised through the same
//! surface `service::now` uses.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use std::collections::HashMap;

use coach::exercise::types::{Metric, Pattern};
use coach::muscle::types::{MuscleRole, Region};
use coach::pacing::ability::Confidence;
use coach::pacing::engine::evaluate;
use coach::pacing::types::{
    Band, Blocker, ExerciseInfo, GroupMeta, Kit, PacingInput, PacingNow, PacingSettings,
    PacingState, Readiness, SetRec, Suggestion, SuggestionKind,
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

/// A bodyweight set with an explicit rep count — for scenarios where the
/// demonstrated maximum, not the volume, is the point.
fn bset(exercise_id: i64, at: NaiveDateTime, reps: i32) -> SetRec {
    SetRec {
        exercise_id,
        logged_at: at,
        reps: Some(reps),
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
    // `available: None` = "the kit isn't what this test is about", which now means
    // a location stocked with everything the catalog needs — not the old "no
    // filter" special case (there isn't one: absent kit means absent kit).
    let kit = Kit(match available {
        Some(v) => v.into_iter().collect(),
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
        exercise_loads: HashMap::new(),
        equipment_names: HashMap::new(),
        notices: Vec::new(),
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

/// Buildable loads for the barbell row (exercise id 5): 20…80 kg in 2.5 kg steps.
/// Keyed by *exercise*, not equipment — what you can build depends on how many
/// implements the movement uses, so the service resolves it per exercise. Without
/// an inventory there's no honest load, so the engine leaves the lift out.
fn owned() -> HashMap<i64, Vec<f64>> {
    let mut loads = Vec::new();
    let mut w = 20.0;
    while w <= 80.0 + 1e-9 {
        loads.push(w);
        w += 2.5;
    }
    HashMap::from([(5, loads)])
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
        .find(|s| s.exercise_id == 1)
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
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(5, vec![20.0, 30.0, 40.0, 50.0, 60.0])]);
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
    let sets_of = |out: &coach::pacing::types::PacingNow, id: i64| -> i32 {
        out.plan
            .iter()
            .filter(|s| s.exercise_id == id && s.kind != SuggestionKind::Warmup)
            .map(|s| s.sets)
            .sum()
    };
    let strength = evaluate(&mk(Mode::Strength), now());
    let skills = evaluate(&mk(Mode::Skills), now());
    assert!(
        sets_of(&strength, 5) > sets_of(&strength, 6),
        "strength spends the day on the loaded row"
    );
    assert!(
        sets_of(&skills, 6) > sets_of(&skills, 5),
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
        equipment_names: HashMap::from([(101, "Barbell".to_string())]),
        ..input(Mode::Strength, exs, vec![], None, Some(vec![]))
    };
    let sug = evaluate(&inp, now()).suggestion.unwrap();
    assert_eq!(sug.exercise_id, 2);
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
        .find(|s| s.exercise_id == 7)
        .expect("a rep pull stands in for the missing machine");
    let hold = out.plan.iter().find(|s| s.exercise_id == 6);
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
        sug.load_kg,
        Some(60.0),
        "no blind +2.5 the reps don't support"
    );
    assert_eq!(sug.rep_high, Some(6));
    assert!(sug.rep_low.unwrap() >= 3 && sug.rep_low.unwrap() <= 6);
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
fn a_calibration_is_complete_after_its_measurement() {
    // A never-done movement is measured (one honest AMRAP, logged mid-session).
    // The plan keeps the card — done, one set of one — and does not turn around
    // and prescribe more of the movement the athlete just took to form breakdown.
    let h = vec![bset(2, minutes_ago(30), 6)]; // ring row: first-ever set, today
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let items: Vec<_> = out.plan.iter().filter(|s| s.exercise_id == 2).collect();
    assert_eq!(items.len(), 1, "one card for the measured movement");
    assert_eq!(
        items[0].kind,
        SuggestionKind::Assess,
        "still the measurement"
    );
    assert_eq!(items[0].sets, 1);
    assert_eq!(items[0].done, 1, "and it's done");
    if let Some(sug) = &out.suggestion {
        assert_ne!(
            sug.exercise_id, 2,
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
        .find(|s| s.exercise_id == 1)
        .expect("push-up planned")
        .sets;

    h.push(set(1, minutes_ago(10)));
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let pushup = out
        .plan
        .iter()
        .find(|s| s.exercise_id == 1)
        .expect("a half-done movement stays on the plan");
    assert_eq!(pushup.sets, asked, "the ask is the committed one");
    assert_eq!(pushup.done, 1, "progress is reported against it");
    assert!(
        out.plan.iter().any(|s| s.exercise_id == 3),
        "untouched movements stay planned too"
    );
}

#[test]
fn no_rep_ratchet_within_a_session() {
    // Trusted at best 4 → today asks 5. Hitting the 5 must not raise the ask to
    // 6 before the next set — best+1 is session-over-session, not set-over-set.
    let mut h = vec![
        bset(1, days_ago(2), 4),
        bset(1, days_ago(4), 4),
        bset(1, days_ago(6), 4),
    ];
    h.push(bset(1, minutes_ago(20), 5));
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let pushup = out.plan.iter().find(|s| s.exercise_id == 1).unwrap();
    assert_eq!(
        pushup.rep_low,
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
        let mut v: Vec<i64> = p.plan.iter().map(|s| s.exercise_id).collect();
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
        .filter(|s| s.exercise_id != 40 && s.kind == SuggestionKind::Assess)
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
        out.reason.contains("Rest a moment"),
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
        .find(|s| s.kind == SuggestionKind::Warmup && s.exercise_id == 9)
        .expect("the drill leads the plan");
    assert_eq!(wu.rep_low, Some(10), "a dose, not a vibe");

    // Log the drill mid-session → its card reads done.
    let h = vec![set(9, minutes_ago(10))];
    let out = evaluate(&input(Mode::Balanced, exs, h, None, None), now());
    let wu = out
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Warmup && s.exercise_id == 9)
        .expect("still on the plan");
    assert_eq!(wu.done, 1, "a logged warm-up completes its card");
}

#[test]
fn a_bodyweight_target_is_one_rep_up_from_ability_not_the_mode_floor() {
    // Three recent sessions ground out at 2 reps — an honest maximum, shown while
    // doing one's best. Balanced mode *likes* 8–12 reps, but a style preference is
    // a ceiling to climb toward, not a floor to demand: asking 8 from an athlete
    // who has shown 2 prescribes failure, and it silently defeats the
    // miss-response too (aim best−1, clamped straight back up to 8). The target is
    // one rep above the best demonstrated.
    let h = vec![
        bset(1, days_ago(2), 2),
        bset(1, days_ago(4), 2),
        bset(1, days_ago(6), 2),
    ];
    let out = evaluate(&input(Mode::Balanced, catalog(), h, None, None), now());
    let pushup = out
        .plan
        .iter()
        .find(|s| s.exercise_id == 1)
        .expect("a trusted, recovered movement in deficit is planned");
    assert_eq!(pushup.kind, SuggestionKind::Work);
    assert_eq!(
        pushup.rep_low,
        Some(3),
        "one rep above the demonstrated 2 — not the mode floor of 8"
    );
    assert_eq!(
        pushup.rep_high,
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
    let pushup = out.plan.iter().find(|s| s.exercise_id == 1).unwrap();
    assert_eq!(pushup.rep_low, Some(12));
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
    assert_eq!(sug.load_kg, Some(20.0), "the lightest owned rung");
    assert_eq!(
        sug.rep_low,
        Some(3),
        "the reps the estimate supports at that weight — not the style floor of 6"
    );
}

#[test]
fn a_stronger_history_earns_a_heavier_owned_weight() {
    // Same exercise, owned 15/17.5/20 kg. A weaker recent history prescribes a
    // lighter owned weight than a stronger one — the load step is *earned* by the
    // logged sets raising the e1RM past the next weight, never a blind bump.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(5, vec![15.0, 17.5, 20.0])]);
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
        strong.load_kg.unwrap() > weak.load_kg.unwrap(),
        "stronger history → heavier owned weight ({:?} > {:?})",
        strong.load_kg,
        weak.load_kg
    );
    // Every prescribed load is a weight actually owned here.
    for s in [&weak, &strong] {
        assert!(
            owned[&5].contains(&s.load_kg.unwrap()),
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
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(5, vec![40.0, 50.0, 60.0])]);
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
        sug.load_kg.unwrap() < 60.0,
        "stale PR decayed below its old weight, got {:?}",
        sug.load_kg
    );
}

#[test]
fn a_work_item_carries_its_reasoning() {
    // A trained group in deficit → the suggestion explains itself: the group's
    // deficit + recovery, the ability confidence, and (here) an e1RM estimate.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(5, vec![40.0, 50.0, 60.0])]);
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
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(5, vec![10.0, 15.0, 20.0])]);
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
    assert_eq!(sug.load_kg, Some(10.0));
}

#[test]
fn trusted_ability_prescribes_untrusted_ability_assesses() {
    // Same lift + owned inventory. Three recent sessions → High confidence → a
    // real prescription (Work). Only a 200-day-old set → Low confidence → the
    // engine re-measures (Assess) rather than trust the stale number.
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(5, vec![40.0, 50.0, 60.0])]);
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
    let owned: HashMap<i64, Vec<f64>> = HashMap::from([(5, vec![40.0, 45.0, 50.0, 55.0, 60.0])]);
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
        exercise_loads: HashMap::new(),
        equipment_names: HashMap::new(),
        notices: vec!["No weights registered here for Barbell.".to_string()],
        ..input(Mode::Strength, exs, vec![], None, Some(vec![3]))
    };
    let out = evaluate(&inp, now());

    assert!(
        out.plan.iter().all(|s| s.exercise_id != 5),
        "a weighted lift with no registered weights is never planned"
    );
    assert!(
        out.plan.iter().any(|s| s.exercise_id == 2),
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
    // (`Option<HashSet>` consulted with `is_none_or`) made that mean "everything
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
fn bells() -> HashMap<i64, Vec<f64>> {
    HashMap::from([(
        7,
        vec![
            6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 24.0, 28.0, 32.0, 36.0,
        ],
    )])
}

/// A carry set: a load *and* a duration, no reps.
fn cset(exercise_id: i64, at: NaiveDateTime, load: f64, secs: i32) -> SetRec {
    SetRec {
        exercise_id,
        logged_at: at,
        reps: None,
        load_kg: Some(load),
        hold_s: Some(secs),
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
    assert!(w.load_kg.is_some(), "a carry has a weight");
    assert!(w.hold_s.is_some(), "a carry has a duration");
    assert_eq!(w.rep_low, None, "a carry is not measured in reps");
    assert_eq!(w.rep_high, None, "a carry is not measured in reps");
}

#[test]
fn a_carry_climbs_the_clock_then_steps_the_weight() {
    // Under the ceiling: same bell, longer walk — the load is *earned* before it
    // moves, exactly as reps are on a weighted lift.
    let climbing = carry_plan(vec![
        cset(7, days_ago(2), 12.0, 30),
        cset(7, days_ago(5), 12.0, 30),
        cset(7, days_ago(9), 12.0, 30),
    ]);
    let w = climbing
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Work)
        .unwrap();
    assert_eq!(
        w.load_kg,
        Some(12.0),
        "the bell holds while the clock climbs"
    );
    assert!(
        w.hold_s.unwrap() > 30,
        "the walk gets longer, got {:?}",
        w.hold_s
    );

    // At the ceiling: the walk is long enough, so it's asking for a heavier bell —
    // the next one actually owned (14, not 12.7) — and the clock starts again.
    let topped = carry_plan(vec![
        cset(7, days_ago(2), 12.0, 60),
        cset(7, days_ago(5), 12.0, 60),
        cset(7, days_ago(9), 12.0, 60),
    ]);
    let w = topped
        .plan
        .iter()
        .find(|s| s.kind == SuggestionKind::Work)
        .unwrap();
    assert_eq!(w.load_kg, Some(14.0), "the next bell up, and one he owns");
    assert_eq!(w.hold_s, Some(30), "the clock resets at the heavier weight");
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
    assert_eq!(a.load_kg, Some(6.0), "opens at the lightest bell owned");
    assert_eq!(a.hold_s, None, "the duration is what's being measured");
    assert_eq!(a.rep_low, None, "still not reps");
}

#[test]
fn a_carry_with_no_registered_weights_is_not_prescribed() {
    // The same rule as any other loaded lift: no honest load exists, so it is left
    // out and named — never carried at a weight he might not own.
    let out = evaluate(
        &PacingInput {
            groups: back_only(),
            exercise_loads: HashMap::new(), // the bells aren't registered
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
        !out.plan.iter().any(|s| s.exercise_id == 7),
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
        .find(|s| s.kind == SuggestionKind::Work && s.exercise_id == 5)
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
        after_misses.load_kg.unwrap() < steady.load_kg.unwrap(),
        "two misses should back the load off: steady {:?} vs after-misses {:?}",
        steady.load_kg,
        after_misses.load_kg
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
        .find(|s| s.exercise_id == 5)
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
        w.load_kg.unwrap() >= 50.0,
        "prescribed near his level, got {:?}",
        w.load_kg
    );
    assert_eq!(w.explanation.map(|e| e.misses), Some(0));
}
