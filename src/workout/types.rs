//! The micro-log: one `WorkoutSet` row per set done "here and there".

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::exercise::types::Metric;
use coach_pacing::domain::LoggedSet;

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

/// A request body that has been checked, and so can be written.
///
/// [`repo::create`] takes this rather than a [`NewSet`], which is what makes the
/// check unskippable: there is no way to reach the INSERT holding only an
/// unvalidated body. It used to take the body itself, with the one call site in
/// the route remembering to ask first.
///
/// [`repo::create`]: super::repo::create
#[derive(Debug)]
pub struct ValidSet {
    pub exercise_id: i64,
    pub performed: LoggedSet,
    pub rpe: Option<i32>,
    pub note: Option<String>,
    pub logged_at: Option<NaiveDateTime>,
}

impl NewSet {
    /// Check the body against its exercise's metric and turn it into something
    /// writable, or say what is wrong in the athlete's terms.
    ///
    /// Consuming, so the unvalidated body is gone once this returns — there is
    /// nothing left to accidentally pass on.
    pub fn validate(self, metric: Metric) -> Result<ValidSet, &'static str> {
        // Effort, unlike the dose, is not part of the set's shape: any metric may
        // carry an RPE or none. (The coach never asks for one — see
        // docs/trainer.md — but an import may bring one.)
        if self.rpe.is_some_and(|r| !(1..=10).contains(&r)) {
            return Err("RPE is 1-10");
        }
        Ok(ValidSet {
            exercise_id: self.exercise_id,
            performed: LoggedSet::parse(metric, self.reps, self.load_kg, self.hold_s)?,
            rpe: self.rpe,
            note: self.note,
            logged_at: self.logged_at,
        })
    }
}
