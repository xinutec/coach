//! Muscle taxonomy queries (global). SQL as `&'static str` literals.

use anyhow::Result;
use sqlx::MySqlPool;

use super::types::{Muscle, MuscleRow};

pub async fn list(pool: &MySqlPool) -> Result<Vec<Muscle>> {
    sqlx::query_as::<_, MuscleRow>(
        "SELECT m.id, m.slug, m.name, g.name AS `group`, g.region, m.function \
         FROM muscles m JOIN muscle_groups g ON g.id = m.muscle_group_id \
         ORDER BY g.region, g.name, m.name",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Muscle::try_from)
    .collect()
}

/// Muscle groups as (id, name, region-string) — for the engine's per-group volume.
pub async fn groups(pool: &MySqlPool) -> Result<Vec<(i64, String, String)>> {
    Ok(
        sqlx::query_as("SELECT id, name, region FROM muscle_groups ORDER BY region, name")
            .fetch_all(pool)
            .await?,
    )
}
