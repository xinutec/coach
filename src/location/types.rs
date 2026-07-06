//! Location wire types. `equipment` is the set of equipment slugs available at
//! the location (the frontend has the full catalog from /api/equipment).

use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

/// Distinguish an absent field from an explicit `null` in a PATCH body: absent →
/// `None` (leave unchanged), `null` → `Some(None)` (clear), value → `Some(Some)`.
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::deserialize(de)?))
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Location {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub is_default: bool,
    pub equipment: Vec<String>,
    /// health-sync focus_place this location is linked to (for auto-select), if any.
    #[ts(type = "number | null")]
    pub health_place_id: Option<i64>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct LocationRow {
    pub id: i64,
    pub name: String,
    pub is_default: bool,
    pub equipment_csv: Option<String>,
    pub health_place_id: Option<i64>,
}

impl From<LocationRow> for Location {
    fn from(r: LocationRow) -> Self {
        Location {
            id: r.id,
            name: r.name,
            is_default: r.is_default,
            equipment: r
                .equipment_csv
                .filter(|s| !s.is_empty())
                .map(|s| s.split(',').map(str::to_string).collect())
                .unwrap_or_default(),
            health_place_id: r.health_place_id,
        }
    }
}

/// Which of the user's locations they're currently at (resolved from health's
/// detected current place), or none.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CurrentLocation {
    #[ts(type = "number | null")]
    pub location_id: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewLocation {
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub equipment: Vec<String>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub health_place_id: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LocationPatch {
    pub name: Option<String>,
    pub is_default: Option<bool>,
    /// When present, replaces the whole equipment set.
    pub equipment: Option<Vec<String>>,
    /// Link to a health focus_place: absent → unchanged, `null` → unlink, id → link.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(type = "number | null")]
    pub health_place_id: Option<Option<i64>>,
}
