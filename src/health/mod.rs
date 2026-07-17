//! Thin, best-effort client for health-sync's internal place API. Coach uses it
//! to (a) list the user's detected places for the location-link picker and
//! (b) find which place the user is in right now, to auto-select a location.
//!
//! Every call is best-effort: on any error, timeout, or misconfiguration it
//! returns empty/`None` and logs at debug — a coach request must never fail
//! because health is slow or down. The feature is off entirely unless both
//! `HEALTH_INTERNAL_URL` and `HEALTH_SERVICE_TOKEN` are set (see `Config::health`).

use std::time::Duration;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Venue categories (health's coarse OSM class) that aren't training places, so
/// coach hides them from the link picker. Everything else — leisure (parks,
/// gyms), lodging, unclassified (home/work/named) — is kept.
const NON_TRAINING_CATEGORIES: &[&str] = &["food", "errand", "transport"];

/// A place health has detected for the user, surfaced to the link picker.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DetectedPlace {
    #[ts(type = "number")]
    pub id: i64,
    pub label: String,
    pub amenity_label: Option<String>,
    /// Whether health considers this a recognisable place (a specific Home/Work
    /// or a mined venue name — not a bare, indistinguishable "Stay"). Defaults
    /// true so an older health that omits the field fails open.
    #[serde(default = "default_true")]
    pub named: bool,
    /// health's coarse venue class ("food", "leisure", "errand", …), or None
    /// when unmined. Coach uses it to drop clearly non-training venues.
    #[serde(default)]
    pub category: Option<String>,
    #[ts(type = "number | null")]
    pub last_seen_ts: Option<i64>,
}

fn default_true() -> bool {
    true
}

impl DetectedPlace {
    /// Worth offering as a training location: recognisable in a picker, and not
    /// a clearly non-training venue (a restaurant, a shop, a transport hub).
    pub fn is_trainable(&self) -> bool {
        self.named
            && !self
                .category
                .as_deref()
                .is_some_and(|c| NON_TRAINING_CATEGORIES.contains(&c))
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

/// The same raw recovery, but *as of* a named past day
/// (`/internal/recovery/history`). The prediction-error ledger needs it: the coach
/// eases the ask on an under-recovered morning, and judging that session as though
/// it had been full-effort records the athlete's compliance as a failure.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayRecovery {
    pub as_of: NaiveDate,
    #[serde(flatten)]
    pub recovery: Recovery,
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

/// The user's raw recovery as of each day in `from..=to`, oldest first. Empty on
/// any failure — the ledger then judges those days as full-effort, which is what it
/// did before health could answer the question at all.
pub async fn recovery_history(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    user: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Vec<DayRecovery> {
    let path = format!("/internal/recovery/history?from={from}&to={to}");
    match get::<Vec<DayRecovery>>(http, base, &path, token, user).await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("health recovery history lookup failed: {e:#}");
            Vec::new()
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
