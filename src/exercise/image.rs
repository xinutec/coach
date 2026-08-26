//! Exercise demo images, stored in-DB as blobs (no file storage). Served by the
//! image route with an ETag for cheap client caching.

use anyhow::Result;
use sqlx::MySqlPool;

pub struct ImageBlob {
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub etag: String,
}

pub async fn get(pool: &MySqlPool, exercise_id: i64) -> Result<Option<ImageBlob>> {
    let row: Option<(String, Vec<u8>, String)> = sqlx::query_as(
        "SELECT content_type, bytes, etag FROM exercise_images WHERE exercise_id = ?",
    )
    .bind(exercise_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(content_type, bytes, etag)| ImageBlob {
        content_type,
        bytes,
        etag,
    }))
}

/// Seed an exercise's image, replacing whatever is there. Idempotent, and the
/// caller only reaches it when the bytes differ — but it must *replace*, not
/// ignore: this was `INSERT IGNORE`, which made the first picture an exercise ever
/// received permanent, so a corrected render could never reach the app.
pub async fn upsert(
    pool: &MySqlPool,
    exercise_id: i64,
    content_type: &str,
    bytes: &[u8],
    etag: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO exercise_images (exercise_id, content_type, bytes, byte_size, etag) \
         VALUES (?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE \
           content_type = VALUES(content_type), bytes = VALUES(bytes), \
           byte_size = VALUES(byte_size), etag = VALUES(etag)",
    )
    .bind(exercise_id)
    .bind(content_type)
    .bind(bytes)
    .bind(bytes.len() as i32)
    .bind(etag)
    .execute(pool)
    .await?;
    Ok(())
}
