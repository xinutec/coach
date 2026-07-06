//! Location queries. Per-user (scoped by `user_id`), soft-deleted. At most one
//! default location per user. SQL as `&'static str` literals.

use anyhow::{Result, anyhow};
use sqlx::MySqlPool;

use super::types::{Location, LocationPatch, LocationRow, NewLocation};

macro_rules! loc_cols {
    () => {
        "l.id, l.name, l.is_default, l.health_place_id, \
         (SELECT GROUP_CONCAT(eq.slug ORDER BY eq.name SEPARATOR ',') \
            FROM location_equipment le JOIN equipment eq ON eq.id = le.equipment_id \
            WHERE le.location_id = l.id) AS equipment_csv"
    };
}

pub async fn list(pool: &MySqlPool, user_id: &str) -> Result<Vec<Location>> {
    let rows = sqlx::query_as::<_, LocationRow>(concat!(
        "SELECT ",
        loc_cols!(),
        " FROM locations l WHERE l.user_id = ? AND l.deleted_at IS NULL \
          ORDER BY l.is_default DESC, l.name"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Location::from).collect())
}

pub async fn get(pool: &MySqlPool, user_id: &str, id: i64) -> Result<Option<Location>> {
    let row = sqlx::query_as::<_, LocationRow>(concat!(
        "SELECT ",
        loc_cols!(),
        " FROM locations l WHERE l.id = ? AND l.user_id = ? AND l.deleted_at IS NULL"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Location::from))
}

pub async fn create(pool: &MySqlPool, user_id: &str, n: &NewLocation) -> Result<Location> {
    if n.is_default {
        clear_default(pool, user_id).await?;
    }
    let res = sqlx::query(
        "INSERT INTO locations (user_id, name, is_default, health_place_id) VALUES (?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(&n.name)
    .bind(n.is_default)
    .bind(n.health_place_id)
    .execute(pool)
    .await?;
    let id = res.last_insert_id() as i64;
    set_equipment(pool, id, &n.equipment).await?;
    get(pool, user_id, id)
        .await?
        .ok_or_else(|| anyhow!("location vanished after insert"))
}

pub async fn patch(
    pool: &MySqlPool,
    user_id: &str,
    id: i64,
    p: &LocationPatch,
) -> Result<Option<Location>> {
    if get(pool, user_id, id).await?.is_none() {
        return Ok(None);
    }
    if p.is_default == Some(true) {
        clear_default(pool, user_id).await?;
    }
    sqlx::query(
        "UPDATE locations SET \
           name = COALESCE(?, name), \
           is_default = COALESCE(?, is_default), \
           updated_at = NOW() \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&p.name)
    .bind(p.is_default)
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;
    // Double-option: only touch the link when the field was present; the inner
    // Option (id or NULL) is written verbatim, so `null` unlinks.
    if let Some(place) = p.health_place_id {
        sqlx::query("UPDATE locations SET health_place_id = ? WHERE id = ? AND user_id = ?")
            .bind(place)
            .bind(id)
            .bind(user_id)
            .execute(pool)
            .await?;
    }
    if let Some(slugs) = &p.equipment {
        set_equipment(pool, id, slugs).await?;
    }
    get(pool, user_id, id).await
}

pub async fn delete(pool: &MySqlPool, user_id: &str, id: i64) -> Result<bool> {
    let res = sqlx::query(
        "UPDATE locations SET deleted_at = NOW() \
         WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// The user's (active) location linked to the given health focus_place, if any.
/// Used to auto-select where the user currently is.
pub async fn by_health_place(
    pool: &MySqlPool,
    user_id: &str,
    health_place_id: i64,
) -> Result<Option<i64>> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM locations \
         WHERE user_id = ? AND health_place_id = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(user_id)
    .bind(health_place_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
}

/// The equipment ids available at a location, if it belongs to the user.
pub async fn equipment_ids(
    pool: &MySqlPool,
    user_id: &str,
    location_id: i64,
) -> Result<Option<Vec<i64>>> {
    // Ownership check first (a foreign location id must not leak equipment).
    if get(pool, user_id, location_id).await?.is_none() {
        return Ok(None);
    }
    let rows: Vec<(i64,)> =
        sqlx::query_as("SELECT equipment_id FROM location_equipment WHERE location_id = ?")
            .bind(location_id)
            .fetch_all(pool)
            .await?;
    Ok(Some(rows.into_iter().map(|(id,)| id).collect()))
}

async fn clear_default(pool: &MySqlPool, user_id: &str) -> Result<()> {
    sqlx::query("UPDATE locations SET is_default = 0 WHERE user_id = ? AND deleted_at IS NULL")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn set_equipment(pool: &MySqlPool, location_id: i64, slugs: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM location_equipment WHERE location_id = ?")
        .bind(location_id)
        .execute(&mut *tx)
        .await?;
    for slug in slugs {
        sqlx::query(
            "INSERT IGNORE INTO location_equipment (location_id, equipment_id) \
             SELECT ?, id FROM equipment WHERE slug = ?",
        )
        .bind(location_id)
        .bind(slug)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
