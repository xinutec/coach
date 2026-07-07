//! One-time migration import endpoint (per-user training history).

use axum::Json;
use axum::extract::State;

use crate::error::AppError;
use crate::import::{Bundle, ImportSummary};
use crate::session::AuthUser;
use crate::state::AppState;

/// POST /api/import/nocodb → ingest the caller's training-history bundle.
/// Idempotent (skips a non-empty log); returns a summary of what it did.
pub async fn nocodb(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Json(bundle): Json<Bundle>,
) -> Result<Json<ImportSummary>, AppError> {
    Ok(Json(
        crate::import::nocodb(&app.pool, &user.user_id, bundle).await?,
    ))
}
