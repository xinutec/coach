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

// ---- inputs (internal; not wire types) -------------------------------------

#[derive(Clone)]
pub struct ExerciseInfo {
    pub id: i64,
    pub name: String,
    pub pattern: Pattern,
    pub metric: Metric,
    /// Ring/parallette or hold work — biased in Skills mode.
    pub is_skill: bool,
    /// Equipment ids required (empty = bodyweight).
    pub equipment: Vec<i64>,
    /// Muscle groups this exercise trains, with the strongest role for each.
    pub groups: Vec<(i64, MuscleRole)>,
}

/// A logged set in the trailing history window (rich enough for volume,
/// progression, and the ability estimate). `rpe` (when logged) makes the e1RM
/// estimate effort-aware — a set left with reps in reserve implies more strength
/// than a grinding one at the same load.
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

pub struct PacingSettings {
    pub window_start_hour: i32,
    pub window_end_hour: i32,
    pub min_rest_min: i32,
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
    /// Equipment available at the selected location. `None` = no filter; `Some` =
    /// the suggestion must be doable here (a location-blocked ideal is swapped out).
    pub available_equipment: Option<std::collections::HashSet<i64>>,
    /// Discrete owned weights (kg, sorted asc) per equipment id at the selected
    /// location — the engine snaps load suggestions to these. Empty for an
    /// equipment (or no location) → classic +2.5 kg progression, no snapping.
    pub equipment_loads: HashMap<i64, Vec<f64>>,
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
    /// A prescription derived from a trusted ability estimate.
    Work,
    /// A calibration set — the engine is measuring what you can do.
    Assess,
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
}

/// The full coach verdict for an instant. Drives the Today UI and the Android
/// nudge (fired only when `nudge` AND the phone's geofence says you're home).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PacingNow {
    pub state: PacingState,
    pub mode: Mode,
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
    pub suggestion: Option<Suggestion>,
}
