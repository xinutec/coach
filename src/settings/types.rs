//! Per-user coach settings: the nudge window + the dynamic-engine knobs (mode +
//! light dials). Enum columns follow the coach convention — read as `String`
//! into `SettingsRow`, converted to typed enums (sqlx won't decode MySQL ENUM).

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::muscle::types::Region;

macro_rules! db_str {
    ($name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        impl $name {
            pub fn as_db(self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }
            pub fn from_db(s: &str) -> Option<Self> {
                match s { $($s => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

/// The high-level training intent the engine optimises for — "what am I aiming
/// for right now", switchable per session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Mode {
    /// Even coverage — keep every muscle group progressing (default).
    #[default]
    Balanced,
    /// Bias the big compound-lift groups, heavier + lower-rep.
    Strength,
    /// Bias the ring/hold/calisthenic work; progress by harder variation.
    Skills,
    /// Higher-rep, larger groups, shorter rest.
    Conditioning,
}
db_str!(Mode {
    Balanced => "balanced",
    Strength => "strength",
    Skills => "skills",
    Conditioning => "conditioning",
});

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Settings {
    pub timezone: String,
    pub window_start_hour: i32,
    pub window_end_hour: i32,
    pub night_cutoff_hour: i32,
    pub min_rest_min: i32,
    /// The active coach mode.
    pub mode: Mode,
    /// Roughly how many days a week you train — scales the weekly volume budget.
    pub days_per_week: i32,
    /// Optional region to bias volume toward (×1.5); `null` = no emphasis.
    pub emphasis: Option<Region>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            timezone: "Europe/London".to_string(),
            window_start_hour: 8,
            window_end_hour: 21,
            night_cutoff_hour: 21,
            min_rest_min: 20,
            mode: Mode::default(),
            days_per_week: 4,
            emphasis: None,
        }
    }
}

/// DB row shape (enum columns as raw strings), converted to [`Settings`].
#[derive(sqlx::FromRow)]
pub(crate) struct SettingsRow {
    pub timezone: String,
    pub window_start_hour: i32,
    pub window_end_hour: i32,
    pub night_cutoff_hour: i32,
    pub min_rest_min: i32,
    pub mode: String,
    pub days_per_week: i32,
    pub emphasis: Option<String>,
}

impl TryFrom<SettingsRow> for Settings {
    type Error = anyhow::Error;
    fn try_from(r: SettingsRow) -> Result<Self> {
        Ok(Settings {
            timezone: r.timezone,
            window_start_hour: r.window_start_hour,
            window_end_hour: r.window_end_hour,
            night_cutoff_hour: r.night_cutoff_hour,
            min_rest_min: r.min_rest_min,
            mode: Mode::from_db(&r.mode).ok_or_else(|| anyhow!("unknown mode {:?}", r.mode))?,
            days_per_week: r.days_per_week,
            emphasis: r
                .emphasis
                .as_deref()
                .map(|e| Region::from_db(e).ok_or_else(|| anyhow!("unknown emphasis {e:?}")))
                .transpose()?,
        })
    }
}

/// Body for PATCH /api/settings. Only present fields are written; `emphasis`
/// uses double-option so an explicit `null` clears it.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingsPatch {
    pub timezone: Option<String>,
    pub window_start_hour: Option<i32>,
    pub window_end_hour: Option<i32>,
    pub night_cutoff_hour: Option<i32>,
    pub min_rest_min: Option<i32>,
    pub mode: Option<Mode>,
    pub days_per_week: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(type = "string | null")]
    pub emphasis: Option<Option<Region>>,
}

/// Absent → leave unchanged; `null` → clear; value → set.
fn double_option<'de, T, D>(de: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(de)?))
}
