//! Pacing endpoint. The Android nudge and the Today view both read this.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;

use crate::error::AppError;
use crate::pacing::service;
use crate::pacing::types::PacingNow;
use crate::session::AuthUser;
use crate::settings::types::Mode;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowQuery {
    /// Optional location to make the suggestion location-aware.
    pub location_id: Option<i64>,
    /// Optional mode for this call (else the user's saved default).
    pub mode: Option<Mode>,
}

/// GET /api/pacing/now[?locationId=&mode=] → the current coach verdict (what to
/// do, whether to nudge); location- and mode-aware.
pub async fn now(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Query(q): Query<NowQuery>,
) -> Result<Json<PacingNow>, AppError> {
    Ok(Json(
        service::now(&app.pool, &user.user_id, q.location_id, q.mode).await?,
    ))
}
