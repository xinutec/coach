//! One-time migration importer: ingest a user's training history bundle
//! (exported from the old NocoDB base) into coach, stamping the calling user.
//!
//! Safe to re-run: history is imported only into a fresh log (zero existing
//! sets), so it never duplicates or clobbers real data. Global catalog data is
//! NOT here; that's the boot seeder (`crate::seed`). This bundle is per-user and
//! private, uploaded at run time — never committed.

use std::collections::HashMap;

use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    #[serde(default)]
    pub history: Vec<HistoryRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub date: String,
    pub exercise_slug: Option<String>,
    #[serde(default = "one")]
    pub sets: i32,
    pub reps: Option<i32>,
    pub weight_kg: Option<f64>,
    pub band: Option<String>,
}

fn one() -> i32 {
    1
}

/// What the importer did — reported back so the one-time run is auditable.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ImportSummary {
    pub history_sets_inserted: i64,
    pub history_skipped_existing: bool,
    /// Bundle rows carrying no measurement at all — see the skip in `nocodb`.
    /// Reported rather than silently dropped: a bundle that loses rows here is a
    /// fact about the export, and the one-time run has to be auditable.
    pub history_skipped_shapeless: i64,
    /// Bundle slugs that don't resolve to a catalog exercise (should be empty).
    pub unknown_exercises: Vec<String>,
}

pub async fn nocodb(pool: &MySqlPool, user_id: &str, bundle: Bundle) -> Result<ImportSummary> {
    let slug_to_id: HashMap<String, i64> = sqlx::query_as("SELECT slug, id FROM exercises")
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
    let mut unknown = Vec::new();
    let resolve = |slug: &Option<String>, unknown: &mut Vec<String>| -> Option<i64> {
        let s = slug.as_deref()?;
        match slug_to_id.get(s) {
            Some(id) => Some(*id),
            None => {
                if !unknown.contains(&s.to_string()) {
                    unknown.push(s.to_string());
                }
                None
            }
        }
    };

    // --- history (only into a fresh log) ---
    let existing_sets: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workout_sets WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    let history_skipped_existing = existing_sets > 0;
    let mut history_sets_inserted = 0i64;
    let mut history_skipped_shapeless = 0i64;
    if !history_skipped_existing {
        for row in &bundle.history {
            let Some(exercise_id) = resolve(&row.exercise_slug, &mut unknown) else {
                continue;
            };
            let Ok(date) = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d") else {
                continue;
            };
            let logged_at = date.and_hms_opt(12, 0, 0).expect("noon is valid");
            // NOTE: the one write path that does not go through
            // `NewSet::validate` / `workout::repo::create`, because it needs the
            // `band` column the API has no field for. It stores NocoDB's
            // `reps`/`weight_kg` against whatever the exercise's metric is, and
            // 65 of the 357 sets now in the log are the result — reps recorded
            // against holds, loads against bodyweight drills.
            //
            // Deliberately left as it is. Parsing here would *reject* those rows
            // rather than store them, and that trades a wrong shape for missing
            // history: NocoDB's "reps" for a hold movement is most likely seconds
            // under the wrong heading, so the fix is to map the column, not to
            // drop the row. Which it is per exercise is a question about the
            // source data, not about this type.
            //
            // Two coercions the schema now requires (0026), and the parser this
            // path skips would have made anyway.
            //
            // A row with no count carries no measurement, and a set with no
            // measurement records that something happened and nothing about
            // what. It is skipped rather than aborting the run: the bundle is a
            // fixed historical artefact, and one shapeless row in it is not a
            // reason to lose the other three hundred.
            let Some(reps) = row.reps else {
                history_skipped_shapeless += i64::from(row.sets.max(1));
                continue;
            };
            // NocoDB wrote an unweighted set as 0 kg. Zero is not a weight, it
            // is the absence of one, and the domain spells that `None`.
            let load_kg = row.weight_kg.filter(|kg| *kg > 0.0);
            // NocoDB stored one row per (exercise, day) with a set count; coach
            // logs one row per set — expand.
            for _ in 0..row.sets.max(1) {
                sqlx::query(
                    "INSERT INTO workout_sets \
                       (user_id, exercise_id, logged_at, reps, load_kg, band) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(user_id)
                .bind(exercise_id)
                .bind(logged_at)
                .bind(reps)
                .bind(load_kg)
                .bind(&row.band)
                .execute(pool)
                .await?;
                history_sets_inserted += 1;
            }
        }
    }

    Ok(ImportSummary {
        history_sets_inserted,
        history_skipped_existing,
        history_skipped_shapeless,
        unknown_exercises: unknown,
    })
}
