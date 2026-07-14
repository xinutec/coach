//! Equipment wire types + the category enum. Same enum-as-string pattern as the
//! exercise module (read a `String` column, convert; write `as_db()`).

use anyhow::{Result, anyhow};
use serde::Serialize;
use ts_rs::TS;

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

/// Broad kit family, for grouping equipment in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Category {
    FreeWeight,
    Band,
    Machine,
    Ball,
    Rig,
    Bench,
}
db_str!(Category {
    FreeWeight => "free_weight",
    Band => "band",
    Machine => "machine",
    Ball => "ball",
    Rig => "rig",
    Bench => "bench",
});

/// A piece of equipment.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Equipment {
    #[ts(type = "number")]
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub category: Category,
    /// A loadable bar (barbell, trap bar): its load is the empty bar + plates,
    /// not a fixed size. The UI collects a bar weight + plate sizes for these,
    /// rather than a list of discrete owned weights.
    pub loadable: bool,
    /// Kit that carries a load — the athlete registers weights for it, and the
    /// coach may prescribe one. True of free weights *and* of a cable/selectorised
    /// stack (whose pin positions are just a ladder of discrete weights); false of
    /// a bench, a rig, a mat. Every `loadable` kit is `weighted`; the converse
    /// isn't — a fixed dumbbell is weighted but takes no plates.
    pub weighted: bool,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EquipmentRow {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub category: String,
    pub loadable: bool,
    pub weighted: bool,
}

impl TryFrom<EquipmentRow> for Equipment {
    type Error = anyhow::Error;
    fn try_from(r: EquipmentRow) -> Result<Self> {
        Ok(Equipment {
            id: r.id,
            slug: r.slug,
            name: r.name,
            category: Category::from_db(&r.category)
                .ok_or_else(|| anyhow!("unknown equipment category {:?}", r.category))?,
            loadable: r.loadable,
            weighted: r.weighted,
        })
    }
}
