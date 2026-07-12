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

/// Per-equipment specifics at a location: which discrete weights (fixed free
/// weights), named variants (bands), or bar/handle you own. Plates live on
/// `Location` (they may be shared across bars). All-empty = no specifics given.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EquipmentOption {
    pub slug: String,
    /// Discrete weights owned (kg) — coach snaps load suggestions to these.
    /// For fixed free weights (dumbbell, kettlebell).
    pub weights: Vec<f64>,
    /// How many of each weight in `weights` you own (same order). A movement that
    /// uses two dumbbells can't be built from a weight you own one of. Missing or
    /// short → "plenty", which is what a gym rack is.
    #[serde(default)]
    pub weight_qty: Vec<u32>,
    /// Named variants owned (e.g. band tensions) — informational.
    pub labels: Vec<String>,
    /// A loadable bar or dumbbell handle's own weight (kg) — the load floor.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub bar_kg: Option<f64>,
    /// How many of that bar/handle you own. A both-arms dumbbell press needs two.
    /// `None` = plenty.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub bar_qty: Option<u32>,
    /// How many discs fit on one sleeve. `None` = as many as you own.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub plate_slots: Option<u32>,
}

/// A plate you own at a location. `equipment` names the kit it fits — `None` is
/// the shared pool every loadable bar here draws from (Olympic discs go on the
/// barbell *and* the trap bar), while a value pins it to one piece of kit (a
/// dumbbell handle's small discs will not go on a barbell). `qty` is how many
/// discs you have: `None` = plenty (a gym rack), otherwise only pairs
/// (`qty / 2`) can be loaded, since plates go on both ends or not at all.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Plate {
    #[serde(default)]
    pub equipment: Option<String>,
    pub load_kg: f64,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub qty: Option<u32>,
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
    /// Specifics for equipment that has them (weights/band variants/bar weight).
    /// Only equipment with at least one option appears here.
    pub equipment_options: Vec<EquipmentOption>,
    /// Plates available at this location — coach only suggests totals it can
    /// actually build from a bar/handle + the plates that fit it, in the numbers
    /// you own.
    pub plates: Vec<Plate>,
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
            equipment_options: Vec::new(), // filled by the repo (separate query)
            plates: Vec::new(),            // filled by the repo (separate query)
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
    pub equipment_options: Vec<EquipmentOption>,
    #[serde(default)]
    pub plates: Vec<Plate>,
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
    /// When present, replaces all per-equipment specifics (weights/band variants).
    pub equipment_options: Option<Vec<EquipmentOption>>,
    /// When present, replaces the location's plate set.
    pub plates: Option<Vec<Plate>>,
    /// Link to a health focus_place: absent → unchanged, `null` → unlink, id → link.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(type = "number | null")]
    pub health_place_id: Option<Option<i64>>,
}
