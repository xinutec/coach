//! The dynamic coaching engine: a pure function from (history, mode, instant) to
//! a verdict. No I/O, no clock — the caller passes `now` (user-local tz). It
//! computes rolling muscle-group volume, gates on recovery, sets per-group
//! targets from the active **mode** blended with the user's own history, picks
//! the biggest recovered deficit, chooses a location-doable exercise, and
//! progresses it off the last performance. No program, no weekly plan.
//!
//! All coefficients below are labelled heuristics, tunable — targets are anchored
//! to the user's own history to avoid false-precision absolute landmarks.

use std::collections::HashMap;

use chrono::{Duration, NaiveDateTime, Timelike};

use crate::exercise::types::{Metric, Pattern};
use crate::muscle::types::{MuscleRole, Region};
use crate::settings::types::Mode;

use super::ability::{self, Ability, Confidence};
use super::types::{
    Band, ExerciseInfo, Explanation, GroupBalance, PacingInput, PacingNow, PacingState, Suggestion,
    SuggestionKind,
};

/// Readiness score below this → hold progression (don't chase PRs on a bad day).
const READINESS_HOLD_BELOW: f64 = 0.40;

/// Reps in reserve the working load targets at the top of the rep range. `0` =
/// prescribe to demonstrated capacity: a load whose top-of-range reps match your
/// estimated e1RM. Progression is then *earned* — the load only steps up when
/// logged sets raise the e1RM enough to cross the next owned weight — never a
/// blind +2.5 kg the reps don't support.
const TARGET_RIR: f64 = 0.0;
/// Extra reps-in-reserve on a low-readiness day → a lighter working load.
const LOW_READINESS_EXTRA_RIR: f64 = 2.0;
/// Cold-start hold (seconds) when an isometric has no history yet.
const COLD_HOLD_S: i32 = 20;
/// Seconds added to a hold when progressing (bounded properly in a later stage).
const HOLD_STEP_S: i32 = 5;
/// A calibration set for a weighted lift: build up to a hard-but-clean set of
/// this many reps and log load/reps/RPE — the measurement the estimate needs.
const ASSESS_WEIGHTED_REPS: i32 = 5;
/// Fewest sets a work item in the plan is ever sized to — below this the stimulus
/// isn't worth a plan slot.
const WORK_MIN_SETS: i32 = 2;
/// Sets for a warm-up item (mobility drill or ramp-in) — one is enough to prep.
const WARMUP_SETS: i32 = 1;
/// A ramp-in set runs at this fraction of the first heavy lift's working load.
const RAMP_FRACTION: f64 = 0.5;

// ---- tunable heuristics ----------------------------------------------------
const ROLLING_DAYS: i64 = 7; // rolling-volume window (a training week)
const HISTORY_WEEKS: i64 = 8; // personal-average window
const RECOVERY_SETS: f64 = 3.0; // unrecovered load (age-weighted sets) that fully gates a group
const RECOVERED_FRACTION: f64 = 0.85; // ≥ this recovery fraction → shown as recovered
const MIN_EFFECTIVE_DEFICIT: f64 = 0.05; // below this recovery-scaled deficit, don't train the group
const DEFAULT_WEEKLY_SETS: f64 = 10.0; // literature maintenance→growth anchor
const SECONDARY_CREDIT: f64 = 0.5; // a synergist (secondary) counts half a set
const STABILIZER_CREDIT: f64 = 0.25; // an isometric stabilizer counts a quarter
const EMPHASIS_MULT: f64 = 1.5;
const DELOAD_RATIO: f64 = 1.6; // last-7d volume this far above avg → auto-deload
const DELOAD_SCALE: f64 = 0.6;

/// How long a region takes to recover from a hard hit (hours) — bigger muscle
/// masses recover slower. Drives the graded recovery ramp (G6): a group's recent
/// load decays to "recovered" linearly over this horizon.
fn recovery_horizon(r: Region) -> f64 {
    use Region::*;
    match r {
        Legs => 72.0,
        Back | Chest => 60.0,
        Shoulders => 48.0,
        Arms | Forearms | Core => 36.0,
    }
}

/// Per-region volume weighting for a mode. Balanced = flat; the others tilt
/// toward what the mode is for.
fn region_mult(mode: Mode, r: Region) -> f64 {
    use Region::*;
    match mode {
        Mode::Balanced => 1.0,
        Mode::Strength => match r {
            Legs => 1.3,
            Back => 1.2,
            Chest => 1.2,
            Shoulders => 1.1,
            Core => 0.9,
            Arms => 0.7,
            Forearms => 0.6,
        },
        Mode::Skills => match r {
            Core => 1.4,
            Shoulders => 1.3,
            Forearms => 1.3,
            Arms => 1.1,
            Back => 1.1,
            Chest => 0.9,
            Legs => 0.8,
        },
        Mode::Conditioning => match r {
            Legs => 1.2,
            Core => 1.2,
            Chest | Back | Shoulders => 1.0,
            Arms => 0.9,
            Forearms => 0.8,
        },
    }
}

/// Rep range for a mode + metric (holds are seconds, handled in `progress`).
fn rep_range(mode: Mode, metric: Metric) -> (Option<i32>, Option<i32>) {
    if metric == Metric::Hold {
        return (None, None);
    }
    let weighted = metric == Metric::WeightedReps;
    match mode {
        Mode::Strength => {
            if weighted {
                (Some(3), Some(6))
            } else {
                (Some(5), Some(8))
            }
        }
        Mode::Balanced => {
            if weighted {
                (Some(6), Some(10))
            } else {
                (Some(8), Some(12))
            }
        }
        Mode::Skills => (Some(3), Some(6)),
        Mode::Conditioning => {
            if weighted {
                (Some(12), Some(20))
            } else {
                (Some(15), Some(25))
            }
        }
    }
}

/// How well an exercise fits the mode's style (for ranking within a group).
fn mode_fit(mode: Mode, ex: &ExerciseInfo) -> f64 {
    match mode {
        Mode::Strength => {
            if ex.metric == Metric::WeightedReps {
                1.0
            } else if ex.is_skill {
                0.2
            } else {
                0.6
            }
        }
        Mode::Skills => {
            if ex.is_skill || ex.metric == Metric::Hold {
                1.0
            } else {
                0.4
            }
        }
        Mode::Conditioning => match ex.metric {
            Metric::Reps => 1.0,
            Metric::WeightedReps => 0.7,
            Metric::Hold => 0.3,
        },
        Mode::Balanced => 0.8,
    }
}

/// Snap a load to the nearest weight you actually own (ties → lighter). Unknown
/// inventory → the raw load rounded to the nearest 0.5 kg (a cleaner number than
/// an inverse-Epley decimal, and the smallest plate step that's universal).
fn snap(loads: &[f64], w: f64) -> f64 {
    loads
        .iter()
        .copied()
        .min_by(|a, b| (a - w).abs().total_cmp(&(b - w).abs()))
        .unwrap_or((w * 2.0).round() / 2.0)
}

/// The load whose top-set of `reps` reps (leaving `rir` in reserve) matches an
/// estimated 1-rep-max of `e1rm` — inverse Epley.
fn load_for(e1rm: f64, reps: f64, rir: f64) -> f64 {
    e1rm / (1.0 + (reps + rir) / 30.0)
}

/// Reps within reach at `load` (leaving `rir` in reserve) given `e1rm` — Epley,
/// solved for reps. The dual of [`load_for`].
fn reps_at(e1rm: f64, load: f64, rir: f64) -> f64 {
    30.0 * (e1rm / load - 1.0) - rir
}

/// Prescribe (sets, rep_low, rep_high, load_kg, hold_s) for an exercise from the
/// athlete's **ability** — not the last set. Weighted work autoregulates: the
/// working load is derived from the decayed e1RM so a layoff self-corrects to a
/// lighter start, and the load only steps up when logged sets raise the estimate
/// past the next owned weight (double progression, but *earned* and snapped to
/// what you own). `advance = false` (low readiness) leaves more in reserve —
/// keep it light, don't chase a PR. No estimate yet → a conservative cold start.
fn prescribe(
    ex: &ExerciseInfo,
    ability: Option<&Ability>,
    mode: Mode,
    advance: bool,
    loads: &[f64],
) -> (i32, Option<i32>, Option<i32>, Option<f64>, Option<i32>) {
    let sets = 3;
    let (lo, hi) = rep_range(mode, ex.metric);
    let cold = loads.first().copied();
    let reserve = if advance {
        TARGET_RIR
    } else {
        TARGET_RIR + LOW_READINESS_EXTRA_RIR
    };
    match ex.metric {
        Metric::WeightedReps => {
            let (lo_v, hi_v) = (lo.unwrap_or(6), hi.unwrap_or(10));
            match ability.and_then(|a| a.e1rm) {
                Some(e) => {
                    // Working load: the weight you own nearest the one that puts
                    // the top of the range within reach at the target reserve.
                    let w = snap(loads, load_for(e, hi_v as f64, reserve));
                    // At that (discrete) weight, how many reps are actually in
                    // reach? Clamped into the range — this is the rep target that
                    // climbs to the top before the weight is allowed to step.
                    let target = (reps_at(e, w, reserve).round() as i32).clamp(lo_v, hi_v);
                    (sets, Some(target), hi, Some(w), None)
                }
                // Never done / no recent estimate → lightest owned + full range.
                None => (sets, lo, hi, cold, None),
            }
        }
        Metric::Reps => {
            // Only lever is reps: climb toward the top of the range off the
            // decayed best; hold (no climb) on a low-readiness day.
            let target = match ability.and_then(|a| a.best_reps) {
                Some(best) => {
                    let aim = if advance { best + 1 } else { best };
                    aim.clamp(lo.unwrap_or(8), hi.unwrap_or(12))
                }
                None => lo.unwrap_or(8),
            };
            (sets, Some(target), hi, None, None)
        }
        Metric::Hold => {
            let base = ability.and_then(|a| a.best_hold).unwrap_or(COLD_HOLD_S);
            let secs = if advance { base + HOLD_STEP_S } else { base };
            (2, None, None, None, Some(secs))
        }
    }
}

/// A calibration set for an exercise whose ability is untrusted (never done, or
/// only stale data): one set, framed as a measurement. The logged result feeds
/// the ability model, so the next verdict prescribes from it (G3). Weighted →
/// build up to a hard, clean `ASSESS_WEIGHTED_REPS` (a starting load offered from
/// any decayed estimate, else the lightest owned); reps → AMRAP to form
/// breakdown; hold → one max hold. Open rep/hold fields signal "as many as clean".
fn assess(
    ex: &ExerciseInfo,
    ability: Option<&Ability>,
    loads: &[f64],
) -> (i32, Option<i32>, Option<i32>, Option<f64>, Option<i32>) {
    match ex.metric {
        Metric::WeightedReps => {
            let load = match ability.and_then(|a| a.e1rm) {
                // Offer a safe build-up target from the (stale) estimate.
                Some(e) => Some(snap(
                    loads,
                    load_for(e, ASSESS_WEIGHTED_REPS as f64, LOW_READINESS_EXTRA_RIR),
                )),
                None => loads.first().copied(),
            };
            (
                1,
                Some(ASSESS_WEIGHTED_REPS),
                Some(ASSESS_WEIGHTED_REPS),
                load,
                None,
            )
        }
        // AMRAP / max hold — the open fields say "as many clean reps / as long as
        // clean form holds", which is exactly what we're measuring.
        Metric::Reps => (1, None, None, None, None),
        Metric::Hold => (1, None, None, None, None),
    }
}

/// Which block of the session an exercise belongs in — the classic order that
/// puts demanding, technical work while the nervous system is fresh and leaves
/// finishers for last. Lower runs earlier. Tier 1 is reserved for the warm-up
/// block (a later stage).
fn tier(ex: &ExerciseInfo) -> u8 {
    if ex.is_skill || ex.metric == Metric::Hold {
        2 // skill / hold work — needs a fresh CNS
    } else if ex.metric == Metric::WeightedReps && ex.pattern != Pattern::Core {
        3 // heavy compound weighted
    } else if ex.pattern == Pattern::Core {
        5 // core / conditioning finisher
    } else {
        4 // bodyweight / isolation accessory
    }
}

/// Size and order the resolved candidates into the day's plan. Work items get a
/// share of the `budget` sets proportional to their muscle-group deficit (at
/// least `WORK_MIN_SETS`); Assess (calibration) and Hold items keep their own
/// counts. Items are added biggest-deficit-first until the budget is spent, then
/// re-sorted into training order (tier, then deficit, then id) so the list reads
/// top-to-bottom as a sensible session — the athlete starts at the top or picks
/// any item. Recomputed statelessly each call, so logging a set reshapes it.
fn build_plan(mut resolved: Vec<(Suggestion, f64, u8)>, budget: i32) -> Vec<Suggestion> {
    if budget <= 0 {
        return Vec::new();
    }
    // Fund the biggest deficits first.
    resolved.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.exercise_id.cmp(&b.0.exercise_id))
    });
    let total_def: f64 = resolved.iter().map(|(_, d, _)| d).sum::<f64>().max(1e-9);
    let mut left = budget;
    let mut chosen: Vec<(Suggestion, f64, u8)> = Vec::new();
    for (mut sug, def, t) in resolved {
        if left <= 0 {
            break;
        }
        // Weighted / bodyweight *work* is sized to its deficit share; assess and
        // hold items keep their fixed counts but still draw down the budget.
        let resizeable = sug.kind == SuggestionKind::Work && sug.hold_s.is_none();
        if resizeable {
            let share = ((budget as f64) * def / total_def).round() as i32;
            sug.sets = share.max(WORK_MIN_SETS).min(left);
        }
        left -= sug.sets;
        chosen.push((sug, def, t));
    }
    // Present in training order.
    chosen.sort_by(|a, b| {
        a.2.cmp(&b.2)
            .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.0.exercise_id.cmp(&b.0.exercise_id))
    });
    chosen.into_iter().map(|(s, _, _)| s).collect()
}

/// The discrete weights owned here for an exercise's kit (union over its
/// load-bearing equipment, sorted asc, deduped) — the set loads snap to.
fn owned_loads(ex: &ExerciseInfo, equipment_loads: &HashMap<i64, Vec<f64>>) -> Vec<f64> {
    let mut loads: Vec<f64> = ex
        .equipment
        .iter()
        .filter_map(|id| equipment_loads.get(id))
        .flatten()
        .copied()
        .collect();
    loads.sort_by(f64::total_cmp);
    loads.dedup();
    loads
}

/// Build the warm-up block for a work plan: a little joint/mobility prep for the
/// muscle groups the session trains, plus a light ramp-in set on the first heavy
/// lift. Warm-ups credit no volume and are the only place warm-up-tagged moves
/// appear. Ordered first (they're what you do before the session). Deterministic:
/// mobility by exercise id, one per not-yet-covered session group.
fn build_warmup(
    work: &[Suggestion],
    input: &PacingInput,
    ex_by_id: &HashMap<i64, &ExerciseInfo>,
    group_name: &HashMap<i64, String>,
) -> Vec<Suggestion> {
    if work.is_empty() {
        return Vec::new();
    }
    // The muscle groups this session trains (primary work).
    let mut session_groups: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for w in work {
        if let Some(ex) = ex_by_id.get(&w.exercise_id) {
            for (g, r) in &ex.groups {
                if *r == MuscleRole::Primary {
                    session_groups.insert(*g);
                }
            }
        }
    }
    let avail = input.available_equipment.as_ref();
    let doable = |eq: &[i64]| avail.is_none_or(|a| eq.iter().all(|e| a.contains(e)));

    // Mobility: warm-up-tagged moves for the session's groups, doable here. Take
    // one per still-uncovered group so we don't stack redundant drills.
    let mut movers: Vec<&ExerciseInfo> = input
        .exercises
        .iter()
        .filter(|e| {
            e.warmup
                && doable(&e.equipment)
                && e.groups
                    .iter()
                    .any(|(g, r)| *r == MuscleRole::Primary && session_groups.contains(g))
        })
        .collect();
    movers.sort_by_key(|e| e.id);
    let mut covered: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut out: Vec<Suggestion> = Vec::new();
    for e in movers {
        let primaries: Vec<i64> = e
            .groups
            .iter()
            .filter(|(_, r)| *r == MuscleRole::Primary)
            .map(|(g, _)| *g)
            .collect();
        // Include only if it warms a session group nothing chosen yet covers.
        if primaries
            .iter()
            .any(|g| session_groups.contains(g) && !covered.contains(g))
        {
            for g in &primaries {
                covered.insert(*g);
            }
            let gname = primaries
                .iter()
                .find_map(|g| group_name.get(g).cloned())
                .unwrap_or_default();
            out.push(Suggestion {
                exercise_id: e.id,
                exercise_name: e.name.clone(),
                pattern: e.pattern,
                kind: SuggestionKind::Warmup,
                sets: WARMUP_SETS,
                rep_low: None,
                rep_high: None,
                load_kg: None,
                hold_s: None,
                group: gname,
                substituted_for: None,
                explanation: None,
            });
        }
    }

    // Ramp-in: the first weighted work item gets one light set (~half load) to
    // groove the movement before the working sets.
    if let Some(w) = work
        .iter()
        .find(|s| s.kind == SuggestionKind::Work && s.load_kg.is_some())
        && let (Some(ex), Some(load)) = (ex_by_id.get(&w.exercise_id), w.load_kg)
    {
        let loads = owned_loads(ex, &input.equipment_loads);
        out.push(Suggestion {
            exercise_id: w.exercise_id,
            exercise_name: w.exercise_name.clone(),
            pattern: w.pattern,
            kind: SuggestionKind::Warmup,
            sets: WARMUP_SETS,
            rep_low: w.rep_high, // an easy set of the top of the range
            rep_high: w.rep_high,
            load_kg: Some(snap(&loads, load * RAMP_FRACTION)),
            hold_s: None,
            group: w.group.clone(),
            substituted_for: None,
            explanation: None,
        });
    }
    out
}

/// Choose the best exercise for a muscle group given mode + location, or `None`
/// if nothing that trains it as a primary is doable here.
#[allow(clippy::too_many_arguments)]
fn pick_for_group(
    input: &PacingInput,
    abilities: &std::collections::HashMap<i64, Ability>,
    group_id: i64,
    group_name: &str,
    deficit: f64,
    recovery: f64,
    now: NaiveDateTime,
) -> Option<Suggestion> {
    let mode = input.mode;
    let avail = input.available_equipment.as_ref();
    let doable = |eq: &[i64]| avail.is_none_or(|a| eq.iter().all(|e| a.contains(e)));
    // A work exercise for this group: trains it as a primary and isn't a warm-up
    // move (those are the warm-up block's alone, and credit no training volume).
    let trains = |e: &ExerciseInfo| {
        !e.warmup
            && e.groups
                .iter()
                .any(|(g, r)| *g == group_id && *r == MuscleRole::Primary)
    };
    // Fresher stimulus scores higher (0..1 over ~3 weeks); never-done = max.
    let recency = |id: i64| -> f64 {
        match input
            .history
            .iter()
            .filter(|s| s.exercise_id == id)
            .map(|s| s.logged_at)
            .max()
        {
            Some(t) => ((now - t).num_hours() as f64 / 24.0).min(21.0) / 21.0,
            None => 1.0,
        }
    };
    let score = |e: &ExerciseInfo| mode_fit(mode, e) * 2.0 + recency(e.id);

    // Deterministic best (score desc, id asc) over a filter.
    let best = |f: &dyn Fn(&ExerciseInfo) -> bool| -> Option<&ExerciseInfo> {
        input.exercises.iter().filter(|e| f(e)).max_by(|a, b| {
            score(a)
                .partial_cmp(&score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.id.cmp(&a.id)) // lower id wins ties (reverse in max)
        })
    };

    let ideal = best(&|e| trains(e));
    // When the ideal's kit is missing here, substitute like-for-like: prefer a
    // doable exercise with the ideal's *metric* (swapping a rep pull for a max
    // hold is a different ask, not a substitute), falling back to any doable one.
    let chosen = match ideal {
        Some(i) if doable(&i.equipment) => i,
        Some(i) => best(&|e| trains(e) && doable(&e.equipment) && e.metric == i.metric)
            .or_else(|| best(&|e| trains(e) && doable(&e.equipment)))?,
        None => return None,
    };
    let substituted_for = match (avail, ideal) {
        (Some(_), Some(i)) if i.id != chosen.id => Some(i.name.clone()),
        _ => None,
    };
    // Hold progression on a low-readiness day.
    let advance = !matches!(input.readiness, Some(r) if r.score < READINESS_HOLD_BELOW);
    let loads = owned_loads(chosen, &input.equipment_loads);
    // Untrusted ability (never done, or only stale data) → measure instead of
    // prescribing a false-precision number.
    let ability = abilities.get(&chosen.id);
    let assessing = matches!(
        ability::confidence_of(abilities, chosen.id),
        Confidence::Low | Confidence::None
    );
    let (sets, rep_low, rep_high, load_kg, hold_s) = if assessing {
        assess(chosen, ability, &loads)
    } else {
        prescribe(chosen, ability, mode, advance, &loads)
    };
    let kind = if assessing {
        SuggestionKind::Assess
    } else {
        SuggestionKind::Work
    };
    let explanation = Explanation {
        deficit,
        recovery,
        confidence: ability::confidence_of(abilities, chosen.id),
        e1rm: ability.and_then(|a| a.e1rm),
        readiness: input.readiness.map(|r| r.band),
    };
    Some(Suggestion {
        exercise_id: chosen.id,
        exercise_name: chosen.name.clone(),
        pattern: chosen.pattern,
        kind,
        sets,
        rep_low,
        rep_high,
        load_kg,
        hold_s,
        group: group_name.to_string(),
        substituted_for,
        explanation: Some(explanation),
    })
}

/// Evaluate the coach verdict for `now` (local time).
pub fn evaluate(input: &PacingInput, now: NaiveDateTime) -> PacingNow {
    let s = &input.settings;
    let hour = now.hour() as i32;
    let within_window = hour >= s.window_start_hour && hour < s.window_end_hour;
    // Past the window's end: coach goes quiet and defers to tomorrow (the single
    // evening line; you can still train + log, it just won't nudge).
    let after_window = hour >= s.window_end_hour;
    let minutes_since_last_set = input.last_set_at.map(|t| (now - t).num_minutes());
    let spacing_ok = minutes_since_last_set.is_none_or(|m| m >= s.min_rest_min as i64);

    let ex_by_id: HashMap<i64, &ExerciseInfo> = input.exercises.iter().map(|e| (e.id, e)).collect();

    // Per-exercise ability (RPE-aware e1RM / best reps / best hold, decayed for
    // staleness) — the basis every prescription derives from. Computed once.
    let abilities = ability::abilities(&input.history, now);

    // --- credit volume into rolling / 8-week-avg / recovery windows ---
    let roll_cut = now - Duration::days(ROLLING_DAYS);
    let hist_cut = now - Duration::days(HISTORY_WEEKS * 7);
    let today = now.date();
    // Region per group, for the graded recovery horizon.
    let region_of: HashMap<i64, Region> = input.groups.iter().map(|g| (g.id, g.region)).collect();

    let mut current: HashMap<i64, f64> = HashMap::new();
    let mut avg_sum: HashMap<i64, f64> = HashMap::new();
    // Age-weighted unrecovered load per group: a set counts fully when fresh and
    // ramps to zero over its region's recovery horizon (G6). This grades the old
    // binary "≥3 sets in 36 h" gate.
    let mut unrecovered: HashMap<i64, f64> = HashMap::new();
    let mut done_today = 0i32;
    let mut raw_hist = 0i32;
    for set in &input.history {
        let Some(ex) = ex_by_id.get(&set.exercise_id) else {
            continue;
        };
        // Warm-up sets are prep, not training: they credit no volume and don't
        // count toward the day's target (so they never eat it).
        if ex.warmup {
            continue;
        }
        if set.logged_at.date() == today {
            done_today += 1;
        }
        if set.logged_at >= hist_cut {
            raw_hist += 1;
        }
        let age_h = (now - set.logged_at).num_minutes().max(0) as f64 / 60.0;
        for (g, role) in &ex.groups {
            let credit = match role {
                MuscleRole::Primary => 1.0,
                MuscleRole::Secondary => SECONDARY_CREDIT,
                MuscleRole::Stabilizer => STABILIZER_CREDIT,
            };
            if set.logged_at >= hist_cut {
                *avg_sum.entry(*g).or_default() += credit;
            }
            if set.logged_at >= roll_cut {
                *current.entry(*g).or_default() += credit;
            }
            // Unrecovered contribution: full when fresh, linearly gone by the
            // region's horizon (a set past it no longer holds the group back).
            let horizon = region_of.get(g).copied().map_or(48.0, recovery_horizon);
            if age_h < horizon {
                *unrecovered.entry(*g).or_default() += credit * (1.0 - age_h / horizon);
            }
        }
    }

    // --- one recovery factor on the per-group target ---
    // Biometric readiness (when health has data) is primary and supersedes the
    // crude volume-spike proxy; without it we fall back to that proxy.
    let avg_weekly_total: f64 = avg_sum.values().sum::<f64>() / HISTORY_WEEKS as f64;
    let last7_total: f64 = current.values().sum();
    let volume_deload = avg_weekly_total > 0.0 && last7_total > DELOAD_RATIO * avg_weekly_total;
    let recovery_scale = match input.readiness {
        Some(r) => 0.75 + 0.5 * r.score, // 0.75 (spent) .. 1.25 (fully recovered)
        None if volume_deload => DELOAD_SCALE,
        None => 1.0,
    };
    // Only reported (and true) on the no-biometric fallback path.
    let deload = input.readiness.is_none() && volume_deload;
    let days_scale = (input.days_per_week as f64 / 4.0).clamp(0.5, 2.0);

    // --- per-group balance + focus candidates (non-recovering, in deficit) ---
    let mut balances: Vec<GroupBalance> = Vec::new();
    // (group id, name, effective_deficit [priority + sizing], raw deficit, recovery)
    let mut candidates: Vec<(i64, String, f64, f64, f64)> = Vec::new();
    for gm in &input.groups {
        let cur = *current.get(&gm.id).unwrap_or(&0.0);
        let avg = avg_sum.get(&gm.id).copied().unwrap_or(0.0) / HISTORY_WEEKS as f64;
        let emph = if input.emphasis == Some(gm.region) {
            EMPHASIS_MULT
        } else {
            1.0
        };
        let base = 0.5 * DEFAULT_WEEKLY_SETS + 0.5 * avg;
        let target =
            (base * region_mult(input.mode, gm.region) * emph * days_scale * recovery_scale)
                .max(3.0);
        let deficit = ((target - cur) / target).clamp(0.0, 1.0);
        // Graded recovery: 0 (just hammered) → 1 (fully recovered). Scale the
        // deficit by it, so a half-recovered group is a half-priority, and the
        // old hard gate falls out as the fraction-≈0 case.
        let recovery =
            (1.0 - unrecovered.get(&gm.id).copied().unwrap_or(0.0) / RECOVERY_SETS).clamp(0.0, 1.0);
        let effective_deficit = deficit * recovery;
        let recovering = recovery < RECOVERED_FRACTION;
        if effective_deficit > MIN_EFFECTIVE_DEFICIT {
            candidates.push((gm.id, gm.name.clone(), effective_deficit, deficit, recovery));
        }
        balances.push(GroupBalance {
            group: gm.name.clone(),
            region: gm.region,
            current: cur,
            target,
            deficit,
            recovering,
        });
    }
    // Balance view: most-in-deficit first.
    balances.sort_by(|a, b| {
        b.deficit
            .partial_cmp(&a.deficit)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Focus order: biggest deficit first, tie-break group id.
    candidates.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    // --- session-size target from personal weekly volume (sizes the plan) ---
    let avg_weekly_sets = if raw_hist > 0 {
        raw_hist as f64 / HISTORY_WEEKS as f64
    } else {
        24.0 // cold-start default ≈ 6 sets × 4 days
    };
    // Scale the day's set count by the same recovery factor as the group targets,
    // so a low-readiness day is fewer sets, not just lighter ones.
    let day_target_sets = ((avg_weekly_sets / input.days_per_week.max(1) as f64 * recovery_scale)
        .round() as i32)
        .clamp(3, 15);

    // --- build the ordered session plan ---
    // Resolve each in-deficit, recovered group to a doable exercise; size + order
    // into the day's plan. The head is "next up"; the rest is the session.
    let mut resolved: Vec<(Suggestion, f64, u8)> = Vec::new();
    for (gid, gname, eff_deficit, deficit, recovery) in &candidates {
        if let Some(sug) = pick_for_group(input, &abilities, *gid, gname, *deficit, *recovery, now)
        {
            let t = ex_by_id.get(&sug.exercise_id).map(|e| tier(e)).unwrap_or(4);
            // Size by the recovery-scaled deficit; explain with the raw values.
            resolved.push((sug, *eff_deficit, t));
        }
    }
    let work = build_plan(resolved, day_target_sets);
    // Prepend the warm-up block (mobility for the session's groups + a ramp-in on
    // the first heavy lift). Warm-ups lead; the head becomes "start here".
    let group_name: HashMap<i64, String> = input
        .groups
        .iter()
        .map(|g| (g.id, g.name.clone()))
        .collect();
    let warmup = build_warmup(&work, input, &ex_by_id, &group_name);
    let plan: Vec<Suggestion> = warmup.into_iter().chain(work).collect();
    // "Next up" for the nudge + Android trigger is the first *training* item, not
    // the warm-up that leads the visible plan.
    let suggestion = plan
        .iter()
        .find(|s| s.kind != SuggestionKind::Warmup)
        .cloned();

    let state = if input.history.is_empty() {
        PacingState::Fresh
    } else if suggestion.is_some() {
        PacingState::Active
    } else {
        PacingState::Rest
    };

    // Burn-down vs window elapsed → nudge when behind (never dump the day at night).
    let now_min = (hour * 60 + now.minute() as i32) as f64;
    let win_start = (s.window_start_hour * 60) as f64;
    let win_end = (s.window_end_hour * 60).max(s.window_start_hour * 60 + 1) as f64;
    let elapsed = ((now_min - win_start) / (win_end - win_start)).clamp(0.0, 1.0);
    let has_work = suggestion.is_some() && done_today < day_target_sets;
    let behind = has_work && (done_today as f64) < elapsed * day_target_sets as f64;
    // `within_window` already implies before the end, so no separate cutoff check.
    let nudge = within_window && has_work && spacing_ok && behind;

    // A short day-state clause, prepended to an active suggestion's reason — the
    // coach says it in the one sentence it speaks, rather than the UI growing
    // status widgets. `deload` only fires when readiness is absent (it's the
    // no-biometric fallback), so the two never compete for the slot.
    let day_note = match input.readiness.map(|r| r.band) {
        Some(Band::High) => Some("Recovered — good day to push."),
        Some(Band::Low) => Some("Low readiness — keeping it light."),
        None if deload => Some("Volume's run hot lately — easing off."),
        _ => None,
    };

    let reason = if suggestion.is_none() {
        if state == PacingState::Rest {
            "You're balanced and recovered — rest up, or an easy optional set.".to_string()
        } else {
            "Nothing doable here right now.".to_string()
        }
    } else if !has_work {
        "You're on top of it today — nice work.".to_string()
    } else if after_window {
        "It's late — this rolls to tomorrow.".to_string()
    } else if !within_window {
        format!(
            "Outside your training window ({:02}:00–{:02}:00).",
            s.window_start_hour, s.window_end_hour
        )
    } else if !spacing_ok {
        format!(
            "Just trained {}m ago — take a breather.",
            minutes_since_last_set.unwrap_or(0)
        )
    } else if let Some(sug) = &suggestion {
        if behind {
            format!(
                "{} × {} ({}) — you're a bit light there this week.",
                sug.sets, sug.exercise_name, sug.group
            )
        } else {
            format!(
                "Next up: {} × {} ({}).",
                sug.sets, sug.exercise_name, sug.group
            )
        }
    } else {
        String::new()
    };
    // Weave the day-state clause in when we're actually suggesting a set to do now.
    let suggesting_now = suggestion.is_some() && has_work && within_window && spacing_ok;
    let reason = match day_note {
        Some(note) if suggesting_now => format!("{note} {reason}"),
        _ => reason,
    };

    PacingNow {
        state,
        deload,
        readiness: input.readiness,
        nudge,
        reason,
        within_window,
        after_window,
        spacing_ok,
        minutes_since_last_set,
        day_target_sets,
        day_done_sets: done_today,
        groups: balances,
        suggestion,
        plan,
    }
}
