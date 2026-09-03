//! Generated 3D demo loops, stored in-DB as blobs beside the photographs rather
//! than instead of them (see migrations/0027). Served with an ETag, like images.
//!
//! Named `animation` because `loop` is a Rust keyword.

use anyhow::Result;
use sqlx::MySqlPool;

pub struct LoopBlob {
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub etag: String,
}

pub async fn get(pool: &MySqlPool, exercise_id: i64) -> Result<Option<LoopBlob>> {
    let row: Option<(String, Vec<u8>, String)> = sqlx::query_as(
        "SELECT content_type, bytes, etag FROM exercise_loops WHERE exercise_id = ?",
    )
    .bind(exercise_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(content_type, bytes, etag)| LoopBlob {
        content_type,
        bytes,
        etag,
    }))
}

/// Seed an exercise's loop, replacing whatever is there. Replacing and not
/// ignoring, for the reason image::upsert records: an `INSERT IGNORE` makes the
/// first artifact an exercise ever received permanent, so a re-render can never
/// land.
pub async fn upsert(
    pool: &MySqlPool,
    exercise_id: i64,
    content_type: &str,
    bytes: &[u8],
    etag: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO exercise_loops (exercise_id, content_type, bytes, byte_size, etag) \
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
