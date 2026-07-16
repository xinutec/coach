//! Exercise catalog wire types + the movement/metric/position enums.
//!
//! Enums serialize as snake_case to JSON and carry `as_db`/`from_db` string
//! conversions: rows are read with enum columns as `String` and converted, and
//! writes bind `as_db()` — sidestepping sqlx deriving `Type` for MySQL `ENUM`.
//!
//! An exercise's equipment and muscles are many-to-many (see the `equipment` and
//! `muscle` modules + the join tables); the lightweight [`Exercise`] list item
//! carries just equipment slugs, while [`ExerciseDetail`] carries the full sets.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::equipment::types::Equipment;
use crate::muscle::types::{MuscleRole, Region};

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

/// Movement pattern. Classification + display; recovery is gated per muscle
/// group, not per pattern (see `pacing::engine`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Pattern {
    Push,
    Pull,
    Legs,
    Core,
}
db_str!(Pattern {
    Push => "push",
    Pull => "pull",
    Legs => "legs",
    Core => "core",
});

/// How a set is measured. Determines which of reps/load/hold a logged set carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Metric {
    Reps,
    WeightedReps,
    Hold,
    /// A loaded carry or hold: **weight and time together** (a farmer's walk, a
    /// waiter walk, an overhead carry). Neither of the other two can say it —
    /// `Hold` has no load, `WeightedReps` has no clock — so the four carries in
    /// the catalog were modelled as weighted *reps* and the coach prescribed
    /// "Farmers walk, 5 reps at 6 kg", which is not a thing anyone does. The
    /// progression is the same double-progression shape as a weighted lift, with
    /// seconds where the reps go: climb the time, then step the weight.
    WeightedHold,
}
db_str!(Metric {
    Reps => "reps",
    WeightedReps => "weighted_reps",
    Hold => "hold",
    WeightedHold => "weighted_hold",
});

/// Body position the movement is performed in (optional).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Position {
    Standing,
    Seated,
    Kneeling,
    HalfKneeling,
    Prone,
    Supine,
    Hanging,
    Lunge,
}
db_str!(Position {
    Standing => "standing",
    Seated => "seated",
    Kneeling => "kneeling",
    HalfKneeling => "half_kneeling",
    Prone => "prone",
    Supine => "supine",
    Hanging => "hanging",
    Lunge => "lunge",
});

/// Lightweight catalog list item. Equipment as slugs; `has_image` gates the
/// thumbnail without shipping the blob.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Exercise {
    #[ts(type = "number")]
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub variation: Option<String>,
    pub pattern: Pattern,
    pub metric: Metric,
    pub unilateral: bool,
    /// Gymnastic skill work (rings/parallettes/lever) — biased in Skills mode.
    /// Catalog-authoritative (was a hardcoded equipment-slug sniff).
    pub skill: bool,
    /// A mobility/activation move: the warm-up block draws from these, and they
    /// credit no training volume.
    pub warmup: bool,
    /// How many of the implement this movement uses — one dumbbell (goblet squat,
    /// single-arm row) or two (dumbbell bench press). Decides how a finite disc
    /// budget is shared out, and so which loads are actually buildable.
    pub implements: i32,
    /// How hard this variation is (1–5) relative to its pattern + primary group
    /// — the rung it occupies on the variation ladder (G7).
    pub difficulty: Option<i32>,
    pub is_active: bool,
    pub equipment: Vec<String>,
    pub has_image: bool,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ExerciseListRow {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub variation: Option<String>,
    pub pattern: String,
    pub metric: String,
    pub unilateral: bool,
    pub skill: bool,
    pub warmup: bool,
    pub implements: i32,
    pub difficulty: Option<i32>,
    pub is_active: bool,
    pub equipment_csv: Option<String>,
    pub has_image: i64,
}

impl TryFrom<ExerciseListRow> for Exercise {
    type Error = anyhow::Error;
    fn try_from(r: ExerciseListRow) -> Result<Self> {
        Ok(Exercise {
            id: r.id,
            slug: r.slug,
            name: r.name,
            variation: r.variation,
            pattern: Pattern::from_db(&r.pattern)
                .ok_or_else(|| anyhow!("unknown pattern {:?}", r.pattern))?,
            metric: Metric::from_db(&r.metric)
                .ok_or_else(|| anyhow!("unknown metric {:?}", r.metric))?,
            unilateral: r.unilateral,
            skill: r.skill,
            warmup: r.warmup,
            implements: r.implements,
            difficulty: r.difficulty,
            is_active: r.is_active,
            equipment: r
                .equipment_csv
                .filter(|s| !s.is_empty())
                .map(|s| s.split(',').map(str::to_string).collect())
                .unwrap_or_default(),
            has_image: r.has_image != 0,
        })
    }
}

/// A muscle worked by an exercise, with its group/region and role denormalized.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExerciseMuscle {
    pub slug: String,
    pub name: String,
    pub group: String,
    pub region: Region,
    pub role: MuscleRole,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ExerciseMuscleRow {
    pub slug: String,
    pub name: String,
    pub group: String,
    pub region: String,
    pub role: String,
}

impl TryFrom<ExerciseMuscleRow> for ExerciseMuscle {
    type Error = anyhow::Error;
    fn try_from(r: ExerciseMuscleRow) -> Result<Self> {
        Ok(ExerciseMuscle {
            slug: r.slug,
            name: r.name,
            group: r.group,
            region: Region::from_db(&r.region)
                .ok_or_else(|| anyhow!("unknown region {:?}", r.region))?,
            role: MuscleRole::from_db(&r.role)
                .ok_or_else(|| anyhow!("unknown role {:?}", r.role))?,
        })
    }
}

/// Full exercise view: scalar fields + equipment + muscles.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExerciseDetail {
    #[ts(type = "number")]
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub variation: Option<String>,
    pub pattern: Pattern,
    pub metric: Metric,
    pub position: Option<Position>,
    pub unilateral: bool,
    pub is_active: bool,
    pub cue: Option<String>,
    pub demo_url: Option<String>,
    pub summary: Option<String>,
    pub difficulty: Option<i32>,
    pub has_image: bool,
    pub equipment: Vec<Equipment>,
    pub muscles: Vec<ExerciseMuscle>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ExerciseDetailRow {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub variation: Option<String>,
    pub pattern: String,
    pub metric: String,
    pub position: Option<String>,
    pub unilateral: bool,
    pub is_active: bool,
    pub cue: Option<String>,
    pub demo_url: Option<String>,
    pub summary: Option<String>,
    pub difficulty: Option<i32>,
    pub has_image: i64,
}

/// A muscle link on create/patch: which muscle, in what role.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MuscleLink {
    pub slug: String,
    pub role: MuscleRole,
}

/// Body for POST /api/exercises (a user-added custom movement).
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewExercise {
    pub name: String,
    #[serde(default)]
    pub variation: Option<String>,
    pub pattern: Pattern,
    pub metric: Metric,
    #[serde(default)]
    pub position: Option<Position>,
    #[serde(default)]
    pub unilateral: bool,
    #[serde(default)]
    pub cue: Option<String>,
    #[serde(default)]
    pub demo_url: Option<String>,
    #[serde(default)]
    pub difficulty: Option<i32>,
    /// Equipment slugs (all required). Empty = bodyweight.
    #[serde(default)]
    pub equipment: Vec<String>,
    #[serde(default)]
    pub muscles: Vec<MuscleLink>,
}

/// Body for PATCH /api/exercises/{id}. Scalar fields COALESCE (only present ones
/// change); `equipment`/`muscles`, when present, replace the whole link set.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExercisePatch {
    pub name: Option<String>,
    pub variation: Option<String>,
    pub pattern: Option<Pattern>,
    pub metric: Option<Metric>,
    pub position: Option<Position>,
    pub unilateral: Option<bool>,
    pub cue: Option<String>,
    pub demo_url: Option<String>,
    pub summary: Option<String>,
    pub difficulty: Option<i32>,
    pub is_active: Option<bool>,
    pub equipment: Option<Vec<String>>,
    pub muscles: Option<Vec<MuscleLink>>,
}
