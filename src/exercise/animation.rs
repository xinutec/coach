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
    let row = sqlx::query!(
        "SELECT content_type, bytes, etag FROM exercise_loops WHERE exercise_id = ?",
        exercise_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| LoopBlob {
        content_type: r.content_type,
        bytes: r.bytes,
        etag: r.etag,
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
    sqlx::query!(
        "INSERT INTO exercise_loops (exercise_id, content_type, bytes, byte_size, etag) \
         VALUES (?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE \
           content_type = VALUES(content_type), bytes = VALUES(bytes), \
           byte_size = VALUES(byte_size), etag = VALUES(etag)",
        exercise_id,
        content_type,
        bytes,
        bytes.len() as i32,
        etag
    )
    .execute(pool)
    .await?;
    Ok(())
}
