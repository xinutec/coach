//! Thin, best-effort client for health-sync's internal place API. Coach uses it
//! to (a) list the user's detected places for the location-link picker and
//! (b) find which place the user is in right now, to auto-select a location.
//!
//! Every call is best-effort: on any error, timeout, or misconfiguration it
//! returns empty/`None` and logs at debug — a coach request must never fail
//! because health is slow or down. The feature is off entirely unless both
//! `HEALTH_INTERNAL_URL` and `HEALTH_SERVICE_TOKEN` are set (see `Config::health`).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// health's fallback label for a dwell cluster it couldn't resolve to a named
/// place (health `src/sleep/known-place-stays.ts`: "Falls back to 'Stay'").
/// Several unnamed stays all carry this identical text, so they're
/// indistinguishable in a link picker — coach hides them (see [`DetectedPlace::is_named`]).
pub const UNNAMED_PLACE_LABEL: &str = "Stay";

/// A place health has detected for the user, surfaced to the link picker.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DetectedPlace {
    #[ts(type = "number")]
    pub id: i64,
    pub label: String,
    pub amenity_label: Option<String>,
    #[ts(type = "number | null")]
    pub last_seen_ts: Option<i64>,
}

impl DetectedPlace {
    /// True when health resolved this stay to a real, named place. Unnamed
    /// stays (all labelled [`UNNAMED_PLACE_LABEL`]) can't be told apart, so
    /// they're not worth offering in a picker.
    pub fn is_named(&self) -> bool {
        self.label != UNNAMED_PLACE_LABEL
    }
}

/// The place the user is currently in (health's `/internal/place/current`).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentPlace {
    pub id: i64,
}

/// Latest value + trailing baseline for one biometric (health's raw stats).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub latest: f64,
    pub mean: f64,
    pub sd: f64,
    pub n: i64,
}

/// Raw recovery data from health (`/internal/recovery`) — coach turns this into a
/// readiness score itself (health stays unopinionated).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub sleep_hours: Option<f64>,
    pub hrv: Option<Stat>,
    pub resting_hr: Option<Stat>,
}

const TIMEOUT: Duration = Duration::from_secs(3);
const HEADER: &str = "X-Service-Token";

/// List the user's detected places. Empty on any failure.
pub async fn places(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    user: &str,
) -> Vec<DetectedPlace> {
    match get::<Vec<DetectedPlace>>(http, base, "/internal/places", token, user).await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("health places lookup failed: {e:#}");
            Vec::new()
        }
    }
}

/// The place the user is currently in, or `None` (also on any failure).
pub async fn current_place(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    user: &str,
) -> Option<CurrentPlace> {
    match get::<Option<CurrentPlace>>(http, base, "/internal/place/current", token, user).await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("health current-place lookup failed: {e:#}");
            None
        }
    }
}

/// The user's raw recovery data, or `None` (also on any failure).
pub async fn recovery(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    user: &str,
) -> Option<Recovery> {
    match get::<Recovery>(http, base, "/internal/recovery", token, user).await {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::debug!("health recovery lookup failed: {e:#}");
            None
        }
    }
}

async fn get<T: for<'de> Deserialize<'de>>(
    http: &reqwest::Client,
    base: &str,
    path: &str,
    token: &str,
    user: &str,
) -> anyhow::Result<T> {
    let mut url = url::Url::parse(&format!("{base}{path}"))?;
    url.query_pairs_mut().append_pair("user", user);
    let resp = http
        .get(url)
        .header(HEADER, token)
        .timeout(TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<T>().await?)
}
