//! Training-location endpoints (per-user: home, office gym, …).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::error::AppError;
use crate::location::repo;
use crate::location::types::{Location, LocationPatch, NewLocation};
use crate::session::AuthUser;
use crate::state::AppState;

/// GET /api/locations → the user's locations (default first).
pub async fn list(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<Location>>, AppError> {
    Ok(Json(repo::list(&app.pool, &user.user_id).await?))
}

/// POST /api/locations → create a location.
pub async fn create(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<NewLocation>,
) -> Result<Json<Location>, AppError> {
    Ok(Json(repo::create(&app.pool, &user.user_id, &body).await?))
}

/// PATCH /api/locations/{id} → rename, set default, or replace the equipment set.
pub async fn patch(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<LocationPatch>,
) -> Result<Json<Location>, AppError> {
    repo::patch(&app.pool, &user.user_id, id, &body)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

/// DELETE /api/locations/{id} → remove a location (soft delete).
pub async fn delete(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    if repo::delete(&app.pool, &user.user_id, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}
