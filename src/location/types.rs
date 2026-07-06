//! Location wire types. `equipment` is the set of equipment slugs available at
//! the location (the frontend has the full catalog from /api/equipment).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Location {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub is_default: bool,
    pub equipment: Vec<String>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct LocationRow {
    pub id: i64,
    pub name: String,
    pub is_default: bool,
    pub equipment_csv: Option<String>,
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
        }
    }
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
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LocationPatch {
    pub name: Option<String>,
    pub is_default: Option<bool>,
    /// When present, replaces the whole equipment set.
    pub equipment: Option<Vec<String>>,
}
