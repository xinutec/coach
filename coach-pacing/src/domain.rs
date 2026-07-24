//! The shared domain enums the pacing engine reasons over — region, muscle role,
//! movement pattern, set metric, and training mode. Each carries `as_db`/`from_db`
//! string conversions (the coach ENUM-column convention); the DB row structs and
//! their fallible `TryFrom` conversions stay in the std shell, which re-exports
//! these enums so `crate::muscle::types::Region` etc. keep resolving.

use serde::{Deserialize, Serialize};

/// Generates the `as_db`/`from_db` string mapping. Pure `match`es — no std.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
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

/// Movement pattern. Classification + display; recovery is gated per muscle
/// group, not per pattern (see `pacing::engine`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
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

/// The high-level training intent the engine optimises for — "what am I aiming
/// for right now", switchable per session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
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
