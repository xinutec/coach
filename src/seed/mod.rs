//! Boot-time catalog seeder. Loads the global training library (equipment,
//! muscle taxonomy, exercises, their M:N links, and image blobs) from the
//! `data/catalog/` bundle into the DB.
//!
//! Hash-gated: `exercises.json` is fingerprinted (SHA-256) into `catalog_state`.
//! An unchanged fingerprint short-circuits the whole seed (fast normal boots). A
//! changed one (a fresh DB, a new exercise, or a corrected muscle/equipment
//! mapping) runs the seed AND **reconciles** the M:N links of already-seeded
//! exercises — delete + re-insert — so catalog corrections actually land instead
//! of being skipped forever. Scalar exercise rows + images are insert-only.
//!
//! This keeps the 119-row catalog and its ~15 MB of images out of SQL migrations
//! while still making any fresh DB (dev or prod) reproduce the full library.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::MySqlPool;

use crate::exercise::image;

#[derive(Deserialize)]
struct SeedEquipment {
    slug: String,
    name: String,
    category: String,
}

#[derive(Deserialize)]
struct SeedGroup {
    slug: String,
    name: String,
    region: String,
}

#[derive(Deserialize)]
struct SeedMuscle {
    slug: String,
    name: String,
    group: String,
    function: Option<String>,
}

#[derive(Deserialize)]
struct SeedMuscleLink {
    slug: String,
    role: String,
}

#[derive(Deserialize)]
struct SeedImage {
    file: String,
    #[serde(rename = "type")]
    content_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedExercise {
    slug: String,
    name: String,
    variation: Option<String>,
    pattern: String,
    metric: String,
    position: Option<String>,
    unilateral: bool,
    cue: Option<String>,
    demo_url: Option<String>,
    summary: Option<String>,
    muscles: Vec<SeedMuscleLink>,
    equipment: Vec<String>,
    image: Option<SeedImage>,
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

pub async fn run(pool: &MySqlPool, catalog_dir: &str) -> Result<()> {
    let dir = Path::new(catalog_dir);
    let exercises_path = dir.join("exercises.json");
    if !exercises_path.exists() {
        tracing::warn!(
            "catalog bundle not found at {} — skipping library seed (set CATALOG_DIR)",
            dir.display()
        );
        return Ok(());
    }

    // Fingerprint the catalog; an unchanged hash means nothing to do.
    let exercises_bytes = std::fs::read(&exercises_path)
        .with_context(|| format!("reading {}", exercises_path.display()))?;
    let catalog_hash = hex::encode(Sha256::digest(&exercises_bytes));
    let stored_hash: Option<String> =
        sqlx::query_scalar("SELECT catalog_hash FROM catalog_state WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    if stored_hash.as_deref() == Some(catalog_hash.as_str()) {
        return Ok(());
    }

    // Equipment.
    for e in read_json::<Vec<SeedEquipment>>(&dir.join("equipment.json"))? {
        sqlx::query("INSERT IGNORE INTO equipment (slug, name, category) VALUES (?, ?, ?)")
            .bind(&e.slug)
            .bind(&e.name)
            .bind(&e.category)
            .execute(pool)
            .await?;
    }

    // Muscle groups, then muscles (group resolved by slug).
    for g in read_json::<Vec<SeedGroup>>(&dir.join("muscle-groups.json"))? {
        sqlx::query("INSERT IGNORE INTO muscle_groups (slug, name, region) VALUES (?, ?, ?)")
            .bind(&g.slug)
            .bind(&g.name)
            .bind(&g.region)
            .execute(pool)
            .await?;
    }
    for m in read_json::<Vec<SeedMuscle>>(&dir.join("muscles.json"))? {
        sqlx::query(
            "INSERT IGNORE INTO muscles (slug, name, muscle_group_id, function) \
             SELECT ?, ?, id, ? FROM muscle_groups WHERE slug = ?",
        )
        .bind(&m.slug)
        .bind(&m.name)
        .bind(&m.function)
        .bind(&m.group)
        .execute(pool)
        .await?;
    }

    // Exercises: insert new ones, reconcile the M:N links of existing ones (the
    // catalog is the source of truth for links). Scalar rows + images are
    // insert-only — a correction there is out of this bounded pass's scope.
    let existing: HashMap<String, i64> = sqlx::query_as("SELECT slug, id FROM exercises")
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();

    let exercises: Vec<SeedExercise> = serde_json::from_slice(&exercises_bytes)
        .with_context(|| format!("parsing {}", exercises_path.display()))?;
    let mut inserted = 0usize;
    let mut reconciled = 0usize;
    for ex in &exercises {
        let (id, is_new) = match existing.get(&ex.slug) {
            Some(&id) => {
                // Clear the M:N links so the catalog's are authoritative.
                sqlx::query("DELETE FROM exercise_equipment WHERE exercise_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM exercise_muscle WHERE exercise_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
                reconciled += 1;
                (id, false)
            }
            None => {
                let position = ex.position.as_deref().map(|p| p.replace(' ', "_"));
                let res = sqlx::query(
                    "INSERT INTO exercises \
                       (slug, name, variation, pattern, metric, position, unilateral, cue, demo_url, summary) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&ex.slug)
                .bind(&ex.name)
                .bind(&ex.variation)
                .bind(&ex.pattern)
                .bind(&ex.metric)
                .bind(&position)
                .bind(ex.unilateral)
                .bind(&ex.cue)
                .bind(&ex.demo_url)
                .bind(&ex.summary)
                .execute(pool)
                .await
                .with_context(|| format!("inserting exercise {}", ex.slug))?;
                inserted += 1;
                (res.last_insert_id() as i64, true)
            }
        };

        for slug in &ex.equipment {
            sqlx::query(
                "INSERT IGNORE INTO exercise_equipment (exercise_id, equipment_id) \
                 SELECT ?, id FROM equipment WHERE slug = ?",
            )
            .bind(id)
            .bind(slug)
            .execute(pool)
            .await?;
        }
        for l in &ex.muscles {
            sqlx::query(
                "INSERT IGNORE INTO exercise_muscle (exercise_id, muscle_id, role) \
                 SELECT ?, id, ? FROM muscles WHERE slug = ?",
            )
            .bind(id)
            .bind(&l.role)
            .bind(&l.slug)
            .execute(pool)
            .await?;
        }
        if is_new && let Some(img) = &ex.image {
            let path = dir.join("images").join(&img.file);
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let etag = hex::encode(Sha256::digest(&bytes));
            image::insert_if_absent(pool, id, &img.content_type, &bytes, &etag).await?;
        }
    }

    // Record the fingerprint so the next unchanged boot short-circuits.
    sqlx::query(
        "INSERT INTO catalog_state (id, catalog_hash) VALUES (1, ?) \
         ON DUPLICATE KEY UPDATE catalog_hash = VALUES(catalog_hash)",
    )
    .bind(&catalog_hash)
    .execute(pool)
    .await?;

    if inserted > 0 || reconciled > 0 {
        tracing::info!("catalog seed: {inserted} inserted, {reconciled} reconciled");
    }
    Ok(())
}
