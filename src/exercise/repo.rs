//! Exercise catalog queries. The catalog is global (not per-user).
//! SQL is written as `&'static str` literals (sqlx 0.9's SqlSafeStr guard);
//! no user data is ever interpolated into a query string. Enum columns are read
//! as strings and converted; equipment/muscles are joined from the M:N tables.

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use sqlx::MySqlPool;

use super::types::{
    Exercise, ExerciseDetail, ExerciseDetailRow, ExerciseListRow, ExerciseMuscle,
    ExerciseMuscleRow, ExercisePatch, NewExercise,
};
use crate::equipment::repo::eq_cols;
use crate::equipment::types::{Equipment, EquipmentRow};
use crate::muscle::types::MuscleRole;

// Equipment slugs (comma-joined) + image presence, as correlated subqueries so
// the list stays one row per exercise without a GROUP BY. A macro (not a const)
// so `concat!` can fold it into the `&'static str` queries below.
macro_rules! list_cols {
    () => {
        "e.id, e.slug, e.name, e.variation, e.pattern, e.metric, e.unilateral, e.skill, e.warmup, e.implements, e.difficulty, e.is_active, \
         (SELECT GROUP_CONCAT(eq.slug ORDER BY eq.name SEPARATOR ',') \
            FROM exercise_equipment xe JOIN equipment eq ON eq.id = xe.equipment_id \
            WHERE xe.exercise_id = e.id) AS equipment_csv, \
         EXISTS(SELECT 1 FROM exercise_images i WHERE i.exercise_id = e.id) AS has_image"
    };
}

pub async fn list(pool: &MySqlPool, include_inactive: bool) -> Result<Vec<Exercise>> {
    let q = if include_inactive {
        sqlx::query_as::<_, ExerciseListRow>(concat!(
            "SELECT ",
            list_cols!(),
            " FROM exercises e ORDER BY e.pattern, e.name, e.variation"
        ))
    } else {
        sqlx::query_as::<_, ExerciseListRow>(concat!(
            "SELECT ",
            list_cols!(),
            " FROM exercises e WHERE e.is_active = 1 ORDER BY e.pattern, e.name, e.variation"
        ))
    };
    q.fetch_all(pool)
        .await?
        .into_iter()
        .map(Exercise::try_from)
        .collect()
}

pub async fn get(pool: &MySqlPool, id: i64) -> Result<Option<Exercise>> {
    sqlx::query_as::<_, ExerciseListRow>(concat!(
        "SELECT ",
        list_cols!(),
        " FROM exercises e WHERE e.id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(Exercise::try_from)
    .transpose()
}

pub async fn detail(pool: &MySqlPool, id: i64) -> Result<Option<ExerciseDetail>> {
    let Some(row) = sqlx::query_as::<_, ExerciseDetailRow>(
        "SELECT e.id, e.slug, e.name, e.variation, e.pattern, e.metric, e.position, \
                e.unilateral, e.is_active, e.cue, e.demo_url, e.summary, e.difficulty, \
                EXISTS(SELECT 1 FROM exercise_images i WHERE i.exercise_id = e.id) AS has_image \
         FROM exercises e WHERE e.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    let equipment = sqlx::query_as::<_, EquipmentRow>(concat!(
        "SELECT ",
        eq_cols!(),
        " FROM exercise_equipment xe JOIN equipment eq ON eq.id = xe.equipment_id \
          WHERE xe.exercise_id = ? ORDER BY eq.category, eq.name"
    ))
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Equipment::try_from)
    .collect::<Result<Vec<_>>>()?;

    let muscles = sqlx::query_as::<_, ExerciseMuscleRow>(
        "SELECT m.slug, m.name, g.name AS `group`, g.region, xm.role \
         FROM exercise_muscle xm JOIN muscles m ON m.id = xm.muscle_id \
         JOIN muscle_groups g ON g.id = m.muscle_group_id \
         WHERE xm.exercise_id = ? \
         ORDER BY FIELD(xm.role, 'primary','secondary','stabilizer'), m.name",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(ExerciseMuscle::try_from)
    .collect::<Result<Vec<_>>>()?;

    use super::types::{Metric, Pattern, Position};
    Ok(Some(ExerciseDetail {
        id: row.id,
        slug: row.slug,
        name: row.name,
        variation: row.variation,
        pattern: Pattern::from_db(&row.pattern)
            .ok_or_else(|| anyhow!("unknown pattern {:?}", row.pattern))?,
        metric: Metric::from_db(&row.metric)
            .ok_or_else(|| anyhow!("unknown metric {:?}", row.metric))?,
        position: row
            .position
            .as_deref()
            .map(|p| Position::from_db(p).ok_or_else(|| anyhow!("unknown position {p:?}")))
            .transpose()?,
        unilateral: row.unilateral,
        is_active: row.is_active,
        cue: row.cue,
        demo_url: row.demo_url,
        summary: row.summary,
        difficulty: row.difficulty,
        has_image: row.has_image != 0,
        equipment,
        muscles,
    }))
}

/// Slug from a display name: lowercase, non-alnum → `_`, collapsed.
fn slugify(name: &str) -> String {
    let mut s = String::new();
    let mut prev_us = false;
    for ch in name.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch);
            prev_us = false;
        } else if !prev_us {
            s.push('_');
            prev_us = true;
        }
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        "exercise".to_string()
    } else {
        s
    }
}

pub async fn create(pool: &MySqlPool, e: &NewExercise) -> Result<ExerciseDetail> {
    let base = slugify(
        &e.variation
            .as_deref()
            .map_or_else(|| e.name.clone(), |v| format!("{} {v}", e.name)),
    );
    for attempt in 1..=50 {
        let slug = if attempt == 1 {
            base.clone()
        } else {
            format!("{base}_{attempt}")
        };
        let res = sqlx::query(
            "INSERT INTO exercises \
               (slug, name, variation, pattern, metric, position, unilateral, cue, demo_url, difficulty) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&slug)
        .bind(&e.name)
        .bind(&e.variation)
        .bind(e.pattern.as_db())
        .bind(e.metric.as_db())
        .bind(e.position.map(|p| p.as_db()))
        .bind(e.unilateral)
        .bind(&e.cue)
        .bind(&e.demo_url)
        .bind(e.difficulty)
        .execute(pool)
        .await;
        match res {
            Ok(r) => {
                let id = r.last_insert_id() as i64;
                set_equipment(pool, id, &e.equipment).await?;
                let links: Vec<(String, &str)> = e
                    .muscles
                    .iter()
                    .map(|m| (m.slug.clone(), m.role.as_db()))
                    .collect();
                set_muscles(pool, id, &links).await?;
                return detail(pool, id)
                    .await?
                    .ok_or_else(|| anyhow!("exercise vanished after insert"));
            }
            Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23000") => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow!("could not allocate a unique slug for {:?}", e.name))
}

pub async fn patch(pool: &MySqlPool, id: i64, p: &ExercisePatch) -> Result<Option<ExerciseDetail>> {
    sqlx::query(
        "UPDATE exercises SET \
           name = COALESCE(?, name), \
           variation = COALESCE(?, variation), \
           pattern = COALESCE(?, pattern), \
           metric = COALESCE(?, metric), \
           position = COALESCE(?, position), \
           unilateral = COALESCE(?, unilateral), \
           cue = COALESCE(?, cue), \
           demo_url = COALESCE(?, demo_url), \
           summary = COALESCE(?, summary), \
           difficulty = COALESCE(?, difficulty), \
           is_active = COALESCE(?, is_active), \
           updated_at = NOW() \
         WHERE id = ?",
    )
    .bind(&p.name)
    .bind(&p.variation)
    .bind(p.pattern.map(|e| e.as_db()))
    .bind(p.metric.map(|e| e.as_db()))
    .bind(p.position.map(|e| e.as_db()))
    .bind(p.unilateral)
    .bind(&p.cue)
    .bind(&p.demo_url)
    .bind(&p.summary)
    .bind(p.difficulty)
    .bind(p.is_active)
    .bind(id)
    .execute(pool)
    .await?;

    if let Some(slugs) = &p.equipment {
        set_equipment(pool, id, slugs).await?;
    }
    if let Some(links) = &p.muscles {
        let links: Vec<(String, &str)> = links
            .iter()
            .map(|m| (m.slug.clone(), m.role.as_db()))
            .collect();
        set_muscles(pool, id, &links).await?;
    }
    detail(pool, id).await
}

/// Replace an exercise's equipment set with the given slugs (unknown slugs are
/// ignored). One transaction.
pub async fn set_equipment(pool: &MySqlPool, exercise_id: i64, slugs: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM exercise_equipment WHERE exercise_id = ?")
        .bind(exercise_id)
        .execute(&mut *tx)
        .await?;
    for slug in slugs {
        sqlx::query(
            "INSERT IGNORE INTO exercise_equipment (exercise_id, equipment_id) \
             SELECT ?, id FROM equipment WHERE slug = ?",
        )
        .bind(exercise_id)
        .bind(slug)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Replace an exercise's muscle links with `(slug, role)` pairs.
pub async fn set_muscles(
    pool: &MySqlPool,
    exercise_id: i64,
    links: &[(String, &str)],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM exercise_muscle WHERE exercise_id = ?")
        .bind(exercise_id)
        .execute(&mut *tx)
        .await?;
    for (slug, role) in links {
        sqlx::query(
            "INSERT IGNORE INTO exercise_muscle (exercise_id, muscle_id, role) \
             SELECT ?, id, ? FROM muscles WHERE slug = ?",
        )
        .bind(exercise_id)
        .bind(role)
        .bind(slug)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// exercise id → required equipment ids (for the pacing engine's doability check).
pub async fn equipment_by_exercise(pool: &MySqlPool) -> Result<HashMap<i64, Vec<i64>>> {
    let rows: Vec<(i64, i64)> =
        sqlx::query_as("SELECT exercise_id, equipment_id FROM exercise_equipment")
            .fetch_all(pool)
            .await?;
    let mut map: HashMap<i64, Vec<i64>> = HashMap::new();
    for (ex, eq) in rows {
        map.entry(ex).or_default().push(eq);
    }
    Ok(map)
}

/// exercise id → primary muscle ids (for the pacing engine's substitution ranking).
pub async fn primary_muscles_by_exercise(pool: &MySqlPool) -> Result<HashMap<i64, Vec<i64>>> {
    let rows: Vec<(i64, i64)> =
        sqlx::query_as("SELECT exercise_id, muscle_id FROM exercise_muscle WHERE role = 'primary'")
            .fetch_all(pool)
            .await?;
    let mut map: HashMap<i64, Vec<i64>> = HashMap::new();
    for (ex, mus) in rows {
        map.entry(ex).or_default().push(mus);
    }
    Ok(map)
}

/// exercise id → its muscle GROUPS with the strongest role, for crediting rolling
/// volume per group. Several muscles of an exercise can share a group (e.g. three
/// quad heads → quadriceps); `MIN(role)` collapses to one row per group taking the
/// strongest role ('primary' < 'secondary' < 'stabilizer' lexically).
pub async fn muscle_groups_by_exercise(
    pool: &MySqlPool,
) -> Result<HashMap<i64, Vec<(i64, MuscleRole)>>> {
    let rows: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT xm.exercise_id, m.muscle_group_id AS grp, MIN(xm.role) AS role \
         FROM exercise_muscle xm JOIN muscles m ON m.id = xm.muscle_id \
         GROUP BY xm.exercise_id, m.muscle_group_id",
    )
    .fetch_all(pool)
    .await?;
    let mut map: HashMap<i64, Vec<(i64, MuscleRole)>> = HashMap::new();
    for (ex, grp, role) in rows {
        let role = MuscleRole::from_db(&role).ok_or_else(|| anyhow!("unknown role {role:?}"))?;
        map.entry(ex).or_default().push((grp, role));
    }
    Ok(map)
}
