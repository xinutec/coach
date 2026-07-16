//! The micro-log: one `WorkoutSet` row per set done "here and there".

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::exercise::types::Metric;

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

impl NewSet {
    /// The fields a set may carry are its exercise metric's fields — nothing
    /// else. A load on a bodyweight mobility drill isn't extra detail, it's a
    /// falsehood the ability model would ingest (a client once posted exactly
    /// that from a stale hidden form field). Returns what's wrong, or `None`
    /// when the shape is honest.
    pub fn shape_error(&self, metric: Metric) -> Option<&'static str> {
        let (load_ok, reps_ok, hold_ok) = match metric {
            Metric::Reps => (false, true, false),
            Metric::WeightedReps => (true, true, false),
            Metric::Hold => (false, false, true),
            Metric::WeightedHold => (true, false, true),
        };
        if self.load_kg.is_some() && !load_ok {
            return Some("this exercise takes no load");
        }
        if self.reps.is_some() && !reps_ok {
            return Some("this exercise is measured in seconds, not reps");
        }
        if self.hold_s.is_some() && !hold_ok {
            return Some("this exercise is measured in reps, not seconds");
        }
        None
    }
}
