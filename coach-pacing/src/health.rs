//! The raw recovery data the readiness model consumes. These are pure data
//! shapes; the std shell (`coach::health`) owns the best-effort HTTP client that
//! fetches them and re-exports these types.

use serde::Deserialize;

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
