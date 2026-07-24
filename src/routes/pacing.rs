//! Pacing endpoint. The Android nudge and the Today view both read this.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use chrono::{Duration, NaiveDate, Utc};
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
    let readiness_history = fetch_readiness_history(&app, &user.user_id).await;
    Ok(Json(
        service::now(
            &app.pool,
            &user.user_id,
            q.location_id,
            readiness,
            readiness_history,
        )
        .await?,
    ))
}

/// How far back the ledger can be asked to reconstruct a morning. Comfortably past
/// the windows the ledger's own judgments turn on (a plateau looks back a month;
/// confidence, six weeks), without asking health for a year of days it would only
/// answer `null` to.
const READINESS_HISTORY_DAYS: i64 = 120;

/// Best-effort biometric readiness from health. Any missing piece (integration
/// off, health down, no usable biometrics) yields `None` and the engine falls
/// back to its volume heuristic.
async fn fetch_readiness(app: &AppState, user_id: &str) -> Option<Readiness> {
    let (base, token) = app.cfg.health()?;
    let recovery = health::recovery(&app.http, base, token, user_id).await?;
    readiness::readiness(&recovery)
}

/// What the coach knew about the athlete's recovery on each recent morning, so the
/// prediction-error ledger can judge a past session against the ask it actually
/// made rather than against a full-effort one it didn't (see
/// [`crate::pacing::types::PacingInput::readiness_history`]). Coach composes the
/// score itself from health's raw streams here, exactly as it does for today —
/// health stays unopinionated, and there is one definition of readiness.
///
/// Best-effort throughout: an empty map means every day is judged full-effort,
/// which is what the ledger did before it could ask.
async fn fetch_readiness_history(app: &AppState, user_id: &str) -> BTreeMap<NaiveDate, Readiness> {
    let Some((base, token)) = app.cfg.health() else {
        return BTreeMap::new();
    };
    let to: NaiveDate = Utc::now().date_naive();
    let from = to - Duration::days(READINESS_HISTORY_DAYS);
    health::recovery_history(&app.http, base, token, user_id, from, to)
        .await
        .into_iter()
        .filter_map(|d| readiness::readiness(&d.recovery).map(|r| (d.as_of, r)))
        .collect()
}
