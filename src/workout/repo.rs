//! Workout-set queries. Soft-deletes (deleted_at) so history stays intact.
//! SQL is `&'static str` literal (sqlx 0.9 SqlSafeStr); no interpolation.

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use chrono::NaiveDateTime;
use sqlx::MySqlPool;

use super::types::{LastPerformance, NewSet, WorkoutSet};

/// Insert a logged set. `logged_at` defaults to now when the client omits it.
pub async fn create(pool: &MySqlPool, user_id: &str, n: &NewSet) -> Result<WorkoutSet> {
    let res = sqlx::query(
        // logged_at defaults to UTC (UTC_TIMESTAMP), so the pacing engine's
        // local-tz day/window math is correct regardless of server tz.
        "INSERT INTO workout_sets \
           (user_id, exercise_id, logged_at, reps, load_kg, hold_s, rpe, note) \
         VALUES (?, ?, COALESCE(?, UTC_TIMESTAMP()), ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(n.exercise_id)
    .bind(n.logged_at)
    .bind(n.reps)
    .bind(n.load_kg)
    .bind(n.hold_s)
    .bind(n.rpe)
    .bind(&n.note)
    .execute(pool)
    .await?;
    get(pool, user_id, res.last_insert_id() as i64)
        .await?
        .ok_or_else(|| anyhow!("set vanished after insert"))
}

pub async fn get(pool: &MySqlPool, user_id: &str, id: i64) -> Result<Option<WorkoutSet>> {
    Ok(sqlx::query_as::<_, WorkoutSet>(
        "SELECT id, exercise_id, logged_at, reps, load_kg, hold_s, rpe, note \
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
        "SELECT id, exercise_id, logged_at, reps, load_kg, hold_s, rpe, note \
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
        "SELECT id, exercise_id, logged_at, reps, load_kg, hold_s, rpe, note \
         FROM workout_sets WHERE user_id = ? AND deleted_at IS NULL AND logged_at >= ? \
         ORDER BY logged_at ASC",
    )
    .bind(user_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

/// exercise id → the top set of its most recent session (max reps/load/hold on the
/// last day it was trained), the basis for progression. One query for all exercises.
pub async fn last_performance_by_exercise(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<HashMap<i64, LastPerformance>> {
    // (exercise_id, reps, load_kg, hold_s)
    type Row = (i64, Option<i32>, Option<f64>, Option<i32>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT w.exercise_id, MAX(w.reps) AS reps, MAX(w.load_kg) AS load_kg, MAX(w.hold_s) AS hold_s \
         FROM workout_sets w \
         JOIN (SELECT exercise_id, MAX(DATE(logged_at)) AS d FROM workout_sets \
               WHERE user_id = ? AND deleted_at IS NULL GROUP BY exercise_id) last \
           ON last.exercise_id = w.exercise_id AND DATE(w.logged_at) = last.d \
         WHERE w.user_id = ? AND w.deleted_at IS NULL \
         GROUP BY w.exercise_id",
    )
    .bind(user_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(ex, reps, load_kg, hold_s)| {
            (
                ex,
                LastPerformance {
                    reps,
                    load_kg,
                    hold_s,
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
