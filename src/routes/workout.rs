//! Workout-set (micro-log) endpoints.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::error::AppError;
use crate::exercise::repo as exercise_repo;
use crate::location::owned;
use crate::session::AuthUser;
use crate::state::AppState;
use crate::workout::repo;
use crate::workout::types::{NewSet, WorkoutSet};

/// POST /api/sets → log a set.
///
/// The body is checked against the exercise's metric before it's stored: a
/// bodyweight drill can't carry a load, a hold can't carry reps. The round-2
/// field test logged "10 reps · 4 kg" mobility drills because the client kept a
/// stale hidden field — the client is fixed too, but data this wrong must not
/// be one bug away from the ability model.
pub async fn create(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<NewSet>,
) -> Result<Json<WorkoutSet>, AppError> {
    let ex = exercise_repo::get(&app.pool, body.exercise_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if let Some(msg) = body.shape_error(ex.metric) {
        return Err(AppError::BadRequest(msg.to_string()));
    }
    // A load far past anything he owns is the signature of a mistyped field, and
    // ability is a max over history — one such number becomes a PR the engine
    // cannot unlearn, and every future session is prescribed off it. So ask
    // once. A confirm rather than a refusal, because improvised weights are real
    // and the ledger already judges them honestly.
    if let Some(load) = body.load_kg
        && !body.confirm_load.unwrap_or(false)
    {
        let heaviest = owned::heaviest_buildable(&app.pool, &user.user_id, &ex).await?;
        if owned::implausible(load, heaviest) {
            let owned_kg = heaviest.unwrap_or_default();
            return Err(AppError::NeedsConfirmation(format!(
                "{load} kg is well past the heaviest you own for {} ({owned_kg} kg) \
                 — confirm if that's right",
                ex.name
            )));
        }
    }
    Ok(Json(repo::create(&app.pool, &user.user_id, &body).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentQuery {
    pub limit: Option<i64>,
}

/// GET /api/sets → most-recent sets first (limit default 50, max 500).
pub async fn list(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Query(q): Query<RecentQuery>,
) -> Result<Json<Vec<WorkoutSet>>, AppError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    Ok(Json(
        repo::list_recent(&app.pool, &user.user_id, limit).await?,
    ))
}

/// DELETE /api/sets/{id} → soft-delete a logged set.
pub async fn delete(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    if repo::soft_delete(&app.pool, &user.user_id, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}
