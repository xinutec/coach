//! Location queries. Per-user (scoped by `user_id`), soft-deleted. At most one
//! default location per user. SQL as `&'static str` literals.

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use sqlx::MySqlPool;

use super::types::{EquipmentOption, Location, LocationPatch, LocationRow, NewLocation};

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
    let mut locs: Vec<Location> = rows.into_iter().map(Location::from).collect();
    for loc in &mut locs {
        loc.equipment_options = load_options(pool, loc.id).await?;
        loc.plates = load_plates(pool, loc.id).await?;
    }
    Ok(locs)
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
    let Some(row) = row else { return Ok(None) };
    let mut loc = Location::from(row);
    loc.equipment_options = load_options(pool, loc.id).await?;
    loc.plates = load_plates(pool, loc.id).await?;
    Ok(Some(loc))
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
    set_options(pool, id, &n.equipment_options).await?;
    set_plates(pool, id, &n.plates).await?;
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
    if let Some(opts) = &p.equipment_options {
        set_options(pool, id, opts).await?;
    }
    if let Some(plates) = &p.plates {
        set_plates(pool, id, plates).await?;
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

/// Discrete loadable weights per equipment id at a location, for the engine's
/// load snapping. Sorted ascending. Fixed free weights → the owned weights
/// directly; a loadable bar → every total buildable from its bar + the location's
/// shared plates (so suggestions never go below the empty bar or land on an
/// unbuildable weight).
pub async fn equipment_loads(pool: &MySqlPool, location_id: i64) -> Result<HashMap<i64, Vec<f64>>> {
    let plates = load_plates(pool, location_id).await?;
    let rows: Vec<(i64, Option<f64>, Option<String>)> = sqlx::query_as(
        "SELECT equipment_id, load_kg, kind FROM location_equipment_option \
         WHERE location_id = ? AND load_kg IS NOT NULL",
    )
    .bind(location_id)
    .fetch_all(pool)
    .await?;
    // Separate loadable bars (a 'bar' row) from fixed weights.
    let mut fixed: HashMap<i64, Vec<f64>> = HashMap::new();
    let mut bars: HashMap<i64, f64> = HashMap::new();
    for (id, load, kind) in rows {
        let Some(w) = load else { continue };
        match kind.as_deref() {
            Some("bar") => {
                bars.insert(id, w);
            }
            _ => fixed.entry(id).or_default().push(w),
        }
    }
    let mut map: HashMap<i64, Vec<f64>> = HashMap::new();
    for (id, bar) in bars {
        map.insert(id, super::loads::reachable_loads(bar, &plates));
    }
    for (id, mut ws) in fixed {
        ws.sort_by(f64::total_cmp);
        map.entry(id).or_default().extend(ws);
    }
    Ok(map)
}

/// A location's shared plate sizes (kg, ascending).
async fn load_plates(pool: &MySqlPool, location_id: i64) -> Result<Vec<f64>> {
    let rows: Vec<(f64,)> =
        sqlx::query_as("SELECT load_kg FROM location_plate WHERE location_id = ? ORDER BY load_kg")
            .bind(location_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(w,)| w).collect())
}

/// Replace a location's shared plate set.
async fn set_plates(pool: &MySqlPool, location_id: i64, plates: &[f64]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM location_plate WHERE location_id = ?")
        .bind(location_id)
        .execute(&mut *tx)
        .await?;
    for p in plates {
        sqlx::query("INSERT INTO location_plate (location_id, load_kg) VALUES (?, ?)")
            .bind(location_id)
            .bind(p)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Load a location's per-equipment specifics, grouped by equipment slug. A row's
/// `kind` of 'bar' is a loadable bar's own weight; untagged rows are a fixed
/// weight (`load_kg`) or band (`label`). Plates live on the location, not here.
async fn load_options(pool: &MySqlPool, location_id: i64) -> Result<Vec<EquipmentOption>> {
    // (slug, load_kg, label, kind) per option row.
    type OptionRow = (String, Option<f64>, Option<String>, Option<String>);
    let rows: Vec<OptionRow> = sqlx::query_as(
        "SELECT eq.slug, o.load_kg, o.label, o.kind \
         FROM location_equipment_option o JOIN equipment eq ON eq.id = o.equipment_id \
         WHERE o.location_id = ? ORDER BY eq.name, o.load_kg, o.label",
    )
    .bind(location_id)
    .fetch_all(pool)
    .await?;
    let mut out: Vec<EquipmentOption> = Vec::new();
    for (slug, load, label, kind) in rows {
        let e = match out.iter_mut().find(|o| o.slug == slug) {
            Some(e) => e,
            None => {
                out.push(EquipmentOption {
                    slug,
                    ..Default::default()
                });
                out.last_mut().unwrap()
            }
        };
        match (kind.as_deref(), load, label) {
            (Some("bar"), Some(w), _) => e.bar_kg = Some(w),
            (_, Some(w), _) => e.weights.push(w),
            (_, _, Some(l)) => e.labels.push(l),
            _ => {}
        }
    }
    Ok(out)
}

/// Replace a location's per-equipment specifics: fixed weights, band variants,
/// and each loadable bar's own weight (a 'bar' row). Plates are set separately.
async fn set_options(pool: &MySqlPool, location_id: i64, opts: &[EquipmentOption]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM location_equipment_option WHERE location_id = ?")
        .bind(location_id)
        .execute(&mut *tx)
        .await?;
    for o in opts {
        for w in &o.weights {
            insert_option(&mut tx, location_id, &o.slug, None, Some(*w), None).await?;
        }
        for label in &o.labels {
            let label = label.trim();
            if !label.is_empty() {
                insert_option(&mut tx, location_id, &o.slug, None, None, Some(label)).await?;
            }
        }
        if let Some(bar) = o.bar_kg {
            insert_option(&mut tx, location_id, &o.slug, Some("bar"), Some(bar), None).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Insert one specifics row (resolving the equipment by slug) inside a tx.
async fn insert_option(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    location_id: i64,
    slug: &str,
    kind: Option<&str>,
    load: Option<f64>,
    label: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO location_equipment_option \
           (location_id, equipment_id, kind, load_kg, label) \
         SELECT ?, id, ?, ?, ? FROM equipment WHERE slug = ?",
    )
    .bind(location_id)
    .bind(kind)
    .bind(load)
    .bind(label)
    .bind(slug)
    .execute(&mut **tx)
    .await?;
    Ok(())
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
