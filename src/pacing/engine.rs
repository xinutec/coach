//! The dynamic coaching engine: a pure function from (history, mode, instant) to
//! a verdict. No I/O, no clock — the caller passes `now` (user-local tz). It
//! computes rolling muscle-group volume, grades each group's recovery, turns the
//! two into a **need vector** over the muscle-group space, and then *covers* that
//! need with the kit actually present: a greedy set-cover ([`super::cover`]) picks
//! the day's sets one at a time, each time taking the one that pays down the most
//! remaining need. No program, no weekly plan, no stored state.
//!
//! Two invariants are carried by types rather than by care:
//!
//! - an exercise appears in the plan **once**, with the set count it earned — the
//!   cover accumulates by exercise, so a duplicate is unrepresentable;
//! - a working load is only ever derived from an ability the engine has actually
//!   measured ([`super::dose::Known`]) and only ever snapped to a weight the
//!   athlete owns ([`super::dose::Inventory`], non-empty by construction).
//!
//! All coefficients below are labelled heuristics, tunable — targets are anchored
//! to the user's own history to avoid false-precision absolute landmarks.

use std::collections::HashMap;

use chrono::{Duration, NaiveDateTime, Timelike};

use crate::exercise::types::{Metric, Pattern};
use crate::muscle::types::{MuscleRole, Region};
use crate::settings::types::Mode;

use super::ability::{self, Ability};
use super::cover::{self, ByGroup, Candidate, GroupIx};
use super::dose::{Dose, Inventory, Known, Measure, RepTarget};
use super::types::{
    Band, ExerciseInfo, Explanation, GroupBalance, Kit, PacingInput, PacingNow, PacingState,
    Suggestion, SuggestionKind,
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
/// Sets for a warm-up item (mobility drill or ramp-in) — one is enough to prep.
const WARMUP_SETS: i32 = 1;
/// A ramp-in set runs at this fraction of the first heavy lift's working load.
const RAMP_FRACTION: f64 = 0.5;

/// Fewest sets of a *work* movement once it's in the session at all — the minimum
/// effective dose. Setting up for a lift and doing one set of it wastes the setup;
/// below this the day fragments into eight movements you barely touch.
const MIN_WORK_SETS: i32 = 2;
/// Most sets of one exercise a single session will ever take: past this, more of
/// the same movement buys little the next movement wouldn't buy more of. (A
/// calibration set is capped at 1 instead — measuring the same thing twice in a
/// session tells you nothing the first didn't.)
const MAX_SETS_PER_EXERCISE: i32 = 4;
/// Effective sets one muscle group can usefully absorb in a single session — the
/// ceiling on how much of its weekly deficit today is allowed to chase. Same
/// scale as `RECOVERY_SETS`: beyond it you're digging a recovery hole, not
/// training. This is what stops the cover pouring the whole day into one group.
const MAX_GROUP_SETS_PER_DAY: f64 = 3.0;

// ---- tunable heuristics ----------------------------------------------------
const ROLLING_DAYS: i64 = 7; // rolling-volume window (a training week)
const HISTORY_WEEKS: i64 = 8; // personal-average window
const RECOVERY_SETS: f64 = 3.0; // unrecovered load (age-weighted sets) that fully gates a group
const RECOVERED_FRACTION: f64 = 0.85; // ≥ this recovery fraction → shown as recovered
const DEFAULT_WEEKLY_SETS: f64 = 10.0; // literature maintenance→growth anchor
const SECONDARY_CREDIT: f64 = 0.5; // a synergist (secondary) counts half a set
const STABILIZER_CREDIT: f64 = 0.25; // an isometric stabilizer counts a quarter
const EMPHASIS_MULT: f64 = 1.5;
const DELOAD_RATIO: f64 = 1.6; // last-7d volume this far above avg → auto-deload
const DELOAD_SCALE: f64 = 0.6;

/// What one set of an exercise credits into a group it trains in this role.
fn role_credit(role: MuscleRole) -> f64 {
    match role {
        MuscleRole::Primary => 1.0,
        MuscleRole::Secondary => SECONDARY_CREDIT,
        MuscleRole::Stabilizer => STABILIZER_CREDIT,
    }
}

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

/// Rep range for a mode + metric (holds are seconds, handled in [`prescribe`]).
fn rep_range(mode: Mode, weighted: bool) -> RepTarget {
    let (low, high) = match mode {
        Mode::Strength => {
            if weighted {
                (3, 6)
            } else {
                (5, 8)
            }
        }
        Mode::Balanced => {
            if weighted {
                (6, 10)
            } else {
                (8, 12)
            }
        }
        Mode::Skills => (3, 6),
        Mode::Conditioning => {
            if weighted {
                (12, 20)
            } else {
                (15, 25)
            }
        }
    };
    RepTarget { low, high }
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
        // Balanced used to score *everything* a flat 0.8 — which is not a
        // preference but the absence of one, and it handed the decision to an
        // arbitrary tie-break on exercise id. That's how a missing lat pull-down
        // once got "substituted" by an L-sit hold: the engine genuinely could not
        // tell a rep-out from an isometric, so the lower id won. A balanced week is
        // built on rep and weighted work, with holds as a narrower accessory
        // stimulus — say so, and the tie-break never has to decide it.
        Mode::Balanced => match ex.metric {
            Metric::WeightedReps | Metric::Reps => 1.0,
            Metric::Hold => 0.6,
        },
    }
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

/// An exercise's metric **together with the weights it can actually be loaded
/// with here** — resolved once, when candidates are built.
///
/// This is what makes [`prescribe`] and [`assess`] total. A weighted lift with no
/// registered weights never becomes a `Loaded`, so it never becomes a candidate,
/// so neither function has an "and what if there's no weight?" branch to fall
/// through into a guess.
enum Loaded {
    Weighted(Inventory),
    Reps,
    Hold,
}

/// The weights this exercise can actually be built with here (the service worked
/// them out from the kit *and* how many implements the movement needs). `None` for
/// a weighted lift = not loadable here, so it isn't selectable and the verdict
/// says why.
fn loadable(ex: &ExerciseInfo, exercise_loads: &HashMap<i64, Vec<f64>>) -> Option<Loaded> {
    match ex.metric {
        Metric::Reps => Some(Loaded::Reps),
        Metric::Hold => Some(Loaded::Hold),
        Metric::WeightedReps => {
            let loads = exercise_loads.get(&ex.id).cloned().unwrap_or_default();
            Inventory::new(loads).map(Loaded::Weighted)
        }
    }
}

/// Prescribe from a **trusted** ability estimate — the type is the proof: there
/// is no way to call this for an exercise the athlete hasn't recently
/// demonstrated (see [`Known`]).
///
/// Weighted work autoregulates: the working load is derived from the decayed e1RM
/// so a layoff self-corrects to a lighter start, and the load only steps up when
/// logged sets raise the estimate past the next owned weight (double progression,
/// but *earned* and snapped to what you own). `advance = false` (low readiness)
/// leaves more in reserve — keep it light, don't chase a PR.
fn prescribe(loaded: &Loaded, ability: &Known, mode: Mode, advance: bool) -> Dose {
    let reserve = if advance {
        TARGET_RIR
    } else {
        TARGET_RIR + LOW_READINESS_EXTRA_RIR
    };
    match loaded {
        Loaded::Weighted(inv) => {
            let range = rep_range(mode, true);
            match ability.e1rm {
                Some(e) => {
                    // Working load: the weight you own nearest the one that puts
                    // the top of the range within reach at the target reserve.
                    let load = inv.snap(load_for(e, range.high as f64, reserve));
                    // At that (discrete) weight, how many reps are actually in
                    // reach? Clamped into the range — this is the rep target that
                    // climbs to the top before the weight is allowed to step.
                    let low =
                        (reps_at(e, load, reserve).round() as i32).clamp(range.low, range.high);
                    Dose::Weighted {
                        load,
                        reps: RepTarget { low, ..range },
                    }
                }
                // Trusted for reps/holds but no e1RM (all its sets were logged
                // without a load): open the full range at the lightest weight.
                None => Dose::Weighted {
                    load: inv.lightest(),
                    reps: range,
                },
            }
        }
        Loaded::Reps => {
            // Only lever is reps: climb toward the top of the range off the
            // decayed best; hold (no climb) on a low-readiness day.
            let range = rep_range(mode, false);
            let low = match ability.best_reps {
                Some(best) => {
                    let aim = if advance { best + 1 } else { best };
                    aim.clamp(range.low, range.high)
                }
                None => range.low,
            };
            Dose::Bodyweight {
                reps: RepTarget { low, ..range },
            }
        }
        Loaded::Hold => {
            let base = ability.best_hold.unwrap_or(COLD_HOLD_S);
            Dose::Hold {
                secs: if advance { base + HOLD_STEP_S } else { base },
            }
        }
    }
}

/// A calibration set for an exercise whose ability is untrusted (never done, or
/// only stale data): the engine measures instead of prescribing a false-precision
/// number (G3). The logged result feeds the ability model, so the next verdict
/// prescribes from it. `stale` is any decayed estimate we have — good enough to
/// open a build-up safely, not good enough to prescribe from.
fn assess(loaded: &Loaded, stale: Option<&Ability>) -> Measure {
    match loaded {
        Loaded::Weighted(inv) => {
            let start = match stale.and_then(|a| a.e1rm) {
                // Open from the stale estimate, with reserve — a safe build-up.
                Some(e) => inv.snap(load_for(
                    e,
                    ASSESS_WEIGHTED_REPS as f64,
                    LOW_READINESS_EXTRA_RIR,
                )),
                None => inv.lightest(),
            };
            Measure::BuildUp {
                start,
                reps: ASSESS_WEIGHTED_REPS,
            }
        }
        Loaded::Reps => Measure::Amrap,
        Loaded::Hold => Measure::MaxHold,
    }
}

/// Which block of the session an exercise belongs in — the classic order that
/// puts demanding, technical work while the nervous system is fresh and leaves
/// finishers for last. Lower runs earlier. Tier 1 is the warm-up block.
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

/// Per-group state the cover and the explanations both read.
struct Groups {
    /// Dense index per muscle-group id.
    ix: HashMap<i64, GroupIx>,
    name: Vec<String>,
    id: Vec<i64>,
    /// Remaining weekly deficit as a fraction (0 = at target, 1 = untrained).
    deficit: ByGroup<f64>,
    /// Graded recovery, 0 (just hammered) .. 1 (fully recovered).
    recovery: ByGroup<f64>,
    /// What today should chase: effective sets still wanted, capped at what one
    /// session can usefully deliver, discounted by how recovered the group is.
    need: ByGroup<f64>,
}

/// A selectable exercise, with everything the cover and the prescription need.
struct Cand<'a> {
    ex: &'a ExerciseInfo,
    loaded: Loaded,
    /// The group this set pays into most — the label the plan item carries.
    label: Option<GroupIx>,
}

/// Build the selectable candidates for this location: catalog minus warm-up moves,
/// minus anything the kit can't do, minus weighted lifts with no registered
/// weights or enough implements to go round (the service names those in the
/// verdict's notices — a drop the athlete can act on, not a silent gap).
fn candidates<'a>(
    input: &'a PacingInput,
    kit: &Kit,
    abilities: &HashMap<i64, Ability>,
    groups: &Groups,
    now: NaiveDateTime,
) -> (Vec<Cand<'a>>, Vec<Candidate>) {
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

    let mut cands = Vec::new();
    let mut scored = Vec::new();

    for ex in &input.exercises {
        // Warm-up moves are the warm-up block's alone (and credit no volume).
        if ex.warmup || !kit.has_all(&ex.equipment) {
            continue;
        }
        // Not loadable here (no registered weights, or not enough implements to go
        // round) → no honest load exists, so it isn't selectable. The service, which
        // knows *why*, says so in the verdict's notices.
        let Some(loaded) = loadable(ex, &input.exercise_loads) else {
            continue;
        };

        // What one set pays into each group: role credit, discounted by how
        // recovered that group is (a hammered group can't bank the stimulus).
        let mut credit = ByGroup::filled(groups.name.len(), 0.0);
        for (gid, role) in &ex.groups {
            if let Some(&i) = groups.ix.get(gid) {
                credit[i] = role_credit(*role) * groups.recovery[i];
            }
        }
        // The group this most pays into — the item's label. Ties → lower group id.
        let label = credit
            .iter()
            .filter(|(i, c)| *c > 0.0 && groups.need[*i] > 0.0)
            .max_by(|a, b| {
                let (pa, pb) = (groups.need[a.0] * a.1, groups.need[b.0] * b.1);
                pa.total_cmp(&pb)
                    .then(groups.id[b.0.0].cmp(&groups.id[a.0.0]))
            })
            .map(|(i, _)| i);

        // A calibration set is a measurement: exactly one, always. Trusted work
        // takes its minimum effective dose, and may earn up to the ceiling.
        let (min, cap) = match Known::of(abilities, ex.id) {
            Some(_) => (MIN_WORK_SETS, MAX_SETS_PER_EXERCISE),
            None => (1, 1),
        };
        scored.push(Candidate {
            id: ex.id,
            credit,
            weight: mode_fit(input.mode, ex) * 2.0 + recency(ex.id),
            min,
            cap,
        });
        cands.push(Cand { ex, loaded, label });
    }

    (cands, scored)
}

/// The exercise the athlete *would* be doing for this group if the kit allowed —
/// the best-scoring one that trains it as a primary, ignoring what's present.
/// Reported when it isn't what we chose, so a swap explains itself.
fn blocked_ideal(
    input: &PacingInput,
    weight: &dyn Fn(&ExerciseInfo) -> f64,
    group_id: i64,
    chosen_id: i64,
) -> Option<String> {
    input
        .exercises
        .iter()
        .filter(|e| {
            !e.warmup
                && e.groups
                    .iter()
                    .any(|(g, r)| *g == group_id && *r == MuscleRole::Primary)
        })
        .max_by(|a, b| {
            weight(a).total_cmp(&weight(b)).then(b.id.cmp(&a.id)) // lower id wins ties (reverse in max)
        })
        .filter(|ideal| ideal.id != chosen_id)
        .map(|ideal| ideal.name.clone())
}

/// Build the warm-up block for a work plan: mobility prep for the muscle groups
/// the session trains, plus a light ramp-in set on the first heavy lift. Warm-ups
/// credit no volume and are the only place warm-up-tagged moves appear. Ordered
/// first. Deterministic: mobility by exercise id, one per not-yet-covered group.
fn build_warmup(
    work: &[Suggestion],
    input: &PacingInput,
    kit: &Kit,
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

    // Mobility: warm-up-tagged moves for the session's groups, doable here. Take
    // one per still-uncovered group so we don't stack redundant drills.
    let mut movers: Vec<&ExerciseInfo> = input
        .exercises
        .iter()
        .filter(|e| {
            e.warmup
                && kit.has_all(&e.equipment)
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
        && let Some(Loaded::Weighted(inv)) = loadable(ex, &input.exercise_loads)
    {
        out.push(Suggestion {
            exercise_id: w.exercise_id,
            exercise_name: w.exercise_name.clone(),
            pattern: w.pattern,
            kind: SuggestionKind::Warmup,
            sets: WARMUP_SETS,
            rep_low: w.rep_high, // an easy set of the top of the range
            rep_high: w.rep_high,
            load_kg: Some(inv.snap(load * RAMP_FRACTION)),
            hold_s: None,
            group: w.group.clone(),
            substituted_for: None,
            explanation: None,
        });
    }
    out
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
            let credit = role_credit(*role);
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

    // --- per-group balance + the need vector the session covers ---
    let n = input.groups.len();
    let mut groups = Groups {
        ix: input
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| (g.id, GroupIx(i)))
            .collect(),
        name: input.groups.iter().map(|g| g.name.clone()).collect(),
        id: input.groups.iter().map(|g| g.id).collect(),
        deficit: ByGroup::filled(n, 0.0),
        recovery: ByGroup::filled(n, 0.0),
        need: ByGroup::filled(n, 0.0),
    };
    let mut balances: Vec<GroupBalance> = Vec::new();
    for (i, gm) in input.groups.iter().enumerate() {
        let ix = GroupIx(i);
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
        // Graded recovery: 0 (just hammered) → 1 (fully recovered).
        let recovery =
            (1.0 - unrecovered.get(&gm.id).copied().unwrap_or(0.0) / RECOVERY_SETS).clamp(0.0, 1.0);

        groups.deficit[ix] = ((target - cur) / target).clamp(0.0, 1.0);
        groups.recovery[ix] = recovery;
        // What today chases: the sets still owed this week, capped at what one
        // session can usefully give the group, and discounted by its recovery.
        // In *effective set* units — the same units an exercise's credit pays in,
        // which is what makes the cover's subtraction mean something physical.
        groups.need[ix] = (target - cur).clamp(0.0, MAX_GROUP_SETS_PER_DAY) * recovery;

        balances.push(GroupBalance {
            group: gm.name.clone(),
            region: gm.region,
            current: cur,
            target,
            deficit: groups.deficit[ix],
            recovering: recovery < RECOVERED_FRACTION,
        });
    }
    // Balance view: most-in-deficit first.
    balances.sort_by(|a, b| b.deficit.total_cmp(&a.deficit));

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

    // --- cover the need with the kit that's actually here ---
    // No location → we don't know what's doable, and we don't guess: no plan.
    let plan = match &input.kit {
        Some(kit) => plan_session(
            input,
            kit,
            &abilities,
            &groups,
            day_target_sets,
            &ex_by_id,
            now,
        ),
        None => Vec::new(),
    };
    // Kit the coach had to leave out — worked out by the service, which knows why.
    // Only worth saying when there's a session for it to be a hole in.
    let notices = if plan.is_empty() {
        Vec::new()
    } else {
        input.notices.clone()
    };

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

    let reason = if input.kit.is_none() {
        "Tell me where you're training and I'll plan the session.".to_string()
    } else if suggestion.is_none() {
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
        notices,
    }
}

/// Cover today's need with the kit present: greedy set-cover over the doable
/// catalog, each chosen exercise prescribed (trusted ability) or assessed
/// (untrusted), then ordered into a session and led by a warm-up block.
fn plan_session(
    input: &PacingInput,
    kit: &Kit,
    abilities: &HashMap<i64, Ability>,
    groups: &Groups,
    budget: i32,
    ex_by_id: &HashMap<i64, &ExerciseInfo>,
    now: NaiveDateTime,
) -> Vec<Suggestion> {
    let (cands, scored) = candidates(input, kit, abilities, groups, now);
    let chosen = cover::select(&scored, &groups.need, budget);

    // Hold progression on a low-readiness day.
    let advance = !matches!(input.readiness, Some(r) if r.score < READINESS_HOLD_BELOW);
    let weight = |e: &ExerciseInfo| mode_fit(input.mode, e) * 2.0;

    // Only the *first* exercise the cover picks for a group is a stand-in for that
    // group's blocked ideal; anything it picks afterwards is simply more work, not
    // a second substitute for the same thing.
    let mut stood_in: std::collections::HashSet<GroupIx> = std::collections::HashSet::new();

    let mut work: Vec<(Suggestion, u8)> = Vec::new();
    for pick in chosen {
        let c = &cands[pick.index];
        let sets = pick.sets;
        let ability = abilities.get(&c.ex.id);
        let (kind, dose, measure) = match Known::of(abilities, c.ex.id) {
            Some(known) => (
                SuggestionKind::Work,
                Some(prescribe(&c.loaded, &known, input.mode, advance)),
                None,
            ),
            None => (
                SuggestionKind::Assess,
                None,
                Some(assess(&c.loaded, ability)),
            ),
        };
        // Wire shape: the sum types above are the engine's truth; these flat
        // fields are their rendering for the UI + Android.
        let (rep_low, rep_high, load_kg, hold_s) = match (&dose, &measure) {
            (Some(Dose::Weighted { load, reps }), _) => {
                (Some(reps.low), Some(reps.high), Some(*load), None)
            }
            (Some(Dose::Bodyweight { reps }), _) => (Some(reps.low), Some(reps.high), None, None),
            (Some(Dose::Hold { secs }), _) => (None, None, None, Some(*secs)),
            (_, Some(Measure::BuildUp { start, reps })) => {
                (Some(*reps), Some(*reps), Some(*start), None)
            }
            // AMRAP / max hold — the open fields say "as many clean reps / as long
            // as clean form holds", which is exactly what we're measuring.
            (_, Some(Measure::Amrap) | Some(Measure::MaxHold)) => (None, None, None, None),
            (None, None) => (None, None, None, None),
        };

        let (group, explanation, substituted_for) = match c.label {
            Some(ix) => (
                groups.name[ix.0].clone(),
                Some(Explanation {
                    deficit: groups.deficit[ix],
                    recovery: groups.recovery[ix],
                    pays: pick.pays,
                    confidence: ability::confidence_of(abilities, c.ex.id),
                    e1rm: ability.and_then(|a| a.e1rm),
                    readiness: input.readiness.map(|r| r.band),
                }),
                stood_in
                    .insert(ix)
                    .then(|| blocked_ideal(input, &weight, groups.id[ix.0], c.ex.id))
                    .flatten(),
            ),
            None => (String::new(), None, None),
        };

        work.push((
            Suggestion {
                exercise_id: c.ex.id,
                exercise_name: c.ex.name.clone(),
                pattern: c.ex.pattern,
                kind,
                sets,
                rep_low,
                rep_high,
                load_kg,
                hold_s,
                group,
                substituted_for,
                explanation,
            },
            tier(c.ex),
        ));
    }

    // Present in training order: tier, then the order the cover picked them
    // (biggest marginal gain first), which the stable sort preserves.
    work.sort_by_key(|(_, t)| *t);
    let work: Vec<Suggestion> = work.into_iter().map(|(s, _)| s).collect();

    let group_name: HashMap<i64, String> = input
        .groups
        .iter()
        .map(|g| (g.id, g.name.clone()))
        .collect();
    let warmup = build_warmup(&work, input, kit, ex_by_id, &group_name);
    warmup.into_iter().chain(work).collect()
}
