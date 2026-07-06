//! Equipment catalog endpoint.

use axum::Json;
use axum::extract::State;

use crate::equipment::repo;
use crate::equipment::types::Equipment;
use crate::error::AppError;
use crate::session::AuthUser;
use crate::state::AppState;

/// GET /api/equipment → the kit vocabulary (for the locations + library UIs).
pub async fn list(
    State(app): State<AppState>,
    AuthUser(_user): AuthUser,
) -> Result<Json<Vec<Equipment>>, AppError> {
    Ok(Json(repo::list(&app.pool).await?))
}
