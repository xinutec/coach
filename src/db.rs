//! MariaDB connection pool. coach's own database — NC is never written to.

use anyhow::{Context, Result};
use sqlx::MySqlPool;
use sqlx::mysql::MySqlPoolOptions;

pub async fn connect(database_url: &str) -> Result<MySqlPool> {
    let pool = MySqlPoolOptions::new()
        .max_connections(8)
        // Pin every connection's session zone to UTC. Columns written by the DB
        // clock (`NOW()`, `DEFAULT CURRENT_TIMESTAMP`) are read back with
        // `.and_utc()`, which asserts UTC — so the session zone MUST be UTC or
        // those instants drift by the server's offset. Without this the code is
        // correct only because the container happens to run UTC; here it's
        // correct by construction, matching the Rust-side `.naive_utc()` writes.
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("SET time_zone = '+00:00'")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await
        .context("connecting to MariaDB")?;
    Ok(pool)
}

/// The id MariaDB assigned to the row just inserted, as the `i64` the BIGINT keys
/// use. An id past `i64::MAX` is not one this schema can hold.
pub fn inserted_id(res: &sqlx::mysql::MySqlQueryResult) -> anyhow::Result<i64> {
    use anyhow::Context as _;
    i64::try_from(res.last_insert_id()).context("insert id past i64::MAX")
}

/// Apply embedded migrations from `migrations/`. Idempotent; safe on every boot.
pub async fn migrate(pool: &MySqlPool) -> Result<()> {
    sqlx::migrate!()
        .run(pool)
        .await
        .context("running migrations")?;
    Ok(())
}
