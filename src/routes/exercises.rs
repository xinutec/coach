//! Exercise catalog endpoints.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::error::AppError;
use crate::exercise::image;
use crate::exercise::repo;
use crate::exercise::types::{Exercise, ExerciseDetail, ExercisePatch, NewExercise};
use crate::session::AuthUser;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    #[serde(default)]
    pub include_inactive: bool,
}

/// GET /api/exercises → the catalog (active only unless includeInactive=true).
pub async fn list(
    State(app): State<AppState>,
    AuthUser(_user): AuthUser,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Exercise>>, AppError> {
    Ok(Json(repo::list(&app.pool, q.include_inactive).await?))
}

/// GET /api/exercises/{id} → full detail (muscles, equipment, media).
pub async fn detail(
    State(app): State<AppState>,
    AuthUser(_user): AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<ExerciseDetail>, AppError> {
    repo::detail(&app.pool, id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

/// GET /api/exercises/{id}/image → the demo image blob (immutable; ETag-cached).
pub async fn image(
    State(app): State<AppState>,
    AuthUser(_user): AuthUser,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(img) = image::get(&app.pool, id).await? else {
        return Err(AppError::NotFound);
    };
    let etag = format!("\"{}\"", img.etag);
    // Content is immutable per exercise-image; a matching ETag → 304.
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == etag)
    {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response());
    }
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, img.content_type),
            (header::ETAG, etag),
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        Body::from(img.bytes),
    )
        .into_response())
}

/// POST /api/exercises → add a custom movement.
pub async fn create(
    State(app): State<AppState>,
    AuthUser(_user): AuthUser,
    Json(body): Json<NewExercise>,
) -> Result<Json<ExerciseDetail>, AppError> {
    Ok(Json(repo::create(&app.pool, &body).await?))
}

/// PATCH /api/exercises/{id} → edit / (de)activate a movement.
pub async fn patch(
    State(app): State<AppState>,
    AuthUser(_user): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<ExercisePatch>,
) -> Result<Json<ExerciseDetail>, AppError> {
    repo::patch(&app.pool, id, &body)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}
