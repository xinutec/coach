//! Dynamic-coach engine input (plain data, assembled from repos) and output
//! (wire types). The engine [`super::engine::evaluate`] is a pure function over
//! these: it computes what to do now from **history + the active mode**, with no
//! program. Rolling muscle-group volume + recovery + progression, location-aware.

use crate::prelude::*;
use alloc::collections::BTreeMap;

use chrono::{NaiveDate, NaiveDateTime};
use serde::Serialize;

use crate::domain::Mode;
use crate::domain::{EquipmentId, ExerciseId, GroupId, SetId};
use crate::domain::{Metric, Pattern};
use crate::domain::{MuscleRole, Region};

use super::ability::Confidence;
use super::dose::{Dose, Measure};

// ---- inputs (internal; not wire types) -------------------------------------

#[derive(Clone)]
pub struct ExerciseInfo {
    pub id: ExerciseId,
    pub name: String,
    /// The movement family — the catalog's base name, shared by its variations
    /// ("Farmers walk" plain/suitcase/waiter; both "Hamstring curls"). Cousins
    /// train the same thing the same way, so a session takes at most one entry
    /// per family (R3-3).
    pub family: String,
    /// How hard this variation is (1–5) *relative to its pattern + primary
    /// group* — the rung it occupies on the variation ladder (G7).
    pub difficulty: Option<i32>,
    pub pattern: Pattern,
    pub metric: Metric,
    /// Ring/parallette or hold work — biased in Skills mode.
    pub is_skill: bool,
    /// Maximal-intent ballistic work (jumps, throws, Olympic lifts, plyo) — the
    /// session leads with it, before strength compounds, so fatigue doesn't rob
    /// the movement (or its calibration measurement) of quality.
    pub is_power: bool,
    /// A mobility/activation move: only the warm-up block picks it, and it
    /// credits no training volume.
    pub warmup: bool,
    /// Equipment ids required (empty = bodyweight).
    pub equipment: Vec<EquipmentId>,
    /// Muscle groups this exercise trains, with the strongest role for each.
    pub groups: Vec<(GroupId, MuscleRole)>,
}

/// A logged set in the trailing history window (rich enough for volume,
/// progression, and the ability estimate). `rpe` (when logged) makes the e1RM
/// estimate effort-aware — a set left with reps in reserve implies more strength
/// than a grinding one at the same load.
#[derive(Clone)]
pub struct SetRec {
    /// The `workout_sets` row this came from. Carried so the engine can point at
    /// the *specific* set behind an estimate — a number the athlete can only
    /// correct if the app can tell him which set produced it. Identifying it by
    /// timestamp instead would risk offering to delete the wrong row.
    pub id: SetId,
    pub exercise_id: ExerciseId,
    pub logged_at: NaiveDateTime,
    pub reps: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
    /// Metres, for a carry measured by distance.
    pub distance_m: Option<i32>,
    pub rpe: Option<i32>,
}

/// Muscle-group identity for output labelling + the balance view.
#[derive(Clone)]
pub struct GroupMeta {
    pub id: GroupId,
    pub name: String,
    pub region: Region,
}

#[derive(Clone, Copy)]
pub struct PacingSettings {
    pub window_start_hour: i32,
    pub window_end_hour: i32,
    pub min_rest_min: i32,
}

/// The equipment present where the athlete is training.
///
/// Deliberately *not* an `Option<BTreeSet>` consulted with `is_none_or`: that
/// spelling made "we don't know the location" mean "everything is doable", so a
/// missing location silently switched the safety filter off and the coach
/// cheerfully suggested trap-bar deadlifts in a living room. Absent kit now means
/// absent kit. Not knowing where you are is a *different* state
/// ([`PacingInput::kit`] = `None`), and it yields a narrower verdict — no
/// suggestions at all — rather than a wider one.
#[derive(Clone, Debug, Default)]
pub struct Kit(pub alloc::collections::BTreeSet<EquipmentId>);

impl Kit {
    /// Is every piece of `required` equipment present here? (Empty = bodyweight,
    /// always true.)
    pub fn has_all(&self, required: &[EquipmentId]) -> bool {
        required.iter().all(|e| self.0.contains(e))
    }
}

/// Everything the engine needs, already fetched.
pub struct PacingInput {
    pub mode: Mode,
    pub days_per_week: i32,
    pub emphasis: Option<Region>,
    pub exercises: Vec<ExerciseInfo>,
    /// Trailing history (≈6 months) — every set's reps/load/hold/rpe, feeding
    /// both rolling volume and the ability estimate (which decays old sets).
    pub history: Vec<SetRec>,
    pub last_set_at: Option<NaiveDateTime>,
    pub settings: PacingSettings,
    pub groups: Vec<GroupMeta>,
    /// The kit at the athlete's location. `None` = no location known, so the
    /// engine can't say what's doable and won't guess: the verdict carries no
    /// plan and asks for a location. Degradation narrows the claim, never widens it.
    pub kit: Option<Kit>,
    /// The loads each exercise can actually be built with here. Keyed per
    /// *exercise* rather than per piece of kit because what's buildable depends on
    /// how many implements the movement needs: a pair of dumbbells splits a finite
    /// disc budget between them, and a fixed weight you own one of can't serve a
    /// two-dumbbell press. Absent or empty = not loadable here, so the lift isn't
    /// selectable (see [`super::dose::Inventory`]) and the verdict says why rather
    /// than inventing a number.
    pub exercise_loads: BTreeMap<ExerciseId, Vec<f64>>,
    /// Equipment id → its display name, so a blocked substitution can name the kit
    /// it's missing instead of saying "its kit isn't here" and leaving the athlete
    /// to guess which piece.
    pub equipment_names: BTreeMap<EquipmentId, String>,
    /// Kit the coach had to leave out, and why — surfaced on the verdict so a drop
    /// reads as something to fix rather than a hole in the plan.
    pub notices: Vec<String>,
    /// Biometric readiness (from health), if available. `None` → the engine falls
    /// back to the volume-spike deload heuristic.
    pub readiness: Option<Readiness>,
    /// The days each movement was *offered* — put on a card the athlete could
    /// have done — keyed by exercise.
    ///
    /// Raw, like `readiness_history`: the engine owns the judgment about what
    /// counts as neglect. This cannot be derived from `history`, which is by
    /// construction the record of what *did* happen; "offered twenty times,
    /// performed zero" is a fact about cards, and R6-4 is the finding that no
    /// group-level statistic can see it (Pistol squat offered 8, performed 0,
    /// while Quadriceps was the best-served group in the whole log).
    pub offers: BTreeMap<ExerciseId, Vec<NaiveDate>>,
    /// Readiness as it stood on each past training day, keyed by local date.
    ///
    /// The prediction-error ledger needs it. The coach asks for *less* on an
    /// under-recovered morning, so judging that session as though it had been
    /// full-effort records the athlete's compliance as a failure — which then holds
    /// their progression back for having slept badly. A day that's absent (health
    /// has no data, or is down) is judged full-effort: exactly what the ledger did
    /// before it could ask the question, so a missing signal never invents an easing
    /// that didn't happen.
    pub readiness_history: BTreeMap<NaiveDate, Readiness>,
}

/// How recovered the user is right now, from biometrics (health-derived).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
pub enum Band {
    Low,
    Normal,
    High,
}

/// The readiness verdict coach computes from health's raw recovery data.
///
/// The fields are private and there is one constructor, because `band` is a pure
/// function of `score` and a struct that stores both invites them to disagree —
/// the same defect as a length kept beside the list it counts. Both still cross
/// the wire: the client should not be re-deriving the thresholds, and when it
/// tried, the two ended up written down in three places (here, `tests/readiness`
/// and inline literals in `tests/engine_props`).
#[derive(Clone, Copy, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Readiness {
    /// 0 (unrecovered) .. 1 (fully recovered).
    score: f64,
    band: Band,
}

/// Below this the day is under-recovered; above [`BAND_HIGH`] it is a good one.
/// The one place these live.
pub const BAND_LOW: f64 = 0.40;
pub const BAND_HIGH: f64 = 0.65;

impl Readiness {
    /// The verdict for a 0..1 recovery score. Clamped, so a caller's arithmetic
    /// cannot produce a readiness outside the scale it is defined on.
    pub fn of(score: f64) -> Self {
        let score = score.clamp(0.0, 1.0);
        let band = if score < BAND_LOW {
            Band::Low
        } else if score > BAND_HIGH {
            Band::High
        } else {
            Band::Normal
        };
        Readiness { score, band }
    }

    /// 0 (unrecovered) .. 1 (fully recovered).
    pub fn score(self) -> f64 {
        self.score
    }

    /// The band `score` falls in — derived, never stored independently.
    pub fn band(self) -> Band {
        self.band
    }
}

// ---- output (wire types) ---------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
pub enum PacingState {
    /// A concrete thing to do now.
    Active,
    /// Everything due is recovered/at target — rest, or an optional light set.
    Rest,
    /// No history yet — a cold-start suggestion to get going.
    Fresh,
}

/// Rolling volume vs target for one muscle group — drives the balance view.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct GroupBalance {
    pub group: String,
    pub region: Region,
    /// Effective sets over the trailing 7 days (primary 1.0, secondary 0.5).
    pub current: f64,
    pub target: f64,
    /// (target − current)/target, clamped 0..1.
    pub deficit: f64,
    pub recovering: bool,
}

/// Whether a suggestion is a normal prescription or a calibration task. When the
/// engine's ability estimate for the chosen exercise is untrusted (never done,
/// or only stale data), it can't prescribe honestly — so it asks you to measure:
/// the logged set *is* the assessment, and the next verdict prescribes from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
pub enum SuggestionKind {
    /// A mobility/activation move or a light ramp-in set — prep, not training.
    Warmup,
    /// A prescription derived from a trusted ability estimate.
    Work,
    /// A calibration set — the engine is measuring what you can do.
    Assess,
}

/// What the coach is asking for, in the terms the movement's metric allows.
///
/// This is the wire form of [`super::dose::Dose`] (a prescription) and
/// [`super::dose::Measure`] (a calibration), which are one type here because a
/// card shows one or the other and never both.
///
/// It exists because the guarantee `dose` establishes used to stop at
/// `Serialize`. The verdict carried `rep_low`, `rep_high`, `load_kg` and
/// `hold_s` as four independent `Option`s — the same "thirty-two representable
/// shapes, about three legal ones" that `dose`'s own doc comment describes
/// itself as having removed. Everything downstream then had to guess the shape
/// back: the engine did it to phrase "do this next", the back-test did it to
/// recover what the coach had asked, the simulator did it to decide what the
/// athlete should perform, and the Today card did it twice more in TypeScript.
/// Six reconstructions of a fact that was known exactly at the point it was
/// computed, each free to disagree with the others — and the ledger disagreeing
/// with the coach is the failure this area keeps rediscovering (R4-1, R5-1,
/// R6-1).
///
/// Tagged, so the frontend gets a discriminated union and `@switch` is
/// exhaustive over it rather than a chain of null tests.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
#[serde(rename_all_fields = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub enum Ask {
    /// A weighted lift: climb from `rep_low` to `rep_high` at this load, then the
    /// load steps. The load is not optional — a weighted set without a weight is
    /// not a lighter prescription, it is a nonsense one.
    Weighted {
        load_kg: f64,
        rep_low: i32,
        rep_high: i32,
    },
    /// Bodyweight reps — the only lever is the rep count.
    Bodyweight { rep_low: i32, rep_high: i32 },
    /// An unloaded hold, in seconds.
    Hold { hold_s: i32 },
    /// A loaded carry: both, because a carry is both.
    WeightedHold { load_kg: f64, hold_s: i32 },
    /// Calibration — build up to a hard-but-clean set of `reps` and log what it
    /// took. `start_kg` is a safe opening weight, never a prescription.
    BuildUp { start_kg: f64, reps: i32 },
    /// Calibration — as many clean reps as you have.
    Amrap,
    /// Calibration — one max hold.
    MaxHold,
    /// Calibration — carry `start_kg` for as long as form holds; the weight *and*
    /// the time are the measurement.
    LoadedCarry { start_kg: f64 },
    /// A carry measured by distance: this weight, this far.
    WeightedDistance { load_kg: f64, distance_m: i32 },
    /// Calibration — carry `start_kg` as far as form holds, logging both.
    LoadedDistance { start_kg: f64 },
}

impl Ask {
    /// The weight this ask names, if it names one. Derived from the variant, so
    /// unlike the `Option<f64>` field it replaced it cannot disagree with the
    /// rest of the ask — there is no way to build a weighted lift that has lost
    /// its load, or a bodyweight one that has acquired a load.
    pub fn load_kg(self) -> Option<f64> {
        match self {
            Ask::Weighted { load_kg, .. }
            | Ask::WeightedHold { load_kg, .. }
            | Ask::WeightedDistance { load_kg, .. } => Some(load_kg),
            Ask::BuildUp { start_kg, .. }
            | Ask::LoadedCarry { start_kg }
            | Ask::LoadedDistance { start_kg } => Some(start_kg),
            Ask::Bodyweight { .. } | Ask::Hold { .. } | Ask::Amrap | Ask::MaxHold => None,
        }
    }

    /// The bottom of the rep range — the number actually asked for — if this ask
    /// is counted in reps.
    pub fn rep_low(self) -> Option<i32> {
        match self {
            Ask::Weighted { rep_low, .. } | Ask::Bodyweight { rep_low, .. } => Some(rep_low),
            Ask::BuildUp { reps, .. } => Some(reps),
            Ask::Hold { .. }
            | Ask::WeightedHold { .. }
            | Ask::Amrap
            | Ask::MaxHold
            | Ask::LoadedCarry { .. }
            | Ask::WeightedDistance { .. }
            | Ask::LoadedDistance { .. } => None,
        }
    }

    /// The seconds this ask names, if it is timed.
    pub fn hold_s(self) -> Option<i32> {
        match self {
            Ask::Hold { hold_s } | Ask::WeightedHold { hold_s, .. } => Some(hold_s),
            Ask::Weighted { .. }
            | Ask::Bodyweight { .. }
            | Ask::BuildUp { .. }
            | Ask::Amrap
            | Ask::MaxHold
            | Ask::LoadedCarry { .. }
            | Ask::WeightedDistance { .. }
            | Ask::LoadedDistance { .. } => None,
        }
    }

    /// The metres this ask names, if it is a carry measured by distance.
    pub fn distance_m(self) -> Option<i32> {
        match self {
            Ask::WeightedDistance { distance_m, .. } => Some(distance_m),
            Ask::Weighted { .. }
            | Ask::Bodyweight { .. }
            | Ask::Hold { .. }
            | Ask::WeightedHold { .. }
            | Ask::BuildUp { .. }
            | Ask::Amrap
            | Ask::MaxHold
            | Ask::LoadedCarry { .. }
            | Ask::LoadedDistance { .. } => None,
        }
    }

    /// The top of the rep range, if this ask is counted in reps.
    pub fn rep_high(self) -> Option<i32> {
        match self {
            Ask::Weighted { rep_high, .. } | Ask::Bodyweight { rep_high, .. } => Some(rep_high),
            Ask::BuildUp { reps, .. } => Some(reps),
            Ask::Hold { .. }
            | Ask::WeightedHold { .. }
            | Ask::Amrap
            | Ask::MaxHold
            | Ask::LoadedCarry { .. }
            | Ask::WeightedDistance { .. }
            | Ask::LoadedDistance { .. } => None,
        }
    }
}

impl From<Dose> for Ask {
    fn from(d: Dose) -> Self {
        match d {
            Dose::Weighted { load, reps } => Ask::Weighted {
                load_kg: load,
                rep_low: reps.low,
                rep_high: reps.high,
            },
            Dose::Bodyweight { reps } => Ask::Bodyweight {
                rep_low: reps.low,
                rep_high: reps.high,
            },
            Dose::Hold { secs } => Ask::Hold { hold_s: secs },
            Dose::WeightedHold { load, secs } => Ask::WeightedHold {
                load_kg: load,
                hold_s: secs,
            },
            Dose::WeightedDistance { load, metres } => Ask::WeightedDistance {
                load_kg: load,
                distance_m: metres,
            },
        }
    }
}

impl From<Measure> for Ask {
    fn from(m: Measure) -> Self {
        match m {
            Measure::BuildUp { start, reps } => Ask::BuildUp {
                start_kg: start,
                reps,
            },
            Measure::Amrap => Ask::Amrap,
            Measure::MaxHold => Ask::MaxHold,
            Measure::LoadedCarry { start } => Ask::LoadedCarry { start_kg: start },
            Measure::LoadedDistance { start } => Ask::LoadedDistance { start_kg: start },
        }
    }
}

/// The logged set an ability estimate came from, named on the card so it can be
/// corrected. `setId` is the `workout_sets` row, so the UI can act on exactly
/// that set rather than guessing from a timestamp.
#[derive(Clone, Copy, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct EstimateSource {
    pub set_id: SetId,
    pub logged_at: NaiveDateTime,
    pub load_kg: Option<f64>,
    pub reps: Option<i32>,
    pub hold_s: Option<i32>,
}

/// Why the engine chose this exercise + prescription — a structured trace so the
/// UI can show its reasoning and tests can assert on it (rather than string-match
/// prose). Every number here is one the verdict already computed.
#[derive(Clone, Copy, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Explanation {
    /// How far below target this muscle group is (0 = at target, 1 = untrained).
    pub deficit: f64,
    /// Recovery fraction for the group (0 = just hammered, 1 = fully recovered).
    pub recovery: f64,
    /// Effective sets of genuine need this exercise's first set paid down — the
    /// number the cover actually ranked and gated it on (`deficit` and `recovery`
    /// are the human-readable factors behind it). An item is only planned when
    /// this clears [`super::cover::MIN_PAY`], so the trace proves the gate held.
    pub pays: f64,
    /// This movement is in today's plan to *confirm its baseline*, not to pay down
    /// group volume — its muscles are already covered for the week, but the estimate
    /// isn't trusted yet, so another session on it is worth more than a new movement.
    /// The card leads with that instead of a near-zero deficit that would read as
    /// "why is this even here?".
    pub confirming: bool,
    /// How much the engine trusts its ability estimate for this exercise.
    pub confidence: Confidence,
    /// Estimated 1-rep-max (kg) the load was derived from, when known.
    pub e1rm: Option<f64>,
    /// The single logged set that set the estimate above — the max is one real
    /// set, and this names it.
    ///
    /// Shown so a wrong number is correctable. Ability is a max, so one mistyped
    /// set becomes a ceiling nothing later can lower, and the offending set is
    /// usually weeks old — "the coach is asking for something absurd" is
    /// otherwise an archaeology problem with no way in.
    pub estimate_from: Option<EstimateSource>,
    /// Sessions in a row the athlete has come in under this estimate. Non-zero means
    /// the prescription was held back or stepped down on purpose, and the card can
    /// say so — "eased off" reads as a decision; the same number twice in a row
    /// after a bad session reads as the coach not listening.
    pub misses: i32,
    /// The biometric readiness band that scaled today's volume, if health had data.
    pub readiness: Option<Band>,
}

/// Why the coach couldn't give you the movement it wanted to, in the athlete's
/// terms. The two cases are different problems with different fixes, so they're
/// different variants rather than one vague "kit isn't here".
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", tag = "kind", content = "kit")]
#[cfg_attr(feature = "ts", ts(export))]
pub enum Blocker {
    /// The location doesn't have this equipment at all (named).
    Absent(Vec<String>),
    /// The equipment is here, but no weights are registered for it — so no honest
    /// load exists and the coach won't invent one.
    Unweighted(Vec<String>),
}

/// The movement the coach would have prescribed, and what stopped it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Substitution {
    pub ideal: String,
    pub blocker: Blocker,
}

/// One set already logged against a plan item, as the row actually holds it.
///
/// The card used to report progress as a bare count — "1 / 2 sets" — which
/// answers "how many" and not "what". Standing over the bar on set two, the
/// question is what you did on set one, and the only place that lived was the
/// History tab.
///
/// Deliberately **not** metric-shaped, unlike [`Ask`] and unlike the validated
/// [`crate::domain::LoggedSet`] the write path now takes. It reports history,
/// and history is not clean: 65 of the 357 sets in the log do not fit their
/// exercise's metric. Nearly all are the 2024 import, which writes its own
/// INSERT and never saw the shape check; two are mobility drills carrying 4 kg
/// from the stale-form-field post that prompted the check in the first place.
/// A sum type here would have to drop those rows or refuse to load them, and
/// silently under-reporting what the athlete did is worse than reporting it
/// oddly. Strictness belongs where the row is *created*, which is where it now
/// is.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DoneSet {
    pub reps: Option<i32>,
    pub load_kg: Option<f64>,
    pub hold_s: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Suggestion {
    pub exercise_id: ExerciseId,
    pub exercise_name: String,
    pub pattern: Pattern,
    /// Work (prescribe) or Assess (measure). Drives the Today card's framing.
    pub kind: SuggestionKind,
    pub sets: i32,
    /// The sets of this item already logged **today**, oldest first — what the
    /// athlete has actually put in against the plan's commitment.
    ///
    /// Day-scoped, not session-scoped: the session gap elapses hours before the
    /// day does, and the plan forgetting your morning is not something you should
    /// have to work around.
    ///
    /// There used to be a `done: i32` beside this, documented as "always `done`
    /// entries long" — a length carried twice, which is a length that can
    /// disagree with itself. It's [`Suggestion::done`] now.
    pub logged: Vec<DoneSet>,
    /// What to actually do: the prescription, or the calibration that stands in
    /// for one when the estimate isn't trusted.
    pub ask: Ask,
    /// The muscle group this targets (for the reason text).
    pub group: String,
    /// When set, the ideal exercise for this group genuinely isn't doable here, so
    /// an equivalent was swapped in: the ideal's name, and what it would take to do
    /// it instead. A swap the athlete can act on ("buy a cable machine", "register
    /// your kettlebell weights") rather than an unexplained substitution.
    ///
    /// Only ever set when the ideal is *actually* blocked. It used to be set
    /// whenever the ideal wasn't what the cover picked — which is the normal case,
    /// and made the card claim kit was missing that was standing right there.
    pub substituted_for: Option<Substitution>,
    /// Why this was chosen (deficit, recovery, ability, readiness). `None` for
    /// warm-up items, which are prep rather than a reasoned prescription.
    pub explanation: Option<Explanation>,
}

impl Suggestion {
    /// How many sets of this item are already in — the length of [`logged`], not
    /// a second copy of it.
    ///
    /// [`logged`]: Suggestion::logged
    pub fn done(&self) -> i32 {
        // A plan never holds more sets than an i32 can count.
        self.logged.len() as i32
    }
}

/// Where the moment sits relative to the athlete's training window.
///
/// This was `within_window: bool` beside `after_window: bool` — four states for
/// three real ones, with "both true" meaningless and readers spelling "before"
/// as `!within_window && !after_window`. A clock is somewhere on a line, so it
/// is one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", ts(export))]
pub enum WindowState {
    /// Before it opens — the coach has nothing to say yet.
    Before,
    /// Inside it. The only state that nudges.
    Within,
    /// Past its end; the coach defers to tomorrow.
    After,
}

/// The full coach verdict for an instant. Drives the Today UI and the Android
/// nudge (fired only when `nudge` AND the phone's geofence says you're home).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PacingNow {
    pub state: PacingState,
    /// Auto-deload active — volume's been high (only the no-biometric fallback;
    /// suppressed when `readiness` is present, which supersedes it).
    pub deload: bool,
    /// Biometric readiness driving today's volume/progression, when health had data.
    pub readiness: Option<Readiness>,
    pub nudge: bool,
    pub reason: String,
    /// Where now sits relative to the training window.
    pub window: WindowState,
    pub spacing_ok: bool,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub minutes_since_last_set: Option<i64>,
    /// The computed session-size target + what's been done today (drive the nudge).
    pub day_target_sets: i32,
    pub day_done_sets: i32,
    pub groups: Vec<GroupBalance>,
    /// The head of `plan` — "next up" — kept for the nudge + the Android trigger.
    pub suggestion: Option<Suggestion>,
    /// The ordered session for today: a greedy set-cover of the day's muscle-group
    /// need (see [`super::cover`]), so each exercise appears **once** with the set
    /// count it earned, ordered by training tier (skill/hold → heavy compound →
    /// accessory → core). Recomputed statelessly each call, so logging a set
    /// reshapes it live.
    pub plan: Vec<Suggestion>,
    /// Things the athlete should know that aren't a set to do — chiefly kit that
    /// can't be prescribed because its weights aren't registered here. The engine
    /// drops those exercises rather than guessing a load; saying so is what keeps
    /// the drop from looking like a silent gap in the plan.
    pub notices: Vec<String>,
}
