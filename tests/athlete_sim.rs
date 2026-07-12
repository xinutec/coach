//! E3 — athlete simulation: convergence as a regression test.
//!
//! A deterministic **virtual athlete** run against the engine for simulated
//! months. The athlete has a hidden *true* ability and performs each
//! prescription honestly — reps to (or short of) failure, with an integer RPE
//! that reports its reserve. The engine sees only the logged sets; it must
//! *recover* the true ability from them and prescribe against it.
//!
//! No randomness: the athlete is a closed-form dose-response model (Epley
//! reps-to-failure at a load), so every run is reproducible. This turns
//! "becomes a close-to-perfect trainer over time" into tested properties:
//!
//!   * **convergence** — from a cold, deliberately-too-light first assessment,
//!     the estimated e1RM climbs to within a few percent of true and stays there;
//!   * **honesty** — the RPE-aware estimate never materially *exceeds* true
//!     ability (no chimera — you can't invent strength the sets don't show);
//!   * **stability** — once converged, the prescribed load doesn't oscillate;
//!   * **tracking** — when true ability *grows*, the estimate follows it up;
//!   * **bounded ramp** — planned volume never exceeds the day budget;
//!   * **recovery honesty** — recovery is graded (G6), so a mostly-recovered
//!     group can take light work; nothing below the effective-recovery gate
//!     (deficit × recovery) is ever prescribed.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate, NaiveDateTime};

use coach::exercise::types::{Metric, Pattern};
use coach::muscle::types::{MuscleRole, Region};
use coach::pacing::cover;
use coach::pacing::engine::evaluate;
use coach::pacing::types::{
    ExerciseInfo, GroupMeta, Kit, PacingInput, PacingSettings, SetRec, Suggestion, SuggestionKind,
};
use coach::settings::types::Mode;

// ---- the virtual athlete ---------------------------------------------------

/// Epley reps-to-failure: how many reps at `load` a lifter with this 1RM can do
/// before form breaks. The dual of the estimator's `load × (1 + reps/30)`, so an
/// honestly-reported set inverts back to (near) the true 1RM.
fn reps_to_failure(true_e1rm: f64, load: f64) -> f64 {
    (30.0 * (true_e1rm / load - 1.0)).max(0.0)
}

/// A lifter with a hidden true strength. `perform` executes a prescribed set and
/// reports what a scrupulously honest trainee would log: as many of the target
/// reps as stay clean, and an integer RPE encoding the reserve left.
struct Athlete {
    /// Hidden true 1-rep-max (kg) for the weighted lift.
    true_e1rm: f64,
    /// Hidden true clean-rep ceiling for the bodyweight move.
    true_reps: i32,
}

impl Athlete {
    /// Perform `target` reps at `load` on the weighted lift. Returns
    /// (reps done, RPE). RPE = 10 − reps-in-reserve, integer-rounded and clamped
    /// — the same lossy signal a real logger produces.
    fn lift(&self, load: f64, target: i32) -> (i32, i32) {
        let capacity = reps_to_failure(self.true_e1rm, load);
        let done = target.min(capacity.floor() as i32).max(1);
        let reserve = (capacity - done as f64).max(0.0);
        let rpe = (10.0 - reserve).round().clamp(1.0, 10.0) as i32;
        (done, rpe)
    }

    /// Perform a bodyweight rep set. `target = None` means AMRAP (an assessment):
    /// go to the clean ceiling. Returns (reps done, RPE).
    fn reps(&self, target: Option<i32>) -> (i32, i32) {
        let cap = self.true_reps;
        let done = target.map_or(cap, |t| t.min(cap)).max(1);
        let rpe = (10 - (cap - done)).clamp(1, 10);
        (done, rpe)
    }
}

// ---- harness ---------------------------------------------------------------

fn settings() -> PacingSettings {
    PacingSettings {
        window_start_hour: 8,
        window_end_hour: 21,
        min_rest_min: 20,
    }
}

// A Monday midday, inside the window — the simulation's session-0 instant.
fn start() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 1, 5)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
}

/// Owned barbell inventory: 20…80 kg in 2.5 kg steps (equipment id 3).
fn owned() -> HashMap<i64, Vec<f64>> {
    let mut loads = Vec::new();
    let mut w = 20.0;
    while w <= 80.0 + 1e-9 {
        loads.push(w);
        w += 2.5;
    }
    HashMap::from([(3, loads)])
}

fn barbell_row() -> ExerciseInfo {
    ExerciseInfo {
        id: 5,
        name: "Barbell row".into(),
        pattern: Pattern::Pull,
        metric: Metric::WeightedReps,
        is_skill: false,
        warmup: false,
        equipment: vec![3],
        groups: vec![(20, MuscleRole::Primary)],
    }
}

fn back_group() -> Vec<GroupMeta> {
    vec![GroupMeta {
        id: 20,
        name: "Lats".into(),
        region: Region::Back,
    }]
}

/// Build the engine input for one simulated instant, given the history so far.
fn row_input(history: Vec<SetRec>) -> PacingInput {
    let last_set_at = history.iter().map(|s| s.logged_at).max();
    PacingInput {
        mode: Mode::Strength,
        days_per_week: 3,
        emphasis: None,
        exercises: vec![barbell_row()],
        history,
        last_set_at,
        settings: settings(),
        groups: back_group(),
        kit: Some(Kit([3].into_iter().collect())),
        equipment_loads: owned(),
        equipment_names: HashMap::new(),
        readiness: None,
    }
}

/// The engine's training suggestion for the (single) back exercise this cycle.
fn row_suggestion(history: &[SetRec], now: NaiveDateTime) -> Suggestion {
    let out = evaluate(&row_input(history.to_vec()), now);
    let sug = out.suggestion.expect("a row suggestion every cycle");
    assert_eq!(sug.exercise_id, 5, "the only exercise is the row");
    sug
}

// ---- convergence + honesty + stability -------------------------------------

#[test]
fn estimate_converges_to_true_ability_and_holds() {
    // True 1RM sits deliberately off the 2.5 kg grid, so convergence has to work
    // through weight-snapping, not land on it for free.
    let t_true = 63.7;
    let athlete = Athlete {
        true_e1rm: t_true,
        true_reps: 0,
    };

    let mut history: Vec<SetRec> = Vec::new();
    let mut est: Vec<Option<f64>> = Vec::new();
    let mut loads: Vec<f64> = Vec::new();

    // 16 sessions, 3 days apart — always past the back region's 60 h recovery
    // horizon, so the group is trainable every cycle.
    for i in 0..16 {
        let now = start() + Duration::days(3 * i);
        let sug = row_suggestion(&history, now);
        est.push(sug.explanation.and_then(|e| e.e1rm));
        let load = sug.load_kg.expect("a weighted prescription carries a load");
        loads.push(load);
        // Aim for the top of the range (what "climb to the top" asks); the load
        // only advances when the logged sets earn it.
        let target = sug.rep_high.or(sug.rep_low).unwrap_or(5);
        let (reps, rpe) = athlete.lift(load, target);
        history.push(SetRec {
            exercise_id: 5,
            logged_at: now,
            reps: Some(reps),
            load_kg: Some(load),
            hold_s: None,
            rpe: Some(rpe),
        });
    }

    // Cold start is honest about not knowing: the first cycle is an assessment
    // with no estimate yet.
    assert!(est[0].is_none(), "cycle 0 has no ability estimate");

    // Convergence: the last five cycles are all within 6 % of true.
    for (i, e) in est.iter().enumerate().skip(11) {
        let e = e.unwrap_or_else(|| panic!("cycle {i} should have an estimate"));
        let err = (e - t_true).abs() / t_true;
        assert!(
            err < 0.06,
            "cycle {i}: estimate {e:.1} vs true {t_true} ({}%)",
            (err * 100.0).round()
        );
    }

    // Honesty: the RPE-aware estimate never materially exceeds true ability — you
    // can't manufacture strength the sets don't demonstrate.
    for (i, e) in est.iter().enumerate() {
        if let Some(e) = e {
            assert!(
                *e <= t_true * 1.03,
                "cycle {i}: estimate {e:.1} overshoots true {t_true}"
            );
        }
    }

    // It actually climbed: the cold assessment under-shoots (a light first set
    // reads weak), then the estimate ratchets up toward true.
    let first = est[1].expect("cycle 1 has an estimate");
    let last = est[15].expect("cycle 15 has an estimate");
    assert!(
        last > first + 10.0,
        "estimate climbed from {first:.1} to {last:.1}"
    );

    // Stability: once converged, the prescribed load doesn't ping-pong — the last
    // five loads span at most a single 2.5 kg increment.
    let tail = &loads[11..];
    let lo = tail.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = tail.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    assert!(hi - lo <= 2.5, "converged load oscillates: {tail:?}");
}

// ---- tracking a strengthening athlete --------------------------------------

#[test]
fn estimate_tracks_true_ability_as_it_grows() {
    // The athlete gets stronger: true 1RM approaches a ceiling geometrically —
    // the classic diminishing-returns dose response. The estimate must follow it
    // up, not lag behind or overshoot.
    const CEILING: f64 = 70.0;
    const GAIN: f64 = 0.04;
    let mut athlete = Athlete {
        true_e1rm: 45.0,
        true_reps: 0,
    };

    let mut history: Vec<SetRec> = Vec::new();
    // (true, estimate) pairs for the second half, where the estimate has caught up.
    let mut track: Vec<(f64, f64)> = Vec::new();
    let mut first_est = None;

    for i in 0..30 {
        let now = start() + Duration::days(3 * i);
        let sug = row_suggestion(&history, now);
        let load = sug.load_kg.expect("a weighted prescription carries a load");
        let target = sug.rep_high.or(sug.rep_low).unwrap_or(5);
        let (reps, rpe) = athlete.lift(load, target);
        history.push(SetRec {
            exercise_id: 5,
            logged_at: now,
            reps: Some(reps),
            load_kg: Some(load),
            hold_s: None,
            rpe: Some(rpe),
        });
        if let Some(e) = sug.explanation.and_then(|e| e.e1rm) {
            first_est.get_or_insert(e);
            if i >= 15 {
                track.push((athlete.true_e1rm, e));
            }
        }
        // Grow after the session — this cycle's set reflected the old strength.
        athlete.true_e1rm += GAIN * (CEILING - athlete.true_e1rm);
    }

    // The estimate captured a real, substantial gain (not stuck at the cold start).
    let (_, final_est) = *track.last().unwrap();
    assert!(
        final_est > first_est.unwrap() + 8.0,
        "estimate tracked the gain: {:.1} → {final_est:.1}",
        first_est.unwrap()
    );

    // Through the strengthening phase it stays in a tight lag band below true —
    // close on the heels of the moving target, never running ahead of it.
    for (t, e) in &track {
        assert!(*e <= t * 1.05, "estimate {e:.1} overshoots true {t:.1}");
        assert!(*e >= t * 0.82, "estimate {e:.1} lags true {t:.1} too far");
    }
}

// ---- recovery honesty + bounded ramp (multi-group, daily training) ---------

fn body_ex(id: i64, name: &str, pattern: Pattern, group: i64) -> ExerciseInfo {
    ExerciseInfo {
        id,
        name: name.into(),
        pattern,
        metric: Metric::Reps,
        is_skill: false,
        warmup: false,
        equipment: vec![],
        groups: vec![(group, MuscleRole::Primary)],
    }
}

#[test]
fn never_prescribes_unrecovered_work_and_stays_within_budget() {
    // Three groups, bodyweight — an athlete who trains every single day. Recovery
    // is graded (G6), so a mostly-recovered group can still take light work; the
    // honest guarantee is that nothing the model considers freshly hammered
    // (deficit × recovery below the gate) is ever prescribed — and the plan stays
    // inside the day's set budget the whole way.
    let catalog = vec![
        body_ex(1, "Push-up", Pattern::Push, 10),
        body_ex(2, "Ring row", Pattern::Pull, 20),
        body_ex(3, "Squat", Pattern::Legs, 30),
    ];
    let groups = vec![
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
    ];
    // Distinct clean-rep ceilings so the moves aren't interchangeable.
    let ability = HashMap::from([(1, 25), (2, 15), (3, 30)]);

    let mut history: Vec<SetRec> = Vec::new();

    for i in 0..14 {
        let now = start() + Duration::days(i);
        let last_set_at = history.iter().map(|s| s.logged_at).max();
        let input = PacingInput {
            mode: Mode::Balanced,
            days_per_week: 6,
            emphasis: None,
            exercises: catalog.clone(),
            history: history.clone(),
            last_set_at,
            settings: settings(),
            groups: groups.clone(),
            kit: Some(Kit(catalog
                .iter()
                .flat_map(|e| e.equipment.clone())
                .collect())),
            equipment_loads: HashMap::new(),
            equipment_names: HashMap::new(),
            readiness: None,
        };
        let out = evaluate(&input, now);

        // Recovery honesty: every work/assess item paid down real, recovered need —
        // the engine never prescribes a group that's already at target or still
        // sore. The item's own explanation trace carries the number the cover gated
        // it on (E5), so this asserts the actual gate rather than a mirror of it.
        for s in &out.plan {
            if s.kind == SuggestionKind::Warmup {
                continue;
            }
            let e = s.explanation.expect("a work/assess item explains itself");
            assert!(
                e.pays >= cover::MIN_PAY - 1e-9,
                "day {i}: planned {} on {}, paying only {:.2} effective sets \
                 (deficit {:.2} × recovery {:.2})",
                s.exercise_name,
                s.group,
                e.pays,
                e.deficit,
                e.recovery
            );
        }

        // Bounded ramp: planned work + assess volume never exceeds the day budget.
        assert!(
            out.day_target_sets >= 3 && out.day_target_sets <= 15,
            "day {i}: budget {} out of range",
            out.day_target_sets
        );
        let planned: i32 = out
            .plan
            .iter()
            .filter(|s| s.kind != SuggestionKind::Warmup)
            .map(|s| s.sets)
            .sum();
        assert!(
            planned <= out.day_target_sets,
            "day {i}: planned {planned} over budget {}",
            out.day_target_sets
        );

        // Perform the whole session; each logged set feeds the next day's verdict.
        let athlete = |id: i64| Athlete {
            true_e1rm: 0.0,
            true_reps: ability[&id],
        };
        for s in &out.plan {
            if s.kind == SuggestionKind::Warmup {
                continue;
            }
            let (reps, rpe) = athlete(s.exercise_id).reps(s.rep_high.or(s.rep_low));
            history.push(SetRec {
                exercise_id: s.exercise_id,
                logged_at: now,
                reps: Some(reps),
                load_kg: None,
                hold_s: None,
                rpe: Some(rpe),
            });
        }
    }
}
