//! Equipment catalog queries (global). SQL as `&'static str` literals.

use anyhow::Result;
use sqlx::MySqlPool;

use super::types::{Equipment, EquipmentRow};

pub async fn list(pool: &MySqlPool) -> Result<Vec<Equipment>> {
    sqlx::query_as::<_, EquipmentRow>(
        "SELECT id, slug, name, category FROM equipment ORDER BY category, name",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Equipment::try_from)
    .collect()
}
