//! Muscle wire types + the region and role enums.

use anyhow::{Result, anyhow};
use serde::Serialize;

// `Region` and `MuscleRole` live in the pure `coach-pacing` core (the engine
// reasons over them); re-exported here so `crate::muscle::types::Region` and the
// `as_db`/`from_db` conversions the DB rows below rely on keep resolving.
pub use coach_pacing::domain::{MuscleRole, Region};

/// A muscle, with its group + region denormalized for display.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Muscle {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
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
