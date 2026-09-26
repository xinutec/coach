//! Boot-time catalog seeder: loads the training library (equipment, muscle taxonomy,
//! exercises and their links, image and loop blobs) from `data/catalog/` into the DB,
//! so any fresh DB reproduces it.
//!
//! Hash-gated: the whole bundle (`bundle_hash`) is fingerprinted into `catalog_state`;
//! unchanged means nothing to do, changed means re-seed and **reconcile**. The catalog
//! owns every scalar it carries and the reconcile writes all of them back: a field it
//! skipped would only *appear* catalog-owned. `is_active` is the exception, since the
//! retired `*_legacy` rows are absent from the catalog.
//!
//! Pictures are **rendered** on the way in ([`render`]): the bundle keeps the source,
//! the DB gets what the app can display.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{Connection, MySqlConnection, MySqlPool};

pub mod render;

use crate::exercise::animation;
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
    /// What the picture's licence asks to be credited with, if anything.
    #[serde(default)]
    credit: Option<SeedCredit>,
}

#[derive(Deserialize)]
struct SeedCredit {
    text: String,
    url: Option<String>,
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
    /// Maximal-intent ballistic work (jumps, throws, Olympic lifts, plyo) — the
    /// engine orders it first, before strength compounds, so it's measured/trained
    /// fresh. `false` for everything the flag isn't explicitly authored on.
    #[serde(default)]
    power: bool,
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

/// Fingerprint the catalog bundle: every `*.json` in the dir **and every file in
/// `images/` and `loops/`**, in path order, each hashed under its own name. Any edit to any of
/// them changes the digest, so the seed runs — which is the whole point of the
/// gate. A file outside the digest is an edit skipped forever, and for an image
/// the skip is silent: the app keeps serving the old picture.
fn bundle_hash(dir: &Path) -> Result<String> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    for sub in ["images", "loops"] {
        let sub_dir = dir.join(sub);
        if sub_dir.is_dir() {
            files.extend(
                std::fs::read_dir(&sub_dir)
                    .with_context(|| format!("reading {}", sub_dir.display()))?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.is_file()),
            );
        }
    }
    files.sort();

    let mut h = Sha256::new();
    for path in &files {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        // The path relative to the bundle is hashed too, so renaming or moving a
        // file is a change like any other. Relative, not absolute, or the digest
        // would depend on where the checkout lives.
        let rel = path.strip_prefix(dir).unwrap_or(path);
        h.update(rel.as_os_str().as_encoded_bytes());
        h.update(&bytes);
    }
    Ok(hex::encode(h.finalize()))
}

/// Re-point one exercise's links at the catalog's, in one transaction: a failure
/// between the delete and the insert would leave a plausible subset of its equipment
/// and muscles, silently narrowing what the trainer offers. New rows take the same
/// path. It uses one connection for the whole pass, since a connection per exercise
/// starves the pool when seeds run concurrently, as the tests do.
async fn relink(conn: &mut MySqlConnection, ex: &SeedExercise, id: i64) -> Result<()> {
    let mut tx = conn.begin().await?;
    sqlx::query("DELETE FROM exercise_equipment WHERE exercise_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM exercise_muscle WHERE exercise_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for slug in &ex.equipment {
        sqlx::query(
            "INSERT IGNORE INTO exercise_equipment (exercise_id, equipment_id) \
             SELECT ?, id FROM equipment WHERE slug = ?",
        )
        .bind(id)
        .bind(slug)
        .execute(&mut *tx)
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
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
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
    // What each exercise currently serves, by the etag of those bytes. Presence alone
    // would make the first file permanent; the etag lets a new or re-rendered one land,
    // while an unchanged bundle writes nothing.
    let loop_etag: HashMap<i64, String> =
        sqlx::query_as("SELECT exercise_id, etag FROM exercise_loops")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();
    let image_etag: HashMap<i64, String> =
        sqlx::query_as("SELECT exercise_id, etag FROM exercise_images")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();

    let exercises: Vec<SeedExercise> = serde_json::from_slice(&exercises_bytes)
        .with_context(|| format!("parsing {}", exercises_path.display()))?;
    let mut inserted = 0usize;
    let mut reconciled = 0usize;
    let mut images = 0usize;
    let mut loops = 0usize;
    // One connection held for the whole catalog pass — see `relink`.
    let mut link_conn = pool.acquire().await?;
    for ex in &exercises {
        let position = ex.position.as_deref().map(|p| p.replace(' ', "_"));
        let credit = ex.image.as_ref().and_then(|i| i.credit.as_ref());
        let credit_text = credit.map(|c| c.text.as_str());
        let credit_url = credit.and_then(|c| c.url.as_deref());
        let id = match existing.get(&ex.slug) {
            Some(&id) => {
                // Write back every scalar the catalog owns. Same column list as the
                // insert below, so a field added to one can't quietly skip the other.
                sqlx::query(
                    "UPDATE exercises SET \
                       name = ?, variation = ?, pattern = ?, metric = ?, position = ?, \
                       unilateral = ?, skill = ?, warmup = ?, power = ?, difficulty = ?, implements = ?, \
                       cue = ?, demo_url = ?, summary = ?, image_credit = ?, image_credit_url = ? \
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
                .bind(ex.power)
                .bind(ex.difficulty)
                .bind(ex.implements)
                .bind(&ex.cue)
                .bind(&ex.demo_url)
                .bind(&ex.summary)
                .bind(credit_text)
                .bind(credit_url)
                .bind(id)
                .execute(pool)
                .await?;
                reconciled += 1;
                id
            }
            None => {
                let res = sqlx::query(
                    "INSERT INTO exercises \
                       (slug, name, variation, pattern, metric, position, unilateral, skill, warmup, power, difficulty, implements, cue, demo_url, summary, image_credit, image_credit_url) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
                .bind(ex.power)
                .bind(ex.difficulty)
                .bind(ex.implements)
                .bind(&ex.cue)
                .bind(&ex.demo_url)
                .bind(&ex.summary)
                .bind(credit_text)
                .bind(credit_url)
                .execute(pool)
                .await
                .with_context(|| format!("inserting exercise {}", ex.slug))?;
                inserted += 1;
                crate::db::inserted_id(&res)?
            }
        };

        relink(&mut link_conn, ex, id).await?;
        // Not gated on the row being new: a picture can arrive after its movement.
        // Every picture is read and rendered, but only when the digest moved.
        if let Some(img) = &ex.image {
            let path = dir.join("images").join(&img.file);
            let raw =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            // The bundle is the source and keeps its alpha; what goes in the DB is
            // what the app can display. An anatomy diagram (transparent line-art,
            // portrait) is composited onto white and padded to 16:9; a photograph is
            // stored exactly as it came. See seed::render.
            let r = render::render(&raw, &img.content_type, &ex.slug)?;
            let etag = hex::encode(Sha256::digest(&r.bytes));
            if image_etag.get(&id) != Some(&etag) {
                image::upsert(pool, id, &r.content_type, &r.bytes, &etag).await?;
                images += 1;
            }
        }

        // A loop is found by convention rather than declared in the catalog
        // JSON: data/catalog/loops/<slug>.mp4 if it exists. It sits BESIDE the
        // photograph — nothing here touches exercise_images — so a loop that
        // renders badly cannot cost an exercise the picture it already had.
        let clip = dir.join("loops").join(format!("{}.mp4", ex.slug));
        if clip.is_file() {
            let bytes =
                std::fs::read(&clip).with_context(|| format!("reading {}", clip.display()))?;
            let etag = hex::encode(Sha256::digest(&bytes));
            if loop_etag.get(&id) != Some(&etag) {
                animation::upsert(pool, id, "video/mp4", &bytes, &etag).await?;
                loops += 1;
            }
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

    if inserted > 0 || reconciled > 0 || images > 0 || loops > 0 {
        tracing::info!(
            "catalog seed: {inserted} inserted, {reconciled} reconciled, \
             {images} image(s) and {loops} loop(s) added"
        );
    }
    Ok(())
}
