//! Boot-time catalog seeder. Loads the global training library (equipment,
//! muscle taxonomy, exercises, their M:N links, and image blobs) from the
//! `data/catalog/` bundle into the DB. Idempotent: reference rows use the unique
//! slug, and an exercise already present is left untouched — so it runs on every
//! boot but only does work on a fresh (or newly-extended) database.
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
    if !dir.join("exercises.json").exists() {
        tracing::warn!(
            "catalog bundle not found at {} — skipping library seed (set CATALOG_DIR)",
            dir.display()
        );
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

    // Exercises + links + image, skipping any already seeded (by slug).
    let existing: HashMap<String, i64> = sqlx::query_as("SELECT slug, id FROM exercises")
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();

    let exercises = read_json::<Vec<SeedExercise>>(&dir.join("exercises.json"))?;
    let mut inserted = 0usize;
    for ex in &exercises {
        if existing.contains_key(&ex.slug) {
            continue;
        }
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
        let id = res.last_insert_id() as i64;

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
        if let Some(img) = &ex.image {
            let path = dir.join("images").join(&img.file);
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let etag = hex::encode(Sha256::digest(&bytes));
            image::insert_if_absent(pool, id, &img.content_type, &bytes, &etag).await?;
        }
        inserted += 1;
    }
    if inserted > 0 {
        tracing::info!("seeded {inserted} exercises from the catalog");
    }
    Ok(())
}
