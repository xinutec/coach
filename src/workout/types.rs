//! The micro-log: one `WorkoutSet` row per set done "here and there".

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, TS, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkoutSet {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub exercise_id: i64,
    pub logged_at: NaiveDateTime,
    pub reps: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
    pub rpe: Option<i32>,
    pub note: Option<String>,
}

/// The top set of an exercise's most recent session — the progression basis the
/// engine bumps off (internal, not a wire type).
#[derive(Clone, Debug)]
pub struct LastPerformance {
    pub reps: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
}

/// Body for POST /api/sets. `loggedAt` defaults to now.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewSet {
    #[ts(type = "number")]
    pub exercise_id: i64,
    pub reps: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
    pub rpe: Option<i32>,
    pub note: Option<String>,
    pub logged_at: Option<NaiveDateTime>,
}
