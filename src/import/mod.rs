//! One-time migration importer: ingest a user's history + programs bundle
//! (exported from the old NocoDB base) into coach, stamping the calling user.
//!
//! Safe to re-run: history is imported only into a fresh log (zero existing
//! sets), and a program is created only if the user has none by that name — so
//! it never duplicates or clobbers real data. Global catalog data is NOT here;
//! that's the boot seeder (`crate::seed`). This bundle is per-user and private,
//! uploaded at run time — never committed.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use ts_rs::TS;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    #[serde(default)]
    pub history: Vec<HistoryRow>,
    #[serde(default)]
    pub programs: Vec<ProgramSpec>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramSpec {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<ProgramEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramEntry {
    pub exercise_slug: Option<String>,
    pub target_reps: Option<i32>,
}

/// What the importer did — reported back so the one-time run is auditable.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportSummary {
    pub history_sets_inserted: i64,
    pub history_skipped_existing: bool,
    pub programs_created: i64,
    pub programs_skipped: i64,
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
    if !history_skipped_existing {
        for row in &bundle.history {
            let Some(exercise_id) = resolve(&row.exercise_slug, &mut unknown) else {
                continue;
            };
            let Ok(date) = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d") else {
                continue;
            };
            let logged_at = date.and_hms_opt(12, 0, 0).expect("noon is valid");
            // NocoDB stored one row per (exercise, day) with a set count; coach
            // logs one row per set — expand.
            for _ in 0..row.sets.max(1) {
                sqlx::query(
                    "INSERT INTO workout_sets \
                       (user_id, exercise_id, program_id, logged_at, reps, load_kg, band) \
                     VALUES (?, ?, NULL, ?, ?, ?, ?)",
                )
                .bind(user_id)
                .bind(exercise_id)
                .bind(logged_at)
                .bind(row.reps)
                .bind(row.weight_kg)
                .bind(&row.band)
                .execute(pool)
                .await?;
                history_sets_inserted += 1;
            }
        }
    }

    // --- programs (create if the user has none by that name) ---
    let start_date = Utc::now().date_naive();
    let mut programs_created = 0i64;
    let mut programs_skipped = 0i64;
    for spec in &bundle.programs {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM programs WHERE user_id = ? AND name = ? AND deleted_at IS NULL",
        )
        .bind(user_id)
        .bind(&spec.name)
        .fetch_one(pool)
        .await?;
        if exists > 0 {
            programs_skipped += 1;
            continue;
        }
        let res = sqlx::query(
            "INSERT INTO programs (user_id, name, start_date, weeks, deload_week, active) \
             VALUES (?, ?, ?, 1, NULL, 0)",
        )
        .bind(user_id)
        .bind(&spec.name)
        .bind(start_date)
        .execute(pool)
        .await?;
        let program_id = res.last_insert_id() as i64;
        for entry in &spec.entries {
            let Some(exercise_id) = resolve(&entry.exercise_slug, &mut unknown) else {
                continue;
            };
            // 3 sets is the coach default; the rep target carries the NocoDB
            // count. UNIQUE(program,exercise,week) → IGNORE a repeated exercise.
            sqlx::query(
                "INSERT IGNORE INTO program_targets \
                   (program_id, exercise_id, week_index, target_sets, rep_low, rep_high) \
                 VALUES (?, ?, 1, 3, ?, ?)",
            )
            .bind(program_id)
            .bind(exercise_id)
            .bind(entry.target_reps)
            .bind(entry.target_reps)
            .execute(pool)
            .await?;
        }
        programs_created += 1;
    }

    Ok(ImportSummary {
        history_sets_inserted,
        history_skipped_existing,
        programs_created,
        programs_skipped,
        unknown_exercises: unknown,
    })
}
