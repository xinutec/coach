//! The micro-log: one `WorkoutSet` row per set done "here and there".

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::exercise::types::Metric;

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct WorkoutSet {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub id: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub exercise_id: i64,
    pub logged_at: NaiveDateTime,
    pub reps: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
    pub rpe: Option<i32>,
    pub note: Option<String>,
}

/// Body for POST /api/sets. `loggedAt` defaults to now.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NewSet {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub exercise_id: i64,
    pub reps: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
    pub rpe: Option<i32>,
    pub note: Option<String>,
    pub logged_at: Option<NaiveDateTime>,
    /// Set by the client when re-sending a load it was asked to confirm — "yes,
    /// I really did lift that". Absent/false on a first attempt, so a surprising
    /// load is queried once and never again for the same set.
    pub confirm_load: Option<bool>,
}

/// Value ceilings no honest set exceeds — plausibility, not policy. Generous
/// on purpose: a real outlier day must never be refused, only a number that
/// describes nothing a human did. The round-3 field test stored a fat-fingered
/// **3 530-second** farmers walk (an "append" instead of a replace) and the
/// carry ability model would have read it as a demonstrated max.
const MAX_REPS: i32 = 100;
const MAX_HOLD_S: i32 = 600;
const MAX_LOAD_KG: f64 = 300.0;

impl NewSet {
    /// The fields a set may carry are its exercise metric's fields — nothing
    /// else — and their values must describe something a human did. A load on
    /// a bodyweight mobility drill isn't extra detail, it's a falsehood the
    /// ability model would ingest (a client once posted exactly that from a
    /// stale hidden form field); a 59-minute carry is the same lie told in
    /// seconds. Returns what's wrong, or `None` when the set is honest.
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
        if self.reps.is_some_and(|r| !(1..=MAX_REPS).contains(&r)) {
            return Some("reps must be between 1 and 100");
        }
        if self.hold_s.is_some_and(|s| !(1..=MAX_HOLD_S).contains(&s)) {
            return Some("seconds must be between 1 and 600");
        }
        if self.load_kg.is_some_and(|l| !(l > 0.0 && l <= MAX_LOAD_KG)) {
            return Some("load must be between 0 and 300 kg");
        }
        if self.rpe.is_some_and(|r| !(1..=10).contains(&r)) {
            return Some("RPE is 1-10");
        }
        None
    }
}
