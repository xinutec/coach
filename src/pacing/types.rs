//! Dynamic-coach engine input (plain data, assembled from repos) and output
//! (wire types). The engine [`super::engine::evaluate`] is a pure function over
//! these: it computes what to do now from **history + the active mode**, with no
//! program. Rolling muscle-group volume + recovery + progression, location-aware.

use std::collections::HashMap;

use chrono::NaiveDateTime;
use serde::Serialize;
use ts_rs::TS;

use crate::exercise::types::{Metric, Pattern};
use crate::muscle::types::{MuscleRole, Region};
use crate::settings::types::Mode;

use super::ability::Confidence;

// ---- inputs (internal; not wire types) -------------------------------------

#[derive(Clone)]
pub struct ExerciseInfo {
    pub id: i64,
    pub name: String,
    pub pattern: Pattern,
    pub metric: Metric,
    /// Ring/parallette or hold work — biased in Skills mode.
    pub is_skill: bool,
    /// A mobility/activation move: only the warm-up block picks it, and it
    /// credits no training volume.
    pub warmup: bool,
    /// Equipment ids required (empty = bodyweight).
    pub equipment: Vec<i64>,
    /// Muscle groups this exercise trains, with the strongest role for each.
    pub groups: Vec<(i64, MuscleRole)>,
}

/// A logged set in the trailing history window (rich enough for volume,
/// progression, and the ability estimate). `rpe` (when logged) makes the e1RM
/// estimate effort-aware — a set left with reps in reserve implies more strength
/// than a grinding one at the same load.
#[derive(Clone)]
pub struct SetRec {
    pub exercise_id: i64,
    pub logged_at: NaiveDateTime,
    pub reps: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
    pub rpe: Option<i32>,
}

/// Muscle-group identity for output labelling + the balance view.
#[derive(Clone)]
pub struct GroupMeta {
    pub id: i64,
    pub name: String,
    pub region: Region,
}

#[derive(Clone, Copy)]
pub struct PacingSettings {
    pub window_start_hour: i32,
    pub window_end_hour: i32,
    pub min_rest_min: i32,
}

/// The equipment present where the athlete is training.
///
/// Deliberately *not* an `Option<HashSet>` consulted with `is_none_or`: that
/// spelling made "we don't know the location" mean "everything is doable", so a
/// missing location silently switched the safety filter off and the coach
/// cheerfully suggested trap-bar deadlifts in a living room. Absent kit now means
/// absent kit. Not knowing where you are is a *different* state
/// ([`PacingInput::kit`] = `None`), and it yields a narrower verdict — no
/// suggestions at all — rather than a wider one.
#[derive(Clone, Debug, Default)]
pub struct Kit(pub std::collections::HashSet<i64>);

impl Kit {
    /// Is every piece of `required` equipment present here? (Empty = bodyweight,
    /// always true.)
    pub fn has_all(&self, required: &[i64]) -> bool {
        required.iter().all(|e| self.0.contains(e))
    }
}

/// Everything the engine needs, already fetched.
pub struct PacingInput {
    pub mode: Mode,
    pub days_per_week: i32,
    pub emphasis: Option<Region>,
    pub exercises: Vec<ExerciseInfo>,
    /// Trailing history (≈6 months) — every set's reps/load/hold/rpe, feeding
    /// both rolling volume and the ability estimate (which decays old sets).
    pub history: Vec<SetRec>,
    pub last_set_at: Option<NaiveDateTime>,
    pub settings: PacingSettings,
    pub groups: Vec<GroupMeta>,
    /// The kit at the athlete's location. `None` = no location known, so the
    /// engine can't say what's doable and won't guess: the verdict carries no
    /// plan and asks for a location. Degradation narrows the claim, never widens it.
    pub kit: Option<Kit>,
    /// The loads each exercise can actually be built with here, keyed by exercise
    /// id — *not* by equipment. What's buildable depends on how many implements the
    /// movement needs: a pair of dumbbells splits a finite disc budget between
    /// them, and a fixed weight you own one of can't serve a two-dumbbell press.
    /// Absent or empty = not loadable here, so the lift isn't selectable (see
    /// [`super::dose::Inventory`]) and the verdict says why rather than inventing
    /// a number.
    pub exercise_loads: HashMap<i64, Vec<f64>>,
    /// Kit the coach had to leave out, and why — surfaced on the verdict so a drop
    /// reads as something to fix rather than a hole in the plan.
    pub notices: Vec<String>,
    /// Biometric readiness (from health), if available. `None` → the engine falls
    /// back to the volume-spike deload heuristic.
    pub readiness: Option<Readiness>,
}

/// How recovered the user is right now, from biometrics (health-derived).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Band {
    Low,
    Normal,
    High,
}

/// The readiness verdict coach computes from health's raw recovery data.
#[derive(Clone, Copy, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Readiness {
    /// 0 (unrecovered) .. 1 (fully recovered).
    pub score: f64,
    pub band: Band,
}

// ---- output (wire types) ---------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PacingState {
    /// A concrete thing to do now.
    Active,
    /// Everything due is recovered/at target — rest, or an optional light set.
    Rest,
    /// No history yet — a cold-start suggestion to get going.
    Fresh,
}

/// Rolling volume vs target for one muscle group — drives the balance view.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GroupBalance {
    pub group: String,
    pub region: Region,
    /// Effective sets over the trailing 7 days (primary 1.0, secondary 0.5).
    pub current: f64,
    pub target: f64,
    /// (target − current)/target, clamped 0..1.
    pub deficit: f64,
    pub recovering: bool,
}

/// Whether a suggestion is a normal prescription or a calibration task. When the
/// engine's ability estimate for the chosen exercise is untrusted (never done,
/// or only stale data), it can't prescribe honestly — so it asks you to measure:
/// the logged set *is* the assessment, and the next verdict prescribes from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SuggestionKind {
    /// A mobility/activation move or a light ramp-in set — prep, not training.
    Warmup,
    /// A prescription derived from a trusted ability estimate.
    Work,
    /// A calibration set — the engine is measuring what you can do.
    Assess,
}

/// Why the engine chose this exercise + prescription — a structured trace so the
/// UI can show its reasoning and tests can assert on it (rather than string-match
/// prose). Every number here is one the verdict already computed.
#[derive(Clone, Copy, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Explanation {
    /// How far below target this muscle group is (0 = at target, 1 = untrained).
    pub deficit: f64,
    /// Recovery fraction for the group (0 = just hammered, 1 = fully recovered).
    pub recovery: f64,
    /// Effective sets of genuine need this exercise's first set paid down — the
    /// number the cover actually ranked and gated it on (`deficit` and `recovery`
    /// are the human-readable factors behind it). An item is only planned when
    /// this clears [`super::cover::MIN_PAY`], so the trace proves the gate held.
    pub pays: f64,
    /// How much the engine trusts its ability estimate for this exercise.
    pub confidence: Confidence,
    /// Estimated 1-rep-max (kg) the load was derived from, when known.
    pub e1rm: Option<f64>,
    /// The biometric readiness band that scaled today's volume, if health had data.
    pub readiness: Option<Band>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Suggestion {
    #[ts(type = "number")]
    pub exercise_id: i64,
    pub exercise_name: String,
    pub pattern: Pattern,
    /// Work (prescribe) or Assess (measure). Drives the Today card's framing.
    pub kind: SuggestionKind,
    pub sets: i32,
    pub rep_low: Option<i32>,
    pub rep_high: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
    /// The muscle group this targets (for the reason text).
    pub group: String,
    /// When set, the ideal exercise for this group isn't doable at the current
    /// location, so an equivalent was swapped in (the ideal's name).
    pub substituted_for: Option<String>,
    /// Why this was chosen (deficit, recovery, ability, readiness). `None` for
    /// warm-up items, which are prep rather than a reasoned prescription.
    pub explanation: Option<Explanation>,
}

/// The full coach verdict for an instant. Drives the Today UI and the Android
/// nudge (fired only when `nudge` AND the phone's geofence says you're home).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PacingNow {
    pub state: PacingState,
    /// Auto-deload active — volume's been high (only the no-biometric fallback;
    /// suppressed when `readiness` is present, which supersedes it).
    pub deload: bool,
    /// Biometric readiness driving today's volume/progression, when health had data.
    pub readiness: Option<Readiness>,
    pub nudge: bool,
    pub reason: String,
    pub within_window: bool,
    /// Past the training window's end — coach defers to tomorrow.
    pub after_window: bool,
    pub spacing_ok: bool,
    #[ts(type = "number | null")]
    pub minutes_since_last_set: Option<i64>,
    /// The computed session-size target + what's been done today (drive the nudge).
    pub day_target_sets: i32,
    pub day_done_sets: i32,
    pub groups: Vec<GroupBalance>,
    /// The head of `plan` — "next up" — kept for the nudge + the Android trigger.
    pub suggestion: Option<Suggestion>,
    /// The ordered session for today: a greedy set-cover of the day's muscle-group
    /// need (see [`super::cover`]), so each exercise appears **once** with the set
    /// count it earned, ordered by training tier (skill/hold → heavy compound →
    /// accessory → core). Recomputed statelessly each call, so logging a set
    /// reshapes it live.
    pub plan: Vec<Suggestion>,
    /// Things the athlete should know that aren't a set to do — chiefly kit that
    /// can't be prescribed because its weights aren't registered here. The engine
    /// drops those exercises rather than guessing a load; saying so is what keeps
    /// the drop from looking like a silent gap in the plan.
    pub notices: Vec<String>,
}
