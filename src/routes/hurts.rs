//! "This hurts" on a card, and taking it back.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use coach_pacing::domain::ExerciseId;

use crate::error::AppError;
use crate::exercise::repo as ex_repo;
use crate::pacing::engine::HURT_REST_DAYS;
use crate::pacing::hurts;
use crate::session::AuthUser;
use crate::state::AppState;

/// POST /api/exercises/{id}/hurts → the movement rests, and the next verdict says so.
pub async fn report(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    if ex_repo::get(&app.pool, id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    hurts::record(
        &app.pool,
        &user.user_id,
        ExerciseId(id),
        Utc::now().naive_utc(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/exercises/{id}/hurts → take back the reports still resting it.
pub async fn take_back(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let from = Utc::now().naive_utc() - Duration::days(HURT_REST_DAYS);
    hurts::take_back(&app.pool, &user.user_id, ExerciseId(id), from).await?;
    Ok(StatusCode::NO_CONTENT)
}
