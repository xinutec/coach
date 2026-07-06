//! Muscle wire types + the region and role enums.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
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

/// Coarse body area a muscle group belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Region {
    Chest,
    Back,
    Shoulders,
    Arms,
    Forearms,
    Core,
    Legs,
}
db_str!(Region {
    Chest => "chest",
    Back => "back",
    Shoulders => "shoulders",
    Arms => "arms",
    Forearms => "forearms",
    Core => "core",
    Legs => "legs",
});

/// How a muscle participates in an exercise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MuscleRole {
    Primary,
    Secondary,
    Stabilizer,
}
db_str!(MuscleRole {
    Primary => "primary",
    Secondary => "secondary",
    Stabilizer => "stabilizer",
});

/// A muscle, with its group + region denormalized for display.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Muscle {
    #[ts(type = "number")]
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub group: String,
    pub region: Region,
    pub function: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct MuscleRow {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub group: String,
    pub region: String,
    pub function: Option<String>,
}

impl TryFrom<MuscleRow> for Muscle {
    type Error = anyhow::Error;
    fn try_from(r: MuscleRow) -> Result<Self> {
        Ok(Muscle {
            id: r.id,
            slug: r.slug,
            name: r.name,
            group: r.group,
            region: Region::from_db(&r.region)
                .ok_or_else(|| anyhow!("unknown region {:?}", r.region))?,
            function: r.function,
        })
    }
}
