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

use super::ability::{self, Ability, Confidence};
use super::cover::{self, ByGroup, Candidate, GroupIx};
use super::dose::{Dose, Inventory, Known, Measure, RepTarget};
use super::residual::{self, Residual};
use super::types::{
    Band, Blocker, ExerciseInfo, Explanation, GroupBalance, Kit, PacingInput, PacingNow,
    PacingState, SetRec, Substitution, Suggestion, SuggestionKind,
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
/// A loaded carry's working duration, and the ceiling it climbs to before the
/// weight steps instead. Double progression, with seconds where the reps go: a
/// carry that has reached the ceiling is asking for more weight, not more walking.
const CARRY_BASE_S: i32 = 30;
const CARRY_TOP_S: i32 = 60;
/// Sets for a warm-up item (mobility drill or ramp-in) — one is enough to prep.
const WARMUP_SETS: i32 = 1;
/// A mobility drill's dose. "Warm up" with no number is a vibe, not an
/// instruction — the athlete asked, reasonably, "how many? for how long?".
/// Deliberately easy: prep, not training.
const WARMUP_REPS: i32 = 10;
const WARMUP_HOLD_S: i32 = 20;
/// Effective sets a muscle group must carry in the committed session before it
/// earns a warm-up slot. One full set's worth — a single primary set (any
/// calibration) or two sets' secondary assist. Below it, a group is touched, not
/// loaded, and prepping it would spend slots the loaded groups need: the round-2
/// field test went into dips, pull-ups and push-ups with two obliques drills and
/// cold shoulders because coverage followed primary *labels*, not session load.
const WARMUP_MIN_LOAD: f64 = 1.0;
/// One mobility drill per this many committed work sets — the warm-up scales
/// with the session it precedes — bounded both ways: even a short session warms
/// its top groups, and even a huge one keeps the block well short of the work.
/// With a gap-free drill catalog, every loaded group would otherwise claim a
/// slot (11 drills before 9 working sets in the round-3 back-test); a coach
/// triages — the heaviest areas get drills, the tail warms up through general
/// movement and the ramp-ins.
const WARMUP_SETS_PER_DRILL: i32 = 3;
const WARMUP_MIN_DRILLS: i32 = 3;
const WARMUP_MAX_DRILLS: i32 = 6;
/// Non-stabilizer muscle groups at which a movement counts as a compound —
/// session ordering runs compounds first (see [`tier`]).
const COMPOUND_BREADTH: usize = 3;
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

/// One-time confirmation need (effective-set units) per session a started movement
/// still owes before its estimate is trusted. Sized to lead a fully-untrained
/// group's coverage — a group can chase at most [`MAX_GROUP_SETS_PER_DAY`] of it —
/// so locking in what you've *begun* comes before broadening into new movements,
/// until the baseline is solid. Zero once High-confidence, so it self-limits: after
/// a couple of sessions on a movement the coach stops asking for it specially and
/// lets ordinary coverage decide. This is the whole of the "calibration phase":
/// there is no phase flag, only an estimate that isn't trusted yet.
const CONFIRM_UNIT: f64 = 5.0;

/// Longest silence between two sets that still counts as the same session. The
/// athlete's real in-session gaps run to ~80 minutes (a home session spread over
/// an afternoon); a morning and an evening visit are two sessions. This is what
/// makes "the plan is committed at the session's first set" decidable from
/// history alone — the engine stays a pure function.
const SESSION_GAP_MIN: i64 = 120;

/// Most never-done movements one session introduces. A calibration day is a few
/// movements learned properly, not a scattershot of one-off sets across every
/// untrained group at once — which is exactly what pure coverage does on day two,
/// when every group you haven't hit yet reads as maximum deficit. Deliberately
/// small: a coach adds two or three new movements at a time and lets you own them
/// before piling on more, and the same holds on a cold start — a focused first
/// session that samples a few patterns beats a broad one nobody can attend to.
const NOVELTY_CAP: i32 = 3;
/// The one-time need (effective sets) to measure the next rung of a variation
/// ladder the athlete has outgrown (G7). Same mechanism as confirmation: knowing
/// what you can do on the movement that replaces a topped-out one *is* a need,
/// so it qualifies the rung into a session even when the group's volume is
/// already covered — otherwise the step-up stays a notice forever while the
/// cover keeps picking other trusted work for the group.
const LADDER_CONFIRM: f64 = 1.0;

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
/// The weekly-volume the session-size estimate starts from, and how many weeks of
/// evidence it is worth. A shrinkage prior: with no history the estimate *is* the
/// anchor, and real weeks pull it toward the athlete's own rate — so there's no
/// cliff between "cold start" and "personal average", which is where the target
/// used to halve itself the moment the first set was logged.
const ANCHOR_WEEKLY_SETS: f64 = 24.0; // ≈ 6 sets × 4 days
const ANCHOR_WEEKS: f64 = 2.0;

/// The one-time confirmation need for a candidate (effective-set units) — the
/// value of turning a *started but unproven* movement into a trusted baseline.
///
/// It fires only for `Medium` confidence: the athlete has one or two recent
/// sessions on the movement, so an estimate exists but isn't yet solid, and another
/// session on *this* movement is worth more than covering a group its muscles have
/// already had this week. Scaled by how many sessions of proof remain, so a
/// barely-started movement is asked for before a nearly-proven one. `None` (never
/// done — nothing to confirm; that's novelty, priced by coverage) and `High`/`Low`
/// (already trusted, or stale and handled by re-assessment) get nothing.
fn confirm_need(confidence: Confidence, sessions_recent: i32) -> f64 {
    match confidence {
        Confidence::Medium => {
            CONFIRM_UNIT * (ability::HIGH_SESSIONS - sessions_recent).max(0) as f64
        }
        Confidence::High | Confidence::Low | Confidence::None => 0.0,
    }
}

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
            // A loaded carry is conditioning almost by definition — time under a
            // weight, with a heart rate to match. It ranks above a static hold and
            // above a heavy set of reps.
            Metric::WeightedHold => 0.9,
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
            // Loaded and real work, but an accessory to the week rather than its
            // spine — between the two.
            Metric::WeightedHold => 0.8,
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
    /// A carry — loaded, like `Weighted`, and so subject to the same rule: no
    /// registered weights means no honest load, so it is never selected.
    WeightedHold(Inventory),
}

/// The weights this exercise can actually be built with here (the service worked
/// them out from the kit *and* how many implements the movement needs). `None` for
/// a weighted lift = not loadable here, so it isn't selectable and the verdict
/// says why.
fn loadable(ex: &ExerciseInfo, exercise_loads: &HashMap<i64, Vec<f64>>) -> Option<Loaded> {
    let inventory = || {
        let loads = exercise_loads.get(&ex.id).cloned().unwrap_or_default();
        Inventory::new(loads)
    };
    match ex.metric {
        Metric::Reps => Some(Loaded::Reps),
        Metric::Hold => Some(Loaded::Hold),
        Metric::WeightedReps => inventory().map(Loaded::Weighted),
        Metric::WeightedHold => inventory().map(Loaded::WeightedHold),
    }
}

/// Prescribe from a **trusted** ability estimate — the type is the proof: there
/// is no way to call this for an exercise the athlete hasn't recently
/// demonstrated (see [`Known`]).
///
/// Weighted work autoregulates: the working load is derived from the decayed e1RM
/// so a layoff self-corrects to a lighter start, and the load only steps up when
/// logged sets raise the estimate past the next owned weight (double progression,
/// but *earned* and snapped to what you own).
///
/// Two things hold it back. `advance = false` (low readiness) leaves more in
/// reserve — keep it light, don't chase a PR. And `feedback`, the prediction-error
/// ledger, answers a session that actually went badly: **one miss holds** the number
/// rather than adding to it, and **two in a row step down** a rung of the owned
/// weights. Without that, ability is a max over decayed sets and a miss pulls
/// nothing down — the athlete gets handed the same number the sets just
/// contradicted, which is how you grind someone into a hole.
///
/// And asking for *more* is a **probe**, not the default: earned by a session
/// that beat the estimate, or periodic after enough consolidation
/// ([`Residual::probe_due`]). Matching your best while failing the ask moves
/// nothing (ability is a max), so without the cadence the same failing +1 was
/// re-asked verbatim every session for weeks — the R4-1 simulation finding.
fn prescribe(
    loaded: &Loaded,
    ability: &Known,
    mode: Mode,
    advance: bool,
    feedback: &Residual,
) -> Dose {
    // A miss is not a day to add load on. (Low readiness already said as much for a
    // different reason; either is enough.)
    let advance = advance && !feedback.wants_hold();
    let back_off = feedback.wants_back_off();
    let probe = advance && feedback.probe_due();
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
                    // the top of the range within reach at the target reserve —
                    // rounding *up* between rungs on a full-effort day. A rung
                    // below caps what the rep range can demonstrate under the
                    // estimate itself, so on a coarse rack every session would
                    // read as a miss no matter how well it went: misses block
                    // progression, three trigger a re-measure, and the loop
                    // repeats (the simulation's OHP finding, R4-3). The rung
                    // above asks fewer reps and leaves success achievable. On an
                    // eased day (low readiness, or holding after a miss) the
                    // lighter rung is the point, so the nearest one stands.
                    let ideal = load_for(e, range.high as f64, reserve);
                    let nearest = inv.snap(ideal);
                    let target = if advance && nearest + 1e-9 < ideal {
                        inv.next_above(nearest)
                    } else {
                        nearest
                    };
                    // Missed it twice running → the estimate is too heavy, not the
                    // day. Take a rung off and rebuild from there.
                    let load = if back_off {
                        inv.next_below(target)
                    } else {
                        target
                    };
                    // At that (discrete) weight, how many reps are actually in
                    // reach? Capped at the range top — this is the rep target that
                    // climbs there before the weight is allowed to step. The range
                    // *floor* deliberately doesn't apply: it's a style preference,
                    // and when the lightest owned rung is heavy for the estimate,
                    // raising the ask to the floor prescribes a set the athlete
                    // has no way to finish. A probe may round the ask up; a
                    // consolidation session never asks past what the sets have
                    // shown.
                    let raw = reps_at(e, load, reserve);
                    let aim = if probe { raw.round() } else { raw.floor() };
                    let low = (aim as i32).clamp(1, range.high);
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
                    // Probe (+1), consolidate (best), or (after two misses) ask one
                    // rep fewer than the number that isn't happening. Capped at the
                    // range top only — the range floor is a style preference, and
                    // demonstrated ability outranks it. Clamping up to the floor
                    // would ask an athlete who ground out 2 for 8, and quietly undo
                    // the miss-response above (aim best−1, hauled straight back up).
                    let aim = match (probe, back_off) {
                        (_, true) => best - 1,
                        (true, false) => best + 1,
                        (false, false) => best,
                    };
                    aim.clamp(1, range.high)
                }
                None => range.low,
            };
            Dose::Bodyweight {
                reps: RepTarget { low, ..range },
            }
        }
        Loaded::Hold => {
            let base = ability.best_hold.unwrap_or(COLD_HOLD_S);
            let secs = match (probe, back_off) {
                (_, true) => base - HOLD_STEP_S,
                (true, false) => base + HOLD_STEP_S,
                (false, false) => base,
            };
            Dose::Hold {
                secs: secs.max(HOLD_STEP_S),
            }
        }
        // Double progression, in the carry's own units: hold the weight and climb
        // the clock; once the clock tops out, take the next weight up and start the
        // clock again. Same earned-then-stepped shape as a weighted lift, so a
        // carry progresses like everything else instead of drifting upward forever.
        Loaded::WeightedHold(inv) => match ability.carry {
            // Missed twice → a rung down, and the clock starts again there.
            Some(c) if back_off => Dose::WeightedHold {
                load: inv.next_below(inv.snap(c.load)),
                secs: CARRY_BASE_S,
            },
            Some(c) if c.secs >= CARRY_TOP_S && probe => Dose::WeightedHold {
                load: inv.next_above(c.load),
                secs: CARRY_BASE_S,
            },
            Some(c) => Dose::WeightedHold {
                load: inv.snap(c.load),
                secs: if probe {
                    (c.secs + HOLD_STEP_S).min(CARRY_TOP_S)
                } else {
                    c.secs
                },
            },
            // Trusted on this exercise, but never actually carried under load (its
            // sets were logged without one) — open at the lightest weight owned
            // rather than inventing a number.
            None => Dose::WeightedHold {
                load: inv.lightest(),
                secs: CARRY_BASE_S,
            },
        },
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
        // Both numbers are the measurement, so both are open: carry this, and tell
        // me how long you lasted. The opening weight comes from a stale carry when
        // there is one — never from the e1RM of some other lift.
        Loaded::WeightedHold(inv) => Measure::LoadedCarry {
            start: match stale.and_then(|a| a.carry) {
                Some(c) => inv.snap(c.load),
                None => inv.lightest(),
            },
        },
    }
}

/// Which block of the session an exercise belongs in — the classic order that
/// puts demanding, technical work while the nervous system is fresh and leaves
/// finishers for last. Lower runs earlier. Tier 1 is the warm-up block.
///
/// Compound vs isolation goes by *breadth* — how many muscle groups the movement
/// genuinely works (primaries + secondaries) — not by whether it's weighted. The
/// old weighted-first rule put a dumbbell curl before pull-ups and a triceps
/// extension before push-ups: the isolation pre-fatigues the small muscle the
/// compound needs as a link, so the compound reads artificially weak — and its
/// reps are exactly what the ability model measures.
fn tier(ex: &ExerciseInfo) -> u8 {
    let breadth = ex
        .groups
        .iter()
        .filter(|(_, r)| *r != MuscleRole::Stabilizer)
        .count();
    if ex.is_skill || ex.metric == Metric::Hold {
        2 // skill / hold work — needs a fresh CNS
    } else if ex.pattern == Pattern::Core {
        5 // core / conditioning finisher
    } else if breadth >= COMPOUND_BREADTH {
        3 // compound — leads, whatever it's loaded with
    } else {
        4 // isolation / accessory — after the compounds it would pre-fatigue
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
///
/// Also where the **variation ladder** (G7) turns: a movement the athlete has
/// topped out or plateaued on steps out of candidacy when a harder doable
/// variation exists, and the returned notes say so — the step up is coaching,
/// and coaching gets said.
fn candidates<'a>(
    input: &'a PacingInput,
    kit: &Kit,
    abilities: &HashMap<i64, Ability>,
    residuals: &HashMap<i64, Residual>,
    groups: &Groups,
    history: &[SetRec],
    now: NaiveDateTime,
) -> (Vec<Cand<'a>>, Vec<Candidate>, Vec<String>) {
    // Fresher stimulus scores higher (0..1 over ~3 weeks); never-done = max.
    let recency = |id: i64| -> f64 {
        match history
            .iter()
            .filter(|s| s.exercise_id == id)
            .map(|s| s.logged_at)
            .max()
        {
            Some(t) => ((now - t).num_hours() as f64 / 24.0).min(21.0) / 21.0,
            None => 1.0,
        }
    };

    // G7 — the variation ladder, decided up front. Two walls end a movement's
    // usefulness as a prescription: the rep range's ceiling (the ask is clamped
    // there, so "keep doing 12s" would be forever) and a plateau (a month of
    // sessions with nothing beaten — see [`Residual::plateaued`]). At High
    // confidence that's a verdict, not thin data: the rung steps aside, and its
    // next-harder doable sibling becomes a measurement need. The step is
    // announced while the sibling is still news; once it has its own estimate
    // it's simply in the rotation.
    let mut stepped_aside: std::collections::HashSet<i64> = Default::default();
    let mut ladder_targets: std::collections::HashSet<i64> = Default::default();
    let mut ladder_notes = Vec::new();
    for ex in &input.exercises {
        if ex.warmup || !kit.has_all(&ex.equipment) {
            continue;
        }
        let Some(loaded) = loadable(ex, &input.exercise_loads) else {
            continue;
        };
        if ability::confidence_of(abilities, ex.id) != Confidence::High {
            continue;
        }
        let Some(d) = ex.difficulty else {
            continue;
        };
        let topped = matches!(loaded, Loaded::Reps)
            && abilities
                .get(&ex.id)
                .and_then(|a| a.best_reps)
                .is_some_and(|b| b >= rep_range(input.mode, false).high);
        let plateaued = residuals.get(&ex.id).is_some_and(|r| r.plateaued(now));
        if !(topped || plateaued) {
            continue;
        }
        let Some(next) = harder_sibling(ex, d, input, kit) else {
            continue;
        };
        stepped_aside.insert(ex.id);
        if matches!(
            ability::confidence_of(abilities, next.id),
            Confidence::None | Confidence::Low
        ) {
            ladder_targets.insert(next.id);
            ladder_notes.push(format!(
                "{} has stopped progressing — stepping up to {}.",
                ex.name, next.name
            ));
        }
    }

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
        // The group this item is labelled with: its neediest **prime mover**.
        // Ranked by need × credit within the primaries only — the label is what
        // the card headlines, and a coach names what the movement *is*, not the
        // neediest synergist it happens to brush (dips once read "(Serratus)").
        // Falls back to any trained group when the catalog gives a movement no
        // primary, so a confirmation pick still gets a label and an explanation.
        // Ties → lower group id, so it stays deterministic.
        let rank = |a: &GroupIx, b: &GroupIx| {
            let (pa, pb) = (groups.need[*a] * credit[*a], groups.need[*b] * credit[*b]);
            pa.total_cmp(&pb).then(groups.id[b.0].cmp(&groups.id[a.0]))
        };
        let label = ex
            .groups
            .iter()
            .filter(|(_, r)| *r == MuscleRole::Primary)
            .filter_map(|(gid, _)| groups.ix.get(gid).copied())
            .max_by(rank)
            .or_else(|| {
                ex.groups
                    .iter()
                    .filter_map(|(gid, _)| groups.ix.get(gid).copied())
                    .max_by(rank)
            });

        // A calibration set is a measurement: exactly one, always. Trusted work
        // takes its minimum effective dose, and may earn up to the ceiling.
        let (min, cap) = match Known::of(abilities, residuals, ex.id) {
            Some(_) => (MIN_WORK_SETS, MAX_SETS_PER_EXERCISE),
            None => (1, 1),
        };
        let confidence = ability::confidence_of(abilities, ex.id);
        let sessions = abilities.get(&ex.id).map_or(0, |a| a.sessions_recent);
        // An outgrown ladder rung isn't selectable — its successor is (below).
        if stepped_aside.contains(&ex.id) {
            continue;
        }
        // Confirmation waits on recovery: don't ask someone to repeat a movement
        // whose prime movers are still fried just to firm up its estimate — the
        // coverage gate already refuses to re-train a fried group for volume, and
        // confirmation has to respect the same physiology. Scale by the
        // least-recovered primary group (the limiting muscle); a movement with no
        // known primary group isn't recovery-gated.
        let prime_recovery = ex
            .groups
            .iter()
            .filter(|(_, r)| *r == MuscleRole::Primary)
            .filter_map(|(gid, _)| groups.ix.get(gid).copied())
            .map(|i| groups.recovery[i])
            .fold(f64::INFINITY, f64::min);
        let prime_recovery = if prime_recovery.is_finite() {
            prime_recovery
        } else {
            1.0
        };
        let confirm = confirm_need(confidence, sessions) * prime_recovery;
        // The next rung of a ladder the athlete has outgrown: measuring it is a
        // need of its own (the same reasoning as confirmation), so it qualifies
        // even when the group's volume is already covered — gated by the same
        // recovery physiology.
        let confirm = if ladder_targets.contains(&ex.id) {
            confirm.max(LADDER_CONFIRM * prime_recovery)
        } else {
            confirm
        };
        let novel = matches!(confidence, Confidence::None);
        // The freshness term rewards variety — but a movement you're *confirming* is
        // one you deliberately want to repeat, and having just done it must not drag
        // its rank down below the never-done movements it should be beating. While a
        // baseline is still firming up, treat the movement as wanted (like a new
        // one), not as "already done recently".
        let novelty = if confirm > 0.0 { 1.0 } else { recency(ex.id) };
        scored.push(Candidate {
            id: ex.id,
            family: ex.family.clone(),
            credit,
            weight: mode_fit(input.mode, ex) * 2.0 + novelty,
            confirm,
            novel,
            min,
            cap,
        });
        cands.push(Cand { ex, loaded, label });
    }

    (cands, scored, ladder_notes)
}

/// The next rung up the variation ladder from `ex` (difficulty `d`): the
/// easiest *harder* doable variation sharing its pattern and a primary muscle
/// group. The nearest rung, not the top — outgrowing incline push-ups earns
/// push-ups, not planche. Ties break to the lower id, so it stays deterministic.
fn harder_sibling<'a>(
    ex: &ExerciseInfo,
    d: i32,
    input: &'a PacingInput,
    kit: &Kit,
) -> Option<&'a ExerciseInfo> {
    input
        .exercises
        .iter()
        .filter(|y| {
            y.id != ex.id
                && !y.warmup
                && y.pattern == ex.pattern
                && y.difficulty.is_some_and(|yd| yd > d)
                && y.groups.iter().any(|(g, r)| {
                    *r == MuscleRole::Primary
                        && ex
                            .groups
                            .iter()
                            .any(|(xg, xr)| *xr == MuscleRole::Primary && xg == g)
                })
                && kit.has_all(&y.equipment)
                && loadable(y, &input.exercise_loads).is_some()
        })
        .min_by_key(|y| (y.difficulty, y.id))
}

/// The exercise the athlete *would* be doing for this group if the kit allowed:
/// the best-scoring one that trains it as a primary **and is actually blocked
/// here** — the equipment is absent, or it's present but has no registered weights.
///
/// The blocked-ness is the whole point. This used to return the best-scoring
/// exercise full stop, so any time the cover preferred a different movement — the
/// normal case, since the cover optimises marginal coverage and this only looks at
/// one group — the card announced a swap and blamed missing kit that was standing
/// right there in the room. A substitution notice must name a real obstacle, or it
/// teaches the athlete to distrust the ones that are real.
fn blocked_ideal(
    input: &PacingInput,
    kit: &Kit,
    weight: &dyn Fn(&ExerciseInfo) -> f64,
    group_id: i64,
    chosen_id: i64,
) -> Option<Substitution> {
    let name_of = |ids: &[i64]| -> Vec<String> {
        ids.iter()
            .filter_map(|id| input.equipment_names.get(id).cloned())
            .collect()
    };
    // Exactly the two ways `candidates` refuses a movement — kept in the same shape
    // so "blocked" here can't drift from "not selectable" there.
    let blocker = |e: &ExerciseInfo| -> Option<Blocker> {
        let absent: Vec<i64> = e
            .equipment
            .iter()
            .copied()
            .filter(|id| !kit.0.contains(id))
            .collect();
        if !absent.is_empty() {
            return Some(Blocker::Absent(name_of(&absent)));
        }
        // Here, but nothing to load it with.
        if loadable(e, &input.exercise_loads).is_none() {
            return Some(Blocker::Unweighted(name_of(&e.equipment)));
        }
        None
    };

    // The best movement for this group, kit or no kit. If *it* is what we're doing,
    // or it's doable and the cover simply preferred another, there is no swap to
    // report — only a top choice we can't do is a substitution.
    let ideal = input
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
        })?;
    if ideal.id == chosen_id {
        return None;
    }
    Some(Substitution {
        ideal: ideal.name.clone(),
        blocker: blocker(ideal)?,
    })
}

/// Build the warm-up block for a work plan: mobility prep for the muscle groups
/// the session actually loads, plus a light ramp-in set on the first heavy lift.
/// Warm-ups credit no volume and are the only place warm-up-tagged moves appear.
/// Ordered first.
///
/// Coverage follows the committed plan's **load**: every group the work hits at
/// primary or secondary credit, summed over its sets, ranked heaviest first. One
/// drill per group, each labelled with the group it was picked for — the old
/// primary-labels-only, sorted-by-exercise-id version spent two of three slots
/// "loosening up Obliques" while dips, pull-ups and push-ups went in with cold
/// shoulders (round-2 field test, R2-3).
///
/// Also returns the loaded groups it has *no* mobility move for. The catalog is
/// only as good as what's been authored into it, and a group with no drill produces
/// an empty warm-up that reads exactly like "you don't need one" — so the caller
/// says which groups the athlete is on their own for.
fn build_warmup(
    work: &[Suggestion],
    input: &PacingInput,
    kit: &Kit,
    ex_by_id: &HashMap<i64, &ExerciseInfo>,
    group_name: &HashMap<i64, String>,
) -> (Vec<Suggestion>, Vec<String>) {
    if work.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // Effective sets each group carries in the committed session — the same
    // credit arithmetic the volume model uses, so "loaded enough to warm up"
    // and "loaded enough to count" can't drift apart.
    let mut load: HashMap<i64, f64> = HashMap::new();
    for w in work {
        if let Some(ex) = ex_by_id.get(&w.exercise_id) {
            for (g, r) in &ex.groups {
                if *r != MuscleRole::Stabilizer {
                    *load.entry(*g).or_default() += w.sets as f64 * role_credit(*r);
                }
            }
        }
    }
    // Heaviest-loaded first; ties by group id, so the order is deterministic.
    let mut want: Vec<(i64, f64)> = load
        .into_iter()
        .filter(|(_, l)| *l >= WARMUP_MIN_LOAD)
        .collect();
    want.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

    let drills: Vec<&ExerciseInfo> = input
        .exercises
        .iter()
        .filter(|e| e.warmup && kit.has_all(&e.equipment))
        .collect();
    let primaries = |e: &ExerciseInfo| -> Vec<i64> {
        e.groups
            .iter()
            .filter(|(_, r)| *r == MuscleRole::Primary)
            .map(|(g, _)| *g)
            .collect()
    };

    // The block is sized to the session: one drill per few committed sets,
    // bounded. Groups past the cap are triage, not gaps — the coach chose the
    // heaviest, it isn't missing a drill — so they get no "warm up your own
    // way" note either.
    let work_sets: i32 = work.iter().map(|w| w.sets).sum();
    let drill_cap = ((work_sets + WARMUP_SETS_PER_DRILL - 1) / WARMUP_SETS_PER_DRILL)
        .clamp(WARMUP_MIN_DRILLS, WARMUP_MAX_DRILLS);

    let mut covered: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut out: Vec<Suggestion> = Vec::new();
    let mut gaps: Vec<String> = Vec::new();
    for (g, _) in &want {
        if out.len() as i32 >= drill_cap {
            break;
        }
        if covered.contains(g) {
            continue;
        }
        // The drill for this group: warms it as a primary, and of those, the one
        // whose primaries cover the most still-wanted groups (fewer total drills);
        // ties by exercise id.
        let pick = drills
            .iter()
            .filter(|e| primaries(e).contains(g))
            .max_by_key(|e| {
                let cover = primaries(e)
                    .iter()
                    .filter(|p| want.iter().any(|(w, _)| w == *p) && !covered.contains(*p))
                    .count();
                (cover, std::cmp::Reverse(e.id))
            });
        let Some(e) = pick else {
            gaps.push(group_name.get(g).cloned().unwrap_or_default());
            continue;
        };
        for p in primaries(e) {
            covered.insert(p);
        }
        // The drill's dose, in its own metric — reps to move through, or
        // seconds to hold. Always unloaded: this is prep, not training.
        let (rep_low, rep_high, hold_s) = match e.metric {
            Metric::Reps | Metric::WeightedReps => (Some(WARMUP_REPS), Some(WARMUP_REPS), None),
            Metric::Hold | Metric::WeightedHold => (None, None, Some(WARMUP_HOLD_S)),
        };
        out.push(Suggestion {
            exercise_id: e.id,
            exercise_name: e.name.clone(),
            pattern: e.pattern,
            kind: SuggestionKind::Warmup,
            sets: WARMUP_SETS,
            done: 0,
            rep_low,
            rep_high,
            load_kg: None,
            hold_s,
            // The group this slot is *for* — never a second card for one group.
            group: group_name.get(g).cloned().unwrap_or_default(),
            substituted_for: None,
            explanation: None,
        });
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
            done: 0,
            rep_low: w.rep_high, // an easy set of the top of the range
            rep_high: w.rep_high,
            load_kg: Some(inv.snap(load * RAMP_FRACTION)),
            hold_s: None,
            group: w.group.clone(),
            substituted_for: None,
            explanation: None,
        });
    }

    // `gaps` (collected above) are loaded groups with no drill in the catalog.
    // Named, not silently skipped: a group with no drill produces no card, which
    // reads exactly like "you don't need one" — the athlete needs to know it's a
    // hole in the catalog, not a judgement.
    gaps.sort();
    (out, gaps)
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

    let ex_by_id: HashMap<i64, &ExerciseInfo> = input.exercises.iter().map(|e| (e.id, e)).collect();

    // The set just logged, for rest guidance. A mobility drill starts no rest
    // clock at all — "Rest a moment" straight after arm circles teaches the
    // athlete to ignore the banner. For anything else, the movement's breadth
    // decides how long the rest it earned is (a hard compound set needs minutes;
    // an isolation is ready again in ninety seconds).
    let last_ex = input
        .history
        .iter()
        .max_by_key(|s| s.logged_at)
        .and_then(|s| ex_by_id.get(&s.exercise_id).copied());
    let breadth = |e: &ExerciseInfo| {
        e.groups
            .iter()
            .filter(|(_, r)| *r != MuscleRole::Stabilizer)
            .count()
    };
    let rest_hint = match last_ex {
        Some(e) if breadth(e) >= COMPOUND_BREADTH => "2–3 min",
        _ => "90 s",
    };
    let spacing_ok = last_ex.is_some_and(|e| e.warmup)
        || minutes_since_last_set.is_none_or(|m| m >= s.min_rest_min as i64);

    // --- the session in progress, if one is ---
    //
    // A session is the maximal run of sets separated by no more than
    // SESSION_GAP_MIN, ending at the most recent set — and we're *in* it if that
    // set is no further back than the gap. Everything that shapes the plan is
    // then computed as of the session's first set, so re-evaluating mid-session
    // reproduces the committed plan exactly instead of re-litigating it against
    // sets logged minutes ago. Without this, a calibration was re-prescribed
    // above the max it had just measured, rep targets ratcheted set-over-set,
    // and half-done movements vanished because their muscles read "recovering".
    let session_start: Option<NaiveDateTime> = {
        let mut times: Vec<NaiveDateTime> = input.history.iter().map(|s| s.logged_at).collect();
        times.sort_unstable();
        match times.last() {
            Some(&last) if (now - last).num_minutes() <= SESSION_GAP_MIN => {
                let mut start = last;
                for &t in times.iter().rev().skip(1) {
                    if (start - t).num_minutes() <= SESSION_GAP_MIN {
                        start = t;
                    } else {
                        break;
                    }
                }
                Some(start)
            }
            _ => None,
        }
    };
    let in_session = session_start.is_some();
    // The instant the plan is evaluated as of, and the history it may see: the
    // session's own sets are progress against the plan, not evidence to replan on.
    let plan_at = session_start.unwrap_or(now);
    let planning: Vec<SetRec> = match session_start {
        Some(cut) => input
            .history
            .iter()
            .filter(|s| s.logged_at < cut)
            .cloned()
            .collect(),
        None => input.history.clone(),
    };

    // Per-exercise ability (RPE-aware e1RM / best reps / best hold, decayed for
    // staleness) — the basis every prescription derives from. Computed once.
    let abilities = ability::abilities(&planning, plan_at);
    // How well those estimates have been describing him lately: for each session, what
    // the engine believed beforehand versus what he actually did. Recomputed from
    // history, so the engine stays stateless.
    let residuals = residual::residuals(&planning);

    // --- credit volume into rolling / 8-week-avg / recovery windows ---
    //
    // Two views of the same history. The *frozen* aggregates (windowed at
    // `plan_at`, seeing only `planning`) shape the plan: need, recovery, the day
    // target. The *live* aggregates (windowed at `now`, seeing everything) are
    // what the athlete is owed as feedback: the balance view and the session's
    // progress count. Outside a session the two coincide exactly.
    let roll_cut = plan_at - Duration::days(ROLLING_DAYS);
    let hist_cut = plan_at - Duration::days(HISTORY_WEEKS * 7);
    let live_roll_cut = now - Duration::days(ROLLING_DAYS);
    let today = now.date();
    // Region per group, for the graded recovery horizon.
    let region_of: HashMap<i64, Region> = input.groups.iter().map(|g| (g.id, g.region)).collect();

    let mut current: HashMap<i64, f64> = HashMap::new();
    let mut avg_sum: HashMap<i64, f64> = HashMap::new();
    // Age-weighted unrecovered load per group: a set counts fully when fresh and
    // ramps to zero over its region's recovery horizon (G6). This grades the old
    // binary "≥3 sets in 36 h" gate.
    let mut unrecovered: HashMap<i64, f64> = HashMap::new();
    let mut live_current: HashMap<i64, f64> = HashMap::new();
    let mut live_unrecovered: HashMap<i64, f64> = HashMap::new();
    let mut done_today = 0i32;
    let mut raw_hist = 0i32;
    let mut first_hist: Option<NaiveDateTime> = None;
    // The weeks *before* the rolling window — the only thing a "this week is a
    // spike" claim can be measured against.
    let mut baseline_sum = 0.0f64;
    let mut baseline_first: Option<NaiveDateTime> = None;
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
        let live_age_h = (now - set.logged_at).num_minutes().max(0) as f64 / 60.0;
        for (g, role) in &ex.groups {
            let credit = role_credit(*role);
            if set.logged_at >= live_roll_cut {
                *live_current.entry(*g).or_default() += credit;
            }
            let horizon = region_of.get(g).copied().map_or(48.0, recovery_horizon);
            if live_age_h < horizon {
                *live_unrecovered.entry(*g).or_default() += credit * (1.0 - live_age_h / horizon);
            }
        }
        // Everything below shapes the plan, so it sees only what was true when
        // the session was committed.
        if set.logged_at >= plan_at {
            continue;
        }
        if set.logged_at >= hist_cut {
            raw_hist += 1;
            first_hist = Some(first_hist.map_or(set.logged_at, |f| f.min(set.logged_at)));
        }
        let age_h = (plan_at - set.logged_at).num_minutes().max(0) as f64 / 60.0;
        for (g, role) in &ex.groups {
            let credit = role_credit(*role);
            if set.logged_at >= hist_cut {
                *avg_sum.entry(*g).or_default() += credit;
            }
            if set.logged_at >= roll_cut {
                *current.entry(*g).or_default() += credit;
            } else if set.logged_at >= hist_cut {
                baseline_sum += credit;
                baseline_first =
                    Some(baseline_first.map_or(set.logged_at, |f| f.min(set.logged_at)));
            }
            // Unrecovered contribution: full when fresh, linearly gone by the
            // region's horizon (a set past it no longer holds the group back).
            let horizon = region_of.get(g).copied().map_or(48.0, recovery_horizon);
            if age_h < horizon {
                *unrecovered.entry(*g).or_default() += credit * (1.0 - age_h / horizon);
            }
        }
    }

    // How much history there actually *is*, in weeks — not the width of the window
    // we looked through.
    //
    // Every weekly average below used to divide by a flat `HISTORY_WEEKS`, which is
    // only your weekly rate if you have been training the whole eight weeks. On a
    // returning athlete it is nonsense in the most damaging direction: one session
    // of 14 sets read as 1.75 sets/week, so the day's target *fell* from the
    // cold-start 6 to the floor of 3. Logging a session made the coach believe he
    // trained less than logging nothing did — the estimate got worse the more it
    // knew, which is the one thing an estimator must never do.
    //
    // Not floored at a week: a single day of history is *zero* weeks of evidence,
    // and the session-size prior below is what keeps that from extrapolating a hard
    // morning into "98 sets a week". Flooring it at one week instead would treat
    // today as a whole week already spent, which understates the rate — and that
    // was still enough to shrink the target the moment a first session landed.
    //
    // What this buys, exactly: logging a set can only ever *raise* the numerator,
    // while the denominator moves only with the calendar. So logging never lowers
    // the day's target. Time passing still can, which is correct — that's
    // detraining, not a measurement artefact.
    let observed_weeks = first_hist
        .map(|first| ((plan_at - first).num_days() as f64 / 7.0).clamp(0.0, HISTORY_WEEKS as f64))
        .unwrap_or(0.0);
    // The per-group averages below have their own anchor (blended 50/50 with the
    // literature default), so they need no prior of their own — only a guard against
    // dividing a week's work by a fraction of a week.
    let avg_weeks = observed_weeks.max(1.0);

    // --- one recovery factor on the per-group target ---
    // Biometric readiness (when health has data) is primary and supersedes the
    // crude volume-spike proxy; without it we fall back to that proxy.
    //
    // The proxy compares this week against the weeks *before* it. It used to compare
    // it against an average that divided all the history by a flat eight weeks —
    // which, for anyone with less than eight weeks of it, is mostly dividing by
    // empty weeks. Every week of a new athlete's training therefore cleared the
    // spike ratio, and the coach would have told him to ease off in every session
    // for his first two months. A spike needs something to be a spike *against*: no
    // prior weeks, no claim (and biometric readiness, which supersedes this proxy
    // whenever health has data, carries the cold start).
    let baseline_weeks = baseline_first
        .map(|first| {
            ((roll_cut - first).num_days() as f64 / 7.0)
                .clamp(1.0, (HISTORY_WEEKS - ROLLING_DAYS / 7) as f64)
        })
        .unwrap_or(0.0);
    let baseline_weekly = if baseline_weeks > 0.0 {
        baseline_sum / baseline_weeks
    } else {
        0.0
    };
    let last7_total: f64 = current.values().sum();
    let volume_deload = baseline_weekly > 0.0 && last7_total > DELOAD_RATIO * baseline_weekly;
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
        let avg = avg_sum.get(&gm.id).copied().unwrap_or(0.0) / avg_weeks;
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

        // The balance view is feedback, not planning: it counts the session's
        // sets the moment they land (outside a session it equals the frozen
        // numbers exactly).
        let live_cur = *live_current.get(&gm.id).unwrap_or(&0.0);
        let live_rec = (1.0 - live_unrecovered.get(&gm.id).copied().unwrap_or(0.0) / RECOVERY_SETS)
            .clamp(0.0, 1.0);
        balances.push(GroupBalance {
            group: gm.name.clone(),
            region: gm.region,
            current: live_cur,
            target,
            deficit: ((target - live_cur) / target).clamp(0.0, 1.0),
            recovering: live_rec < RECOVERED_FRACTION,
        });
    }
    // Balance view: most-in-deficit first.
    balances.sort_by(|a, b| b.deficit.total_cmp(&a.deficit));

    // --- session-size target from personal weekly volume (sizes the plan) ---
    //
    // A shrinkage estimate, not a switch: the weekly rate starts *as* the anchor and
    // each observed week pulls it toward what he actually does. The old form was a
    // cliff — a hard-coded 24 with no history, your (mis-computed) average the
    // moment you had any — so the first logged set could drop the target by half.
    // With no history this is exactly the anchor, so the cold start is unchanged.
    let avg_weekly_sets =
        (raw_hist as f64 + ANCHOR_WEEKLY_SETS * ANCHOR_WEEKS) / (observed_weeks + ANCHOR_WEEKS);
    // Scale the day's set count by the same recovery factor as the group targets,
    // so a low-readiness day is fewer sets, not just lighter ones.
    let day_target_sets = ((avg_weekly_sets / input.days_per_week.max(1) as f64 * recovery_scale)
        .round() as i32)
        .clamp(3, 15);

    // Novel movements already introduced today spend their novelty slots for the
    // whole day, not just their own session — otherwise finishing a morning's
    // calibrations would simply let the evening backfill three more. Counted over
    // `planning`: within a session the committed picks *are* the introductions,
    // so only earlier sessions bind.
    let novel_introduced = {
        let mut first_seen: HashMap<i64, NaiveDateTime> = HashMap::new();
        for s in &planning {
            if ex_by_id.get(&s.exercise_id).is_none_or(|e| e.warmup) {
                continue;
            }
            first_seen
                .entry(s.exercise_id)
                .and_modify(|t| *t = (*t).min(s.logged_at))
                .or_insert(s.logged_at);
        }
        first_seen.values().filter(|t| t.date() == today).count() as i32
    };
    let novelty_cap = (NOVELTY_CAP - novel_introduced).max(0);

    // --- cover the need with the kit that's actually here ---
    // No location → we don't know what's doable, and we don't guess: no plan.
    let (mut plan, warmup_gaps, ladder_notes) = match &input.kit {
        Some(kit) => plan_session(
            input,
            kit,
            &abilities,
            &residuals,
            &groups,
            day_target_sets,
            novelty_cap,
            &ex_by_id,
            &planning,
            plan_at,
        ),
        None => (Vec::new(), Vec::new(), Vec::new()),
    };

    // Progress against the committed plan: the session's sets pay its items in
    // plan order (a ramp-in warm-up shares its exercise with the work item that
    // follows, so order is what attributes them).
    if let Some(start) = session_start {
        let mut session_sets: HashMap<i64, i32> = HashMap::new();
        for s in &input.history {
            if s.logged_at >= start {
                *session_sets.entry(s.exercise_id).or_default() += 1;
            }
        }
        for item in &mut plan {
            let rem = session_sets.entry(item.exercise_id).or_default();
            item.done = (*rem).min(item.sets);
            *rem -= item.done;
        }
    }
    // Kit the coach had to leave out — worked out by the service, which knows why.
    // Only worth saying when there's a session for it to be a hole in.
    let mut notices = if plan.is_empty() {
        Vec::new()
    } else {
        input.notices.clone()
    };
    if !warmup_gaps.is_empty() {
        notices.push(format!(
            "I don't know a warm-up for {} — warm those up your own way.",
            warmup_gaps.join(", ")
        ));
    }
    // The variation ladder's step-ups (G7) — coaching, so it gets said. Only
    // alongside a session, same as every other notice: on a rest day there is
    // no plan for the step to be part of.
    if !plan.is_empty() {
        notices.extend(ladder_notes);
    }

    // "Next up" for the nudge + Android trigger is the first *unfinished*
    // training item, not the warm-up that leads the visible plan and not
    // something already done this session.
    let suggestion = plan
        .iter()
        .find(|s| s.kind != SuggestionKind::Warmup && s.done < s.sets)
        .cloned();
    // What the athlete should literally do next — warm-ups very much included.
    // The banner speaks from this, so it can never disagree with the plan's
    // "Next up" pill (which points at the same item): the round-2 test caught
    // the banner saying "Next up: 2 × Dips" while the pill sat on a mobility
    // drill.
    let next_item = plan.iter().find(|s| s.done < s.sets).cloned();
    // One phrasing for "do this next", kind-aware: a warm-up is named with its
    // dose, work with its remaining sets and muscle group.
    let next_phrase = |s: &Suggestion| -> String {
        if s.kind == SuggestionKind::Warmup {
            let dose = match (s.rep_low, s.hold_s, s.load_kg) {
                (Some(r), _, Some(kg)) => format!("{r} × {kg} kg ramp-in"),
                (Some(r), _, None) => format!("{r} slow reps"),
                (None, Some(secs), _) => format!("{secs}s"),
                _ => "easy prep".to_string(),
            };
            format!("{} — {}", s.exercise_name, dose)
        } else {
            format!("{} × {} ({})", s.sets - s.done, s.exercise_name, s.group)
        }
    };

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
    // Tone matters as much as content here. This is the one sentence the coach
    // speaks, and it's read by someone returning to training — so it affirms
    // readiness and invites the work, and never urges *intensity* ("push", "go
    // hard"). The athlete decides how hard; the coach's job is to say what to do and
    // that today's a good day for it.
    let day_note = match input.readiness.map(|r| r.band) {
        Some(Band::High) => Some("Recovered — a good day to train well."),
        Some(Band::Low) => Some("Low readiness — keeping it light."),
        None if deload => Some("Volume's run hot lately — easing off."),
        _ => None,
    };

    let reason = if input.kit.is_none() {
        "Tell me where you're training and I'll plan the session.".to_string()
    } else if suggestion.is_none() {
        if in_session && done_today > 0 {
            // Every committed item is done — close the session, don't gloss it
            // as a rest day.
            "That's the session — nice work.".to_string()
        } else if state == PacingState::Rest {
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
        // Less than the rest interval since the last set — that's *between sets*,
        // not between sessions (a fresh set always puts us inside the session
        // window). Name the rest's length and what's next instead of waving the
        // athlete off; the old "just trained — take a breather" line fired after
        // every single set, and "a moment" told them nothing a coach would.
        match next_item.as_ref().or(suggestion.as_ref()) {
            Some(next) => format!("Rest {rest_hint} — then: {}.", next_phrase(next)),
            None => String::new(),
        }
    } else if suggestion.is_some()
        && let Some(next) = &next_item
    {
        // Always an invitation to the next movement — never "you're a bit light this
        // week", which frames a returning athlete as behind a quota and pressures the
        // volume up. Whether he's ahead of or behind the day's burn-down still drives
        // the *nudge* (a reminder's timing); it must not colour what the coach says.
        // A warm-up that hasn't been done leads: prep first, and the banner and the
        // plan's pill name the same thing.
        if next.kind == SuggestionKind::Warmup {
            format!("Warm up first: {}.", next_phrase(next))
        } else {
            format!("Next up: {}.", next_phrase(next))
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
#[allow(clippy::too_many_arguments)]
fn plan_session(
    input: &PacingInput,
    kit: &Kit,
    abilities: &HashMap<i64, Ability>,
    residuals: &HashMap<i64, Residual>,
    groups: &Groups,
    budget: i32,
    novelty_cap: i32,
    ex_by_id: &HashMap<i64, &ExerciseInfo>,
    history: &[SetRec],
    now: NaiveDateTime,
) -> (Vec<Suggestion>, Vec<String>, Vec<String>) {
    let (cands, scored, ladder_notes) =
        candidates(input, kit, abilities, residuals, groups, history, now);
    let chosen = cover::select(&scored, &groups.need, budget, novelty_cap);

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
        let feedback = residuals.get(&c.ex.id).cloned().unwrap_or_default();
        let (kind, dose, measure) = match Known::of(abilities, residuals, c.ex.id) {
            Some(known) => (
                SuggestionKind::Work,
                Some(prescribe(&c.loaded, &known, input.mode, advance, &feedback)),
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
            (Some(Dose::WeightedHold { load, secs }), _) => (None, None, Some(*load), Some(*secs)),
            (_, Some(Measure::BuildUp { start, reps })) => {
                (Some(*reps), Some(*reps), Some(*start), None)
            }
            // The weight is given; the duration is the open field, because it is
            // what we're measuring.
            (_, Some(Measure::LoadedCarry { start })) => (None, None, Some(*start), None),
            // AMRAP / max hold — the open fields say "as many clean reps / as long
            // as clean form holds", which is exactly what we're measuring.
            (_, Some(Measure::Amrap) | Some(Measure::MaxHold)) => (None, None, None, None),
            (None, None) => (None, None, None, None),
        };

        let (group, explanation, substituted_for) = match c.label {
            Some(ix) => {
                // A swap note only makes sense when this pick actually *trains*
                // the label group as a prime mover. Labels can fall to a
                // secondary group (its primaries covered), and "Triceps
                // extension — swapped in for Good morning" via a shared
                // erector-spinae assist is nonsense the athlete rightly
                // distrusts.
                let label_is_primary =
                    c.ex.groups
                        .iter()
                        .any(|(g, r)| *r == MuscleRole::Primary && *g == groups.id[ix.0]);
                (
                    groups.name[ix.0].clone(),
                    Some(Explanation {
                        deficit: groups.deficit[ix],
                        recovery: groups.recovery[ix],
                        pays: pick.pays,
                        confirming: pick.confirming,
                        confidence: ability::confidence_of(abilities, c.ex.id),
                        e1rm: ability.and_then(|a| a.e1rm),
                        misses: feedback.consecutive_misses,
                        readiness: input.readiness.map(|r| r.band),
                    }),
                    (label_is_primary && stood_in.insert(ix))
                        .then(|| blocked_ideal(input, kit, &weight, groups.id[ix.0], c.ex.id))
                        .flatten(),
                )
            }
            None => (String::new(), None, None),
        };

        work.push((
            Suggestion {
                exercise_id: c.ex.id,
                exercise_name: c.ex.name.clone(),
                pattern: c.ex.pattern,
                kind,
                sets,
                done: 0,
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
    let (warmup, gaps) = build_warmup(&work, input, kit, ex_by_id, &group_name);
    (warmup.into_iter().chain(work).collect(), gaps, ladder_notes)
}
