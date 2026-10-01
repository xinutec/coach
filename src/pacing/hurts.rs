//! "This hurts": what the athlete said about a movement, which the set log cannot
//! hold. Stored as reported; what it means (a rest, then an eased return) is the pure
//! engine's call ([`coach_pacing::pacing::engine`]).

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::NaiveDateTime;
use coach_pacing::domain::ExerciseId;
use sqlx::MySqlPool;

/// How far back the engine is shown reports: the two-week rest, and long enough
/// after it to see whether the movement has been done since.
pub const HURT_WEEKS: i64 = 8;

/// Record that `exercise` hurt, at `at` (UTC).
pub async fn record(
    pool: &MySqlPool,
    user_id: &str,
    exercise: ExerciseId,
    at: NaiveDateTime,
) -> Result<()> {
    sqlx::query("INSERT INTO hurts (user_id, exercise_id, reported_at) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind(exercise.get())
        .bind(at)
        .execute(pool)
        .await?;
    Ok(())
}

/// Take back the reports on `exercise` since `from` (UTC): a mis-tap, or "it was
/// fine after all". Older ones stay; they are history.
pub async fn take_back(
    pool: &MySqlPool,
    user_id: &str,
    exercise: ExerciseId,
    from: NaiveDateTime,
) -> Result<()> {
    sqlx::query("DELETE FROM hurts WHERE user_id = ? AND exercise_id = ? AND reported_at >= ?")
        .bind(user_id)
        .bind(exercise.get())
        .bind(from)
        .execute(pool)
        .await?;
    Ok(())
}

/// The latest report per movement since `from` (UTC).
pub async fn since(
    pool: &MySqlPool,
    user_id: &str,
    from: NaiveDateTime,
) -> Result<BTreeMap<ExerciseId, NaiveDateTime>> {
    let rows: Vec<(i64, NaiveDateTime)> = sqlx::query_as(
        "SELECT exercise_id, MAX(reported_at) FROM hurts \
         WHERE user_id = ? AND reported_at >= ? GROUP BY exercise_id",
    )
    .bind(user_id)
    .bind(from)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(ex, at)| (ExerciseId(ex), at))
        .collect())
}
