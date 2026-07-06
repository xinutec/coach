//! Muscle taxonomy endpoint.

use axum::Json;
use axum::extract::State;

use crate::error::AppError;
use crate::muscle::repo;
use crate::muscle::types::Muscle;
use crate::session::AuthUser;
use crate::state::AppState;

/// GET /api/muscles → the muscle taxonomy (grouped, with regions).
pub async fn list(
    State(app): State<AppState>,
    AuthUser(_user): AuthUser,
) -> Result<Json<Vec<Muscle>>, AppError> {
    Ok(Json(repo::list(&app.pool).await?))
}
