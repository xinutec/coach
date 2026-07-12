//! Location queries. Per-user (scoped by `user_id`), soft-deleted. At most one
//! default location per user. SQL as `&'static str` literals.

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use sqlx::MySqlPool;

use super::loads;
use super::types::{EquipmentOption, Location, LocationPatch, LocationRow, NewLocation, Plate};

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
        loc.plates = plates_wire(pool, loc.id).await?;
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
    loc.plates = plates_wire(pool, loc.id).await?;
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

/// The loadable kit at a location, per equipment id: fixed free weights (with how
/// many of each you own), the loadable bar/handle if it is one, and the plates
/// that fit *it* — the shared pool plus anything pinned to this kit alone.
///
/// Deliberately raw facts, not loads: what's buildable depends on how many
/// implements the *movement* needs (a pair of dumbbells splits the disc budget),
/// which only the exercise knows. `loads::loads_for` does that per exercise.
pub async fn kit_loads(
    pool: &MySqlPool,
    location_id: i64,
) -> Result<HashMap<i64, loads::KitLoads>> {
    let plates = load_plates(pool, location_id).await?;
    let rows: Vec<KitRow> = sqlx::query_as(
        "SELECT equipment_id, load_kg, kind, qty, plate_slots FROM location_equipment_option \
         WHERE location_id = ? AND load_kg IS NOT NULL",
    )
    .bind(location_id)
    .fetch_all(pool)
    .await?;

    let qty = |q: Option<i32>| q.and_then(|q| u32::try_from(q).ok());
    let mut map: HashMap<i64, loads::KitLoads> = HashMap::new();
    for (id, load, kind, q, slots) in rows {
        let Some(kg) = load else { continue };
        let entry = map.entry(id).or_default();
        match kind.as_deref() {
            // A loadable bar or dumbbell handle.
            Some("bar") => {
                entry.bar = Some(loads::Bar {
                    kg,
                    qty: qty(q),
                    slots: qty(slots),
                });
            }
            // A fixed free weight (one 5 kg dumbbell, a 16 kg kettlebell).
            _ => entry.fixed.push((kg, qty(q))),
        }
    }
    // Plates reach the kit they fit: the shared pool (an Olympic disc goes on the
    // barbell *and* the trap bar) plus any pinned to one piece of kit (a dumbbell
    // handle's small discs will not go on an Olympic sleeve).
    for (id, kit) in map.iter_mut() {
        kit.plates = plates
            .iter()
            .filter(|p| p.equipment_id.is_none_or(|e| e == *id))
            .map(|p| loads::Plate {
                kg: p.load_kg,
                qty: p.qty,
            })
            .collect();
    }
    Ok(map)
}

/// One kit-loads row: (equipment_id, load_kg, kind, qty, plate_slots).
type KitRow = (i64, Option<f64>, Option<String>, Option<i32>, Option<i32>);

/// One plate row: (equipment_id, slug, load_kg, qty).
type PlateSqlRow = (Option<i64>, Option<String>, f64, Option<i32>);

/// One `location_equipment_option` row: (slug, load_kg, label, kind, qty, slots).
type OptionRow = (
    String,
    Option<f64>,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
);

/// A plate as stored: the kit it fits (`None` = the shared pool every loadable bar
/// here draws from), carried as both id (for the engine's load expansion) and slug
/// (for the wire), plus its size and how many you own.
struct PlateRow {
    equipment_id: Option<i64>,
    equipment: Option<String>,
    load_kg: f64,
    qty: Option<u32>,
}

/// A location's plates (ascending).
async fn load_plates(pool: &MySqlPool, location_id: i64) -> Result<Vec<PlateRow>> {
    let rows: Vec<PlateSqlRow> = sqlx::query_as(
        "SELECT p.equipment_id, eq.slug, p.load_kg, p.qty \
         FROM location_plate p LEFT JOIN equipment eq ON eq.id = p.equipment_id \
         WHERE p.location_id = ? ORDER BY p.load_kg",
    )
    .bind(location_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(equipment_id, equipment, load_kg, qty)| PlateRow {
            equipment_id,
            equipment,
            load_kg,
            qty: qty.and_then(|q| u32::try_from(q).ok()),
        })
        .collect())
}

/// A location's plates as the wire sees them (kit named by slug).
async fn plates_wire(pool: &MySqlPool, location_id: i64) -> Result<Vec<Plate>> {
    Ok(load_plates(pool, location_id)
        .await?
        .into_iter()
        .map(|p| Plate {
            equipment: p.equipment,
            load_kg: p.load_kg,
            qty: p.qty,
        })
        .collect())
}

/// Replace a location's plate set. A plate with no `equipment` lands in the shared
/// pool (the sub-select yields NULL), which is what an Olympic disc is.
async fn set_plates(pool: &MySqlPool, location_id: i64, plates: &[Plate]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM location_plate WHERE location_id = ?")
        .bind(location_id)
        .execute(&mut *tx)
        .await?;
    for p in plates {
        sqlx::query(
            "INSERT INTO location_plate (location_id, equipment_id, load_kg, qty) \
             VALUES (?, (SELECT id FROM equipment WHERE slug = ?), ?, ?)",
        )
        .bind(location_id)
        .bind(p.equipment.as_deref())
        .bind(p.load_kg)
        .bind(p.qty.map(|q| q as i32))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Load a location's per-equipment specifics, grouped by equipment slug. A row's
/// `kind` of 'bar' is a loadable bar/handle (its weight, how many you own, and how
/// many discs its sleeve takes); untagged rows are a fixed weight (`load_kg` + how
/// many of it) or a band (`label`). Plates live on the location, not here.
async fn load_options(pool: &MySqlPool, location_id: i64) -> Result<Vec<EquipmentOption>> {
    let rows: Vec<OptionRow> = sqlx::query_as(
        "SELECT eq.slug, o.load_kg, o.label, o.kind, o.qty, o.plate_slots \
         FROM location_equipment_option o JOIN equipment eq ON eq.id = o.equipment_id \
         WHERE o.location_id = ? ORDER BY eq.name, o.load_kg, o.label",
    )
    .bind(location_id)
    .fetch_all(pool)
    .await?;
    let u32_of = |q: Option<i32>| q.and_then(|q| u32::try_from(q).ok());
    let mut out: Vec<EquipmentOption> = Vec::new();
    for (slug, load, label, kind, qty, slots) in rows {
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
            (Some("bar"), Some(w), _) => {
                e.bar_kg = Some(w);
                e.bar_qty = u32_of(qty);
                e.plate_slots = u32_of(slots);
            }
            (_, Some(w), _) => {
                e.weights.push(w);
                // Parallel arrays: a weight with no count means "plenty" (0 here,
                // read back as None), which is what a gym rack is.
                e.weight_qty.push(u32_of(qty).unwrap_or(0));
            }
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
        for (i, w) in o.weights.iter().enumerate() {
            // 0 / absent = "plenty" (a gym rack); otherwise how many you own, which
            // is what decides whether a two-dumbbell movement can use it.
            let qty = o.weight_qty.get(i).copied().filter(|q| *q > 0);
            insert_option(
                &mut tx,
                location_id,
                &o.slug,
                None,
                Some(*w),
                None,
                qty,
                None,
            )
            .await?;
        }
        for label in &o.labels {
            let label = label.trim();
            if !label.is_empty() {
                insert_option(
                    &mut tx,
                    location_id,
                    &o.slug,
                    None,
                    None,
                    Some(label),
                    None,
                    None,
                )
                .await?;
            }
        }
        if let Some(bar) = o.bar_kg {
            insert_option(
                &mut tx,
                location_id,
                &o.slug,
                Some("bar"),
                Some(bar),
                None,
                o.bar_qty,
                o.plate_slots,
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Insert one specifics row (resolving the equipment by slug) inside a tx.
#[allow(clippy::too_many_arguments)]
async fn insert_option(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    location_id: i64,
    slug: &str,
    kind: Option<&str>,
    load: Option<f64>,
    label: Option<&str>,
    qty: Option<u32>,
    plate_slots: Option<u32>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO location_equipment_option \
           (location_id, equipment_id, kind, load_kg, label, qty, plate_slots) \
         SELECT ?, id, ?, ?, ?, ?, ? FROM equipment WHERE slug = ?",
    )
    .bind(location_id)
    .bind(kind)
    .bind(load)
    .bind(label)
    .bind(qty.map(|q| q as i32))
    .bind(plate_slots.map(|q| q as i32))
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
