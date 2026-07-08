//! Pacing endpoint. The Android nudge and the Today view both read this.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;

use crate::error::AppError;
use crate::health;
use crate::pacing::types::{PacingNow, Readiness};
use crate::pacing::{readiness, service};
use crate::session::AuthUser;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowQuery {
    /// Optional location to make the suggestion location-aware.
    pub location_id: Option<i64>,
}

/// GET /api/pacing/now[?locationId=] → the current coach verdict (what to do,
/// whether to nudge); location-aware. The mode comes from settings — the coach's
/// standing brief, not a per-call choice.
pub async fn now(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Query(q): Query<NowQuery>,
) -> Result<Json<PacingNow>, AppError> {
    let readiness = fetch_readiness(&app, &user.user_id).await;
    Ok(Json(
        service::now(&app.pool, &user.user_id, q.location_id, readiness).await?,
    ))
}

/// Best-effort biometric readiness from health. Any missing piece (integration
/// off, health down, no usable biometrics) yields `None` and the engine falls
/// back to its volume heuristic.
async fn fetch_readiness(app: &AppState, user_id: &str) -> Option<Readiness> {
    let (base, token) = app.cfg.health()?;
    let recovery = health::recovery(&app.http, base, token, user_id).await?;
    readiness::readiness(&recovery)
}
