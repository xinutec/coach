//! Workout-set queries. Soft-deletes (deleted_at) so history stays intact.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use chrono::NaiveDateTime;
use coach_pacing::domain::ExerciseId;
use sqlx::MySqlPool;

use crate::pacing::types::Best;

use super::types::{ValidSet, WorkoutSet};

/// Insert a logged set. `logged_at` defaults to now when the client omits it.
pub async fn create(pool: &MySqlPool, user_id: &str, n: &ValidSet) -> Result<WorkoutSet> {
    let c = n.performed.columns();
    let res = sqlx::query(
        // logged_at defaults to UTC (UTC_TIMESTAMP), so the pacing engine's
        // local-tz day/window math is correct regardless of server tz.
        "INSERT INTO workout_sets \
           (user_id, exercise_id, logged_at, reps, load_kg, hold_s, distance_m, rpe, note) \
         VALUES (?, ?, COALESCE(?, UTC_TIMESTAMP()), ?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(n.exercise_id)
    .bind(n.logged_at)
    .bind(c.reps)
    .bind(c.load_kg)
    .bind(c.hold_s)
    .bind(c.distance_m)
    .bind(n.rpe)
    .bind(&n.note)
    .execute(pool)
    .await?;
    get(pool, user_id, crate::db::inserted_id(&res)?)
        .await?
        .ok_or_else(|| anyhow!("set vanished after insert"))
}

pub async fn get(pool: &MySqlPool, user_id: &str, id: i64) -> Result<Option<WorkoutSet>> {
    Ok(sqlx::query_as::<_, WorkoutSet>(
        "SELECT id, exercise_id, logged_at, reps, load_kg, hold_s, distance_m, rpe, note \
         FROM workout_sets WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

/// Most-recent sets first, capped at `limit`.
pub async fn list_recent(pool: &MySqlPool, user_id: &str, limit: i64) -> Result<Vec<WorkoutSet>> {
    Ok(sqlx::query_as::<_, WorkoutSet>(
        "SELECT id, exercise_id, logged_at, reps, load_kg, hold_s, distance_m, rpe, note \
         FROM workout_sets WHERE user_id = ? AND deleted_at IS NULL \
         ORDER BY logged_at DESC LIMIT ?",
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// All live sets logged at or after `since`, oldest first. Feeds the pacing
/// engine's weekly/daily burn-down.
pub async fn list_since(
    pool: &MySqlPool,
    user_id: &str,
    since: NaiveDateTime,
) -> Result<Vec<WorkoutSet>> {
    Ok(sqlx::query_as::<_, WorkoutSet>(
        "SELECT id, exercise_id, logged_at, reps, load_kg, hold_s, distance_m, rpe, note \
         FROM workout_sets WHERE user_id = ? AND deleted_at IS NULL AND logged_at >= ? \
         ORDER BY logged_at ASC",
    )
    .bind(user_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

/// Each movement's best set before `before` (UTC), over the whole log: the Epley
/// estimate of loaded reps, bodyweight reps, unloaded hold seconds. Casts pin the
/// types, as an aggregate over an expression decodes as whatever the server picks.
pub async fn bests_before(
    pool: &MySqlPool,
    user_id: &str,
    before: NaiveDateTime,
) -> Result<BTreeMap<ExerciseId, Best>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        exercise_id: i64,
        e1rm: Option<f64>,
        reps: Option<i64>,
        hold_s: Option<i64>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT exercise_id, \
           CAST(MAX(CASE WHEN load_kg IS NOT NULL AND reps IS NOT NULL \
                    THEN load_kg * (1 + reps / 30) END) AS DOUBLE) AS e1rm, \
           CAST(MAX(CASE WHEN load_kg IS NULL AND hold_s IS NULL AND distance_m IS NULL \
                    THEN reps END) AS SIGNED) AS reps, \
           CAST(MAX(CASE WHEN load_kg IS NULL AND reps IS NULL THEN hold_s END) AS SIGNED) \
             AS hold_s \
         FROM workout_sets WHERE user_id = ? AND deleted_at IS NULL AND logged_at < ? \
         GROUP BY exercise_id",
    )
    .bind(user_id)
    .bind(before)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                ExerciseId(r.exercise_id),
                Best {
                    e1rm: r.e1rm,
                    reps: r.reps.and_then(|n| i32::try_from(n).ok()),
                    hold_s: r.hold_s.and_then(|n| i32::try_from(n).ok()),
                },
            )
        })
        .collect())
}

/// Soft-delete a set. Returns false if nothing matched (wrong user / already gone).
pub async fn soft_delete(pool: &MySqlPool, user_id: &str, id: i64) -> Result<bool> {
    let res = sqlx::query(
        "UPDATE workout_sets SET deleted_at = NOW() \
         WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}
