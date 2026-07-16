//! Boot-time catalog seeder. Loads the global training library (equipment,
//! muscle taxonomy, exercises, their M:N links, and image blobs) from the
//! `data/catalog/` bundle into the DB.
//!
//! Hash-gated: the **whole bundle** — every `*.json` in the catalog dir — is
//! fingerprinted (SHA-256) into `catalog_state`. An unchanged fingerprint
//! short-circuits the seed (fast normal boots); a changed one re-seeds and
//! **reconciles**. Gating on `exercises.json` alone made every other file
//! silently un-editable: a corrected `equipment.json` left the hash untouched, so
//! the seed short-circuited and the correction never reached the DB — it looked
//! applied (it was committed, it was in the image) and simply wasn't.
//!
//! **The catalog is the source of truth for every scalar it carries**, and the
//! reconcile writes all of them back to already-seeded rows, not just the flags
//! the engine reads. Reconciling a subset had the same shape of bug: fixing two
//! broken `demo_url`s in the catalog changed the hash, re-ran the seed, and left
//! prod's rows exactly as broken, because `demo_url` wasn't in the UPDATE list.
//! A field the catalog owns but the reconcile skips is a field the catalog only
//! *appears* to own.
//!
//! `is_active` is the one column the catalog does *not* own: the retired
//! `*_legacy` rows (migration 0006) are deliberately absent from it, so the
//! reconcile never sees them.
//!
//! Images seed whenever the row hasn't got one — a movement is catalogued the
//! moment it is real, and the picture turns up later — and are **rendered** on the
//! way in (see [`render`]): the bundle is the source and keeps its alpha, while
//! what the app is served is what the app can actually display.
//!
//! This keeps the exercise catalog and its ~15 MB of images out of SQL migrations
//! while still making any fresh DB (dev or prod) reproduce the full library.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::MySqlPool;

mod render;

use crate::exercise::image;

#[derive(Deserialize)]
struct SeedEquipment {
    slug: String,
    name: String,
    category: String,
    #[serde(default)]
    loadable: bool,
    #[serde(default)]
    weighted: bool,
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

/// A movement uses one implement unless it says otherwise.
fn one() -> i32 {
    1
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
    #[serde(default)]
    skill: bool,
    #[serde(default)]
    warmup: bool,
    /// Relative difficulty 1–5 *within a movement family* (pattern + primary
    /// group) — orders variations so the engine can offer the next-harder one
    /// (G7) and seed a first estimate for a harder sibling. `None` = unrated.
    #[serde(default)]
    difficulty: Option<i32>,
    /// How many of the implement the movement uses — a goblet squat takes one
    /// dumbbell, a dumbbell bench press takes two. Decides how a finite disc
    /// budget is shared out, so it decides which loads are buildable.
    #[serde(default = "one")]
    implements: i32,
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

/// Fingerprint the catalog bundle: every `*.json` in the dir, in filename order,
/// each hashed under its own name. Any edit to any of them changes the digest, so
/// the seed runs — which is the whole point of the gate. Hashing only
/// `exercises.json` (what this used to do) meant an edit to `equipment.json` or
/// the muscle taxonomy left the digest unchanged and was skipped forever.
fn bundle_hash(dir: &Path) -> Result<String> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();

    let mut h = Sha256::new();
    for path in &files {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        // The name is hashed too, so renaming a file is a change like any other.
        h.update(path.file_name().unwrap_or_default().as_encoded_bytes());
        h.update(&bytes);
    }
    Ok(hex::encode(h.finalize()))
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

    // Fingerprint the whole bundle; an unchanged hash means nothing to do.
    let catalog_hash = bundle_hash(dir)?;
    let stored_hash: Option<String> =
        sqlx::query_scalar("SELECT catalog_hash FROM catalog_state WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    if stored_hash.as_deref() == Some(catalog_hash.as_str()) {
        return Ok(());
    }
    let exercises_bytes = std::fs::read(&exercises_path)
        .with_context(|| format!("reading {}", exercises_path.display()))?;

    // Equipment. Every column the catalog carries is reconciled, not just
    // `loadable` — `weighted` decides whether the coach may put a load on this kit
    // at all, so a stale copy of it silently drops lifts from the plan.
    for e in read_json::<Vec<SeedEquipment>>(&dir.join("equipment.json"))? {
        sqlx::query(
            "INSERT INTO equipment (slug, name, category, loadable, weighted) \
             VALUES (?, ?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE name = VALUES(name), category = VALUES(category), \
               loadable = VALUES(loadable), weighted = VALUES(weighted)",
        )
        .bind(&e.slug)
        .bind(&e.name)
        .bind(&e.category)
        .bind(e.loadable)
        .bind(e.weighted)
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

    // Exercises: insert new ones, and for existing ones reconcile the M:N links +
    // *every* scalar the catalog carries — it is the source of truth for all of
    // them. `is_active` is untouched: the retired `*_legacy` rows aren't in the
    // catalog, so this loop never sees them.
    let existing: HashMap<String, i64> = sqlx::query_as("SELECT slug, id FROM exercises")
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
    // Which exercises already carry a picture — so a *newly added* one lands on a
    // row that has been there for months, and an existing one isn't re-read off
    // disk on every catalog change.
    let has_image: std::collections::HashSet<i64> =
        sqlx::query_scalar("SELECT exercise_id FROM exercise_images")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();

    let exercises: Vec<SeedExercise> = serde_json::from_slice(&exercises_bytes)
        .with_context(|| format!("parsing {}", exercises_path.display()))?;
    let mut inserted = 0usize;
    let mut reconciled = 0usize;
    let mut images = 0usize;
    for ex in &exercises {
        let position = ex.position.as_deref().map(|p| p.replace(' ', "_"));
        let id = match existing.get(&ex.slug) {
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
                // Write back every scalar the catalog owns. Same column list as the
                // insert below, so a field added to one can't quietly skip the other.
                sqlx::query(
                    "UPDATE exercises SET \
                       name = ?, variation = ?, pattern = ?, metric = ?, position = ?, \
                       unilateral = ?, skill = ?, warmup = ?, difficulty = ?, implements = ?, \
                       cue = ?, demo_url = ?, summary = ? \
                     WHERE id = ?",
                )
                .bind(&ex.name)
                .bind(&ex.variation)
                .bind(&ex.pattern)
                .bind(&ex.metric)
                .bind(&position)
                .bind(ex.unilateral)
                .bind(ex.skill)
                .bind(ex.warmup)
                .bind(ex.difficulty)
                .bind(ex.implements)
                .bind(&ex.cue)
                .bind(&ex.demo_url)
                .bind(&ex.summary)
                .bind(id)
                .execute(pool)
                .await?;
                reconciled += 1;
                id
            }
            None => {
                let res = sqlx::query(
                    "INSERT INTO exercises \
                       (slug, name, variation, pattern, metric, position, unilateral, skill, warmup, difficulty, implements, cue, demo_url, summary) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&ex.slug)
                .bind(&ex.name)
                .bind(&ex.variation)
                .bind(&ex.pattern)
                .bind(&ex.metric)
                .bind(&position)
                .bind(ex.unilateral)
                .bind(ex.skill)
                .bind(ex.warmup)
                .bind(ex.difficulty)
                .bind(ex.implements)
                .bind(&ex.cue)
                .bind(&ex.demo_url)
                .bind(&ex.summary)
                .execute(pool)
                .await
                .with_context(|| format!("inserting exercise {}", ex.slug))?;
                inserted += 1;
                res.last_insert_id() as i64
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
        // A picture can arrive *after* the movement does — an exercise is catalogued
        // the moment it's real, and the photo turns up when someone takes one. Gating
        // this on `is_new` meant the picture then had nowhere to land: the row already
        // existed, so the seed skipped it forever, and the movement stayed
        // illustrated-by-nothing however many images were added to the bundle.
        //
        // Reading the file only when the row has no image keeps a re-seed from
        // hauling ~15 MB off disk to `INSERT IGNORE` it away.
        if let Some(img) = &ex.image
            && !has_image.contains(&id)
        {
            let path = dir.join("images").join(&img.file);
            let raw =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            // The bundle is the source and keeps its alpha; what goes in the DB is
            // what the app can display. An anatomy diagram (transparent line-art,
            // portrait) is composited onto white and padded to 16:9; a photograph is
            // stored exactly as it came. See seed::render.
            let r = render::render(&raw, &img.content_type, &ex.slug)?;
            let etag = hex::encode(Sha256::digest(&r.bytes));
            image::insert_if_absent(pool, id, &r.content_type, &r.bytes, &etag).await?;
            images += 1;
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

    if inserted > 0 || reconciled > 0 || images > 0 {
        tracing::info!(
            "catalog seed: {inserted} inserted, {reconciled} reconciled, {images} image(s) added"
        );
    }
    Ok(())
}
