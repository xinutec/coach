//! The record of what the coach *offered*, which the set log cannot hold.
//!
//! `workout_sets` is the record of what happened; a card that was shown and not
//! taken leaves no trace in it. R6-4 needs both — "offered twenty times,
//! performed zero" — so the offers are written here and the judgment about what
//! that means stays in the pure engine ([`coach_pacing::pacing::engine`]).

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::NaiveDate;
use coach_pacing::domain::ExerciseId;
use sqlx::MySqlPool;

/// How far back the engine is shown offers. The same eight weeks the volume
/// model reasons over: what he skipped last winter is not evidence about how he
/// trains now.
pub const OFFER_WEEKS: i64 = 8;

/// Record that these movements were on today's card.
///
/// Idempotent per (user, day, movement) — the verdict is recomputed on every
/// poll, and an offer is a fact about the day, not about the request. Warm-ups
/// are excluded by the caller: a mobility drill nobody does is not the finding.
pub async fn record(
    pool: &MySqlPool,
    user_id: &str,
    on: NaiveDate,
    exercises: &[ExerciseId],
) -> Result<()> {
    for ex in exercises {
        sqlx::query(
            "INSERT INTO plan_offers (user_id, offered_on, exercise_id) VALUES (?, ?, ?) \
             ON DUPLICATE KEY UPDATE exercise_id = exercise_id",
        )
        .bind(user_id)
        .bind(on)
        .bind(ex.get())
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Every movement offered since `from`, with the days it was offered on.
pub async fn since(
    pool: &MySqlPool,
    user_id: &str,
    from: NaiveDate,
) -> Result<BTreeMap<ExerciseId, Vec<NaiveDate>>> {
    let rows: Vec<(i64, NaiveDate)> = sqlx::query_as(
        "SELECT exercise_id, offered_on FROM plan_offers \
         WHERE user_id = ? AND offered_on >= ? ORDER BY offered_on",
    )
    .bind(user_id)
    .bind(from)
    .fetch_all(pool)
    .await?;
    let mut map: BTreeMap<ExerciseId, Vec<NaiveDate>> = BTreeMap::new();
    for (ex, day) in rows {
        map.entry(ExerciseId(ex)).or_default().push(day);
    }
    Ok(map)
}
