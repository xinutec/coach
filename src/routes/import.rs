//! One-time migration import endpoint (per-user history + programs).

use axum::Json;
use axum::extract::State;

use crate::error::AppError;
use crate::import::{Bundle, ImportSummary};
use crate::session::AuthUser;
use crate::state::AppState;

/// POST /api/import/nocodb → ingest the caller's history + programs bundle.
/// Idempotent (skips a non-empty log / existing program names); returns a
/// summary of what it did.
pub async fn nocodb(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Json(bundle): Json<Bundle>,
) -> Result<Json<ImportSummary>, AppError> {
    Ok(Json(
        crate::import::nocodb(&app.pool, &user.user_id, bundle).await?,
    ))
}
