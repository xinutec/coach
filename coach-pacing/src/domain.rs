//! The shared domain vocabulary the pacing engine reasons over — the row
//! identifiers, and the enums for region, muscle role, movement pattern, set
//! metric, and training mode. Each enum carries `as_db`/`from_db` string
//! conversions (the coach ENUM-column convention); the DB row structs and their
//! fallible `TryFrom` conversions stay in the std shell, which re-exports these
//! so `crate::muscle::types::Region` etc. keep resolving.

use serde::{Deserialize, Serialize};

// ---- identifiers -----------------------------------------------------------

/// Generates a row-id newtype: a `i64` primary key that knows which table it
/// came from.
///
/// Every id in this app was an `i64`, which made four different things one type.
/// Nothing stopped a group id being passed where an exercise id was wanted —
/// `blocked_ideal(.., groups.id[ix], c.ex.id)` takes both, adjacent, and swapping
/// them compiled, ran, and produced a plausible-looking wrong answer. The maps
/// had the same problem from the other side: `exercise_loads` needed a doc
/// comment shouting "keyed by exercise id — **not** by equipment" precisely
/// because its type could not say so. A `BTreeMap<ExerciseId, _>` says it, and
/// the comment becomes redundant.
///
/// `#[serde(transparent)]` keeps the wire format a bare number, so this is
/// invisible to the frontend and to Android — the guarantee is bought entirely
/// inside Rust, at no cost to the API.
macro_rules! row_id {
    // `wire` marks an id that appears in a serialized API type, so ts-rs emits a
    // TypeScript alias for it. The other ids are internal to the engine, and
    // generating TS for them would be output nothing imports.
    ($name:ident, wire, $doc:literal) => {
        row_id!(@def $name, $doc,
            #[cfg_attr(feature = "ts", derive(ts_rs::TS))]
            #[cfg_attr(feature = "ts", ts(export, type = "number"))]
        );
    };
    ($name:ident, $doc:literal) => {
        row_id!(@def $name, $doc,);
    };
    (@def $name:ident, $doc:literal, $(#[$extra:meta])*) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        $(#[$extra])*
        pub struct $name(pub i64);

        impl $name {
            /// The underlying row id — for the DB layer and for formatting. Named
            /// rather than reached through `.0` at call sites, so a grep for
            /// `.get()` finds every place the wrapper is deliberately shed.
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

row_id!(
    ExerciseId,
    wire,
    "A row in `exercises` — one movement variation."
);
row_id!(GroupId, "A row in `muscle_groups`.");
row_id!(EquipmentId, "A row in `equipment` — one piece of kit.");
row_id!(
    SetId,
    wire,
    "A row in `workout_sets` — one logged set. Carried on an estimate so the app \
     can point at the *specific* set behind a number the athlete may need to \
     correct."
);

/// A set that has been checked against its exercise's metric — the only shape
/// the log is written from.
///
/// The fields a set may carry are its metric's fields and nothing else. That was
/// a runtime predicate over three independent `Option`s (`NewSet::shape_error`)
/// with a test file of its own, enforced at exactly one call site, and a load on
/// a bodyweight mobility drill got in anyway — twice, and the rows are still in
/// the log. Parsing the request body into this instead means the illegal shapes
/// stop existing after the check rather than merely being disapproved of, and a
/// second write path cannot forget to ask.
///
/// The measurement is required; the load is not. A set that records neither reps
/// nor seconds is not a set — the old predicate accepted an entirely empty body,
/// because it only ever asked whether a *present* field was allowed. The load
/// stays optional because an unweighted set of a weighted movement is honest: an
/// empty-bar technique set the athlete chose not to weigh, and refusing it would
/// lose a real set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoggedSet {
    Reps { reps: i32 },
    WeightedReps { reps: i32, load_kg: Option<f64> },
    Hold { hold_s: i32 },
    WeightedHold { hold_s: i32, load_kg: Option<f64> },
}

/// Value ceilings no honest set exceeds — plausibility, not policy. Generous on
/// purpose: a real outlier day must never be refused, only a number that
/// describes nothing a human did. The round-3 field test stored a fat-fingered
/// **3 530-second** farmers walk (an "append" instead of a replace) and the
/// carry ability model would have read it as a demonstrated max.
const MAX_REPS: i32 = 100;
const MAX_HOLD_S: i32 = 600;
const MAX_LOAD_KG: f64 = 300.0;

impl LoggedSet {
    /// Build the one shape `metric` allows from what the client sent, or say
    /// what is wrong with it. The error strings are the athlete-facing ones.
    pub fn parse(
        metric: Metric,
        reps: Option<i32>,
        load_kg: Option<f64>,
        hold_s: Option<i32>,
    ) -> Result<Self, &'static str> {
        // Which fields this metric has anything to say with. A field outside them
        // is refused rather than dropped: a client sending a load for a bodyweight
        // drill is wrong about something, and silently storing the honest part
        // hides that.
        let (load_ok, reps_ok, hold_ok) = match metric {
            Metric::Reps => (false, true, false),
            Metric::WeightedReps => (true, true, false),
            Metric::Hold => (false, false, true),
            Metric::WeightedHold => (true, false, true),
        };
        if load_kg.is_some() && !load_ok {
            return Err("this exercise takes no load");
        }
        if reps.is_some() && !reps_ok {
            return Err("this exercise is measured in seconds, not reps");
        }
        if hold_s.is_some() && !hold_ok {
            return Err("this exercise is measured in reps, not seconds");
        }
        if reps.is_some_and(|r| !(1..=MAX_REPS).contains(&r)) {
            return Err("reps must be between 1 and 100");
        }
        if hold_s.is_some_and(|s| !(1..=MAX_HOLD_S).contains(&s)) {
            return Err("seconds must be between 1 and 600");
        }
        if load_kg.is_some_and(|l| !(l > 0.0 && l <= MAX_LOAD_KG)) {
            return Err("load must be between 0 and 300 kg");
        }
        // The measurement itself is not optional: "a set" with no reps and no
        // seconds records that something happened and nothing about what.
        const NEEDS_REPS: &str = "how many reps?";
        const NEEDS_SECONDS: &str = "how many seconds?";
        Ok(match metric {
            Metric::Reps => LoggedSet::Reps {
                reps: reps.ok_or(NEEDS_REPS)?,
            },
            Metric::WeightedReps => LoggedSet::WeightedReps {
                reps: reps.ok_or(NEEDS_REPS)?,
                load_kg,
            },
            Metric::Hold => LoggedSet::Hold {
                hold_s: hold_s.ok_or(NEEDS_SECONDS)?,
            },
            Metric::WeightedHold => LoggedSet::WeightedHold {
                hold_s: hold_s.ok_or(NEEDS_SECONDS)?,
                load_kg,
            },
        })
    }

    /// The set as the flat `workout_sets` columns hold it. The schema is three
    /// nullable columns, so exactly one place converts, and this is it.
    pub fn columns(self) -> (Option<i32>, Option<f64>, Option<i32>) {
        match self {
            LoggedSet::Reps { reps } => (Some(reps), None, None),
            LoggedSet::WeightedReps { reps, load_kg } => (Some(reps), load_kg, None),
            LoggedSet::Hold { hold_s } => (None, None, Some(hold_s)),
            LoggedSet::WeightedHold { hold_s, load_kg } => (None, load_kg, Some(hold_s)),
        }
    }
}

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
