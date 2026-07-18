//! Assemble the dynamic engine's input from the DB and run it. All timezone
//! handling lives here: `logged_at` is stored UTC, everything the engine sees is
//! the user's local tz. No program is loaded — the engine works off history +
//! the active mode.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use chrono::{Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use sqlx::MySqlPool;

use crate::equipment::repo as equipment_repo;
use crate::exercise::repo as ex_repo;
use crate::exercise::types::Metric;
use crate::location::loads;
use crate::location::repo as location_repo;
use crate::muscle::repo as muscle_repo;
use crate::muscle::types::Region;
use crate::settings::repo as settings_repo;
use crate::settings::types::Mode;
use crate::workout::repo as workout_repo;

use super::engine;
use super::types::{
    ExerciseInfo, GroupMeta, Kit, PacingInput, PacingNow, PacingSettings, Readiness, SetRec,
};

/// How far back to load set history. Wide enough that the ability model's
/// staleness decay (which floors around ~30 weeks idle) sees a returning
/// athlete's recent-ish PRs; the engine's own 7-day / 8-week windows filter
/// within it. A set older than this simply doesn't inform today's estimate.
const HISTORY_WEEKS: i64 = 26;

/// The history-independent engine context for a user + location: their timezone,
/// settings, active mode, and the catalog/group/inventory metadata a verdict
/// needs. Assembled once from the DB ([`context`]), then combined with a history
/// slice + instant into a [`PacingInput`] ([`input_from`]). The live verdict
/// ([`now`]) and the back-test harness share this so both see identical assembly.
pub struct PacingContext {
    pub tz: Tz,
    pub settings: PacingSettings,
    pub mode: Mode,
    pub days_per_week: i32,
    pub emphasis: Option<Region>,
    pub exercises: Vec<ExerciseInfo>,
    pub groups: Vec<GroupMeta>,
    /// The kit where the athlete is training. `None` only when they have no
    /// location at all — the engine then declines to plan rather than guessing
    /// what's doable.
    pub kit: Option<Kit>,
    /// Buildable loads per *exercise* (not per equipment — a two-dumbbell movement
    /// gets half the discs). Empty = not loadable here.
    pub exercise_loads: HashMap<i64, Vec<f64>>,
    /// Kit the coach had to leave out, and why.
    pub notices: Vec<String>,
    /// Equipment id → display name, so a blocked substitution can name the kit.
    pub equipment_names: HashMap<i64, String>,
}

/// Load the history-independent context: settings + tz, the active mode, the
/// exercise catalog with its flags/equipment/groups, the muscle groups, and the
/// location's available equipment + owned weights (for load snapping). Everything
/// a verdict needs *except* the set history and the instant.
pub async fn context(
    pool: &MySqlPool,
    user_id: &str,
    location_id: Option<i64>,
) -> Result<PacingContext> {
    let s = settings_repo::get(pool, user_id).await?;
    let tz: Tz = s.timezone.parse().unwrap_or(chrono_tz::Europe::London);
    let settings = PacingSettings {
        window_start_hour: s.window_start_hour,
        window_end_hour: s.window_end_hour,
        min_rest_min: s.min_rest_min,
    };
    let mode = s.mode;

    // Where are we training? An explicit location, else the default one. Only a
    // user with *no* locations at all gets `None` — and then the engine declines
    // to plan rather than assuming an empty gym or, worse, a fully-stocked one.
    let location = match location_id {
        Some(id) => Some(id),
        None => location_repo::list(pool, user_id)
            .await?
            .iter()
            .find(|l| l.is_default)
            .map(|l| l.id),
    };
    let kit = match location {
        Some(id) => location_repo::equipment_ids(pool, user_id, id)
            .await?
            .map(|ids| Kit(ids.into_iter().collect::<HashSet<i64>>())),
        None => None,
    };
    // The loadable kit here: fixed weights, bars/handles, and the plates that fit
    // each. Raw facts — what's *buildable* depends on how many implements the
    // movement needs, so that's resolved per exercise below.
    let kit_loads = match location {
        Some(id) => location_repo::kit_loads(pool, id).await?,
        None => HashMap::new(),
    };
    let equipment = equipment_repo::list(pool).await?;
    let equipment_names: HashMap<i64, String> =
        equipment.iter().map(|e| (e.id, e.name.clone())).collect();
    // Which kit actually *carries* the weight. A bench and a pull-up bar are needed
    // for a dumbbell bench press and a weighted chin-up, but you don't load them —
    // asking what weights are registered for a bench is a category error, and it
    // used to produce a notice telling the athlete to go and weigh their furniture.
    //
    // This is the catalog's `weighted` flag, not a guess from the category. Reading
    // it as `category == FreeWeight` was right about the bench and wrong about the
    // pulley: a cable stack is a `machine`, so the coach could put no weight on the
    // one machine in the gym whose whole purpose is the weight on it.
    let bears_load: HashSet<i64> = equipment
        .iter()
        .filter(|e| e.weighted)
        .map(|e| e.id)
        .collect();

    // Exercise metadata: equipment ids, muscle-group contributions, flags.
    let equip_by_ex = ex_repo::equipment_by_exercise(pool).await?;
    let groups_by_ex = ex_repo::muscle_groups_by_exercise(pool).await?;
    let exercises: Vec<ExerciseInfo> = ex_repo::list(pool, false)
        .await?
        .into_iter()
        .map(|e| {
            let equipment = equip_by_ex.get(&e.id).cloned().unwrap_or_default();
            // Skill = the catalog flag (gymnastic ring/parallette work) or any
            // hold (isometrics are skill-biased). No more equipment-slug sniffing.
            let is_skill = e.skill || e.metric == Metric::Hold;
            // The full display name: variations are distinct movements ("Pull-up
            // (L-sit)" is a hold, not a rep-out) — a bare shared base name in a
            // suggestion would misname what the coach is actually asking for.
            let name = match &e.variation {
                Some(v) => format!("{} ({v})", e.name),
                None => e.name.clone(),
            };
            ExerciseInfo {
                id: e.id,
                name,
                // The bare base name is the movement family: variations share it.
                family: e.name,
                difficulty: e.difficulty,
                pattern: e.pattern,
                metric: e.metric,
                is_skill,
                is_power: e.power,
                warmup: e.warmup,
                equipment,
                groups: groups_by_ex.get(&e.id).cloned().unwrap_or_default(),
            }
        })
        .collect();

    // What each exercise can actually be loaded with here. A movement using *two*
    // dumbbells only gets half the discs per dumbbell, and can't use a fixed weight
    // you own one of — so this can't be a per-equipment answer. Empty = not
    // loadable, and the engine leaves the lift out rather than guessing a weight.
    let implements_by_ex: HashMap<i64, i32> = ex_repo::list(pool, false)
        .await?
        .into_iter()
        .map(|e| (e.id, e.implements))
        .collect();
    let mut exercise_loads: HashMap<i64, Vec<f64>> = HashMap::new();
    let mut short_kit: Vec<(i64, i32)> = Vec::new(); // (equipment, implements needed)
    let mut unweighted: Vec<i64> = Vec::new();
    for ex in &exercises {
        // Every metric that carries a weight, not just weighted *reps* — a carry is
        // loaded too, and leaving it out of this loop would give it no inventory,
        // which the engine correctly reads as "no honest load" and drops it.
        let loaded = matches!(ex.metric, Metric::WeightedReps | Metric::WeightedHold);
        if !loaded || ex.equipment.is_empty() {
            continue;
        }
        // Only kit that's actually *here* can be short of weights. Without this the
        // notice indicts every barbell and kettlebell in the catalog for having no
        // registered weights at a location that doesn't own one.
        let present = kit.as_ref().is_some_and(|k: &Kit| k.has_all(&ex.equipment));
        if !present {
            continue;
        }
        let implements = implements_by_ex.get(&ex.id).copied().unwrap_or(1).max(1);
        let mut loads: Vec<f64> = Vec::new();
        let load_bearing: Vec<i64> = ex
            .equipment
            .iter()
            .copied()
            .filter(|eq| bears_load.contains(eq))
            .collect();
        if load_bearing.is_empty() {
            // A weighted lift whose kit can't hold a weight: the catalog is wrong,
            // not the gym. Say so in the log rather than nagging the athlete about
            // kit they can do nothing with.
            tracing::warn!(
                exercise = %ex.name,
                "weighted lift declares no load-bearing equipment — left out"
            );
            continue;
        }
        for eq in &load_bearing {
            let Some(kit) = kit_loads.get(eq) else {
                // Present, but nothing registered to load it with.
                if !unweighted.contains(eq) {
                    unweighted.push(*eq);
                }
                continue;
            };
            let l = loads::loads_for(kit, implements as u32);
            // Loadable in principle, but not enough of it to go round (one handle
            // can't do a two-dumbbell press) — a different problem from no weights.
            if l.is_empty() && !short_kit.contains(&(*eq, implements)) {
                short_kit.push((*eq, implements));
            }
            loads.extend(l);
        }
        exercise_loads.insert(ex.id, loads);
    }
    let notices = kit_notices(&equipment_names, &mut unweighted, &mut short_kit);

    let groups = muscle_repo::groups(pool)
        .await?
        .into_iter()
        .map(|(id, name, region)| {
            Ok(GroupMeta {
                id,
                name,
                region: Region::from_db(&region)
                    .ok_or_else(|| anyhow!("unknown region {region:?}"))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(PacingContext {
        tz,
        settings,
        mode,
        days_per_week: s.days_per_week,
        emphasis: s.emphasis,
        exercises,
        groups,
        kit,
        exercise_loads,
        notices,
        equipment_names,
    })
}

/// Say what the coach had to leave out and why. A silent drop reads as a hole in
/// the plan; naming the kit turns it into something the athlete can fix.
fn kit_notices(
    names: &HashMap<i64, String>,
    unweighted: &mut [i64],
    short_kit: &mut [(i64, i32)],
) -> Vec<String> {
    let name_of = |id: &i64| names.get(id).cloned().unwrap_or_else(|| "equipment".into());
    let mut out = Vec::new();

    unweighted.sort_unstable();
    if !unweighted.is_empty() {
        let list: Vec<String> = unweighted.iter().map(name_of).collect();
        out.push(format!(
            "No weights registered here for {} — I've left its exercises out rather than guess a load.",
            list.join(", ")
        ));
    }

    short_kit.sort_unstable();
    for (eq, need) in short_kit.iter() {
        out.push(format!(
            "You'd need {need} × {} for some movements here — I've left those out.",
            name_of(eq)
        ));
    }
    out
}

/// Combine a context with a (local-tz) history slice + biometric readiness into
/// an engine input. Clones the catalog/group/inventory so the same context can
/// drive many verdicts (the back-test replays one per training day).
pub fn input_from(
    ctx: &PacingContext,
    history: Vec<SetRec>,
    last_set_at: Option<NaiveDateTime>,
    readiness: Option<Readiness>,
    readiness_history: HashMap<NaiveDate, Readiness>,
) -> PacingInput {
    PacingInput {
        mode: ctx.mode,
        days_per_week: ctx.days_per_week,
        emphasis: ctx.emphasis,
        exercises: ctx.exercises.clone(),
        history,
        last_set_at,
        settings: ctx.settings,
        groups: ctx.groups.clone(),
        kit: ctx.kit.clone(),
        exercise_loads: ctx.exercise_loads.clone(),
        equipment_names: ctx.equipment_names.clone(),
        notices: ctx.notices.clone(),
        readiness,
        readiness_history,
    }
}

/// The coach verdict for the user right now. `location_id` makes the suggestion
/// location-aware; the mode is the user's saved setting (the coach's brief, not
/// a per-call choice). `readiness` is the biometric recovery signal
/// (health-derived, best-effort — `None` when unavailable, and the engine
/// degrades gracefully).
pub async fn now(
    pool: &MySqlPool,
    user_id: &str,
    location_id: Option<i64>,
    readiness: Option<Readiness>,
    readiness_history: HashMap<NaiveDate, Readiness>,
) -> Result<PacingNow> {
    let ctx = context(pool, user_id, location_id).await?;
    let now_local = Utc::now().with_timezone(&ctx.tz).naive_local();
    let to_local = |utc: NaiveDateTime| {
        Utc.from_utc_datetime(&utc)
            .with_timezone(&ctx.tz)
            .naive_local()
    };

    // History over the trailing window (logged_at is UTC). Convert to local for
    // the engine's date/hour math.
    let since_utc = Utc::now().naive_utc() - Duration::weeks(HISTORY_WEEKS);
    let raw = workout_repo::list_since(pool, user_id, since_utc).await?;
    let history: Vec<SetRec> = raw
        .iter()
        .map(|w| SetRec {
            exercise_id: w.exercise_id,
            logged_at: to_local(w.logged_at),
            reps: w.reps,
            load_kg: w.load_kg,
            hold_s: w.hold_s,
            rpe: w.rpe,
        })
        .collect();
    let last_set_at = raw.iter().map(|w| w.logged_at).max().map(to_local);

    let inp = input_from(&ctx, history, last_set_at, readiness, readiness_history);
    Ok(engine::evaluate(&inp, now_local))
}
