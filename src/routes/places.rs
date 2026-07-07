//! Detected-place + current-location endpoints. These bridge to health-sync
//! (via `crate::health`) and degrade to empty/none when the integration is
//! unconfigured or health is unreachable — the location feature stays fully
//! usable manually.

use axum::Json;
use axum::extract::State;

use crate::error::AppError;
use crate::health::{self, DetectedPlace};
use crate::location::repo as location_repo;
use crate::location::types::CurrentLocation;
use crate::session::AuthUser;
use crate::state::AppState;

/// GET /api/places/detected → the user's health-detected places, for the
/// location-link picker. Empty when the integration is off / health is down.
pub async fn detected(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<DetectedPlace>>, AppError> {
    let Some((base, token)) = app.cfg.health() else {
        return Ok(Json(Vec::new()));
    };
    // Only offer named places: health surfaces unnamed dwell clusters all
    // labelled "Stay", which are indistinguishable (and unlinkable) in a picker.
    let places = health::places(&app.http, base, token, &user.user_id)
        .await
        .into_iter()
        .filter(DetectedPlace::is_named)
        .collect();
    Ok(Json(places))
}

/// GET /api/location/current → the location the user is currently at (their
/// linked location for health's current place), or `{ locationId: null }`.
pub async fn current(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<CurrentLocation>, AppError> {
    let location_id = resolve_current(&app, &user.user_id).await;
    Ok(Json(CurrentLocation { location_id }))
}

/// Best-effort: current health place → the user's location linked to it. Any
/// missing piece (integration off, health down, no fix, no linked location)
/// yields `None`.
async fn resolve_current(app: &AppState, user_id: &str) -> Option<i64> {
    let (base, token) = app.cfg.health()?;
    let place = health::current_place(&app.http, base, token, user_id).await?;
    location_repo::by_health_place(&app.pool, user_id, place.id)
        .await
        .ok()
        .flatten()
}
