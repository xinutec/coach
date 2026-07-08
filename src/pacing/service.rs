//! Assemble the dynamic engine's input from the DB and run it. All timezone
//! handling lives here: `logged_at` is stored UTC, everything the engine sees is
//! the user's local tz. No program is loaded — the engine works off history +
//! the active mode.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use chrono::{Duration, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use sqlx::MySqlPool;

use crate::exercise::repo as ex_repo;
use crate::exercise::types::Metric;
use crate::location::repo as location_repo;
use crate::muscle::repo as muscle_repo;
use crate::muscle::types::Region;
use crate::settings::repo as settings_repo;
use crate::settings::types::Mode;
use crate::workout::repo as workout_repo;

use super::engine;
use super::types::{
    ExerciseInfo, GroupMeta, PacingInput, PacingNow, PacingSettings, Readiness, SetRec,
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
    pub available_equipment: Option<HashSet<i64>>,
    pub equipment_loads: HashMap<i64, Vec<f64>>,
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

    let available_equipment = match location_id {
        Some(id) => location_repo::equipment_ids(pool, user_id, id)
            .await?
            .map(|ids| ids.into_iter().collect::<HashSet<i64>>()),
        None => None,
    };
    // Discrete owned weights per equipment at this location (for load snapping).
    let equipment_loads = match location_id {
        Some(id) => location_repo::equipment_loads(pool, id).await?,
        None => HashMap::new(),
    };

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
                None => e.name,
            };
            ExerciseInfo {
                id: e.id,
                name,
                pattern: e.pattern,
                metric: e.metric,
                is_skill,
                warmup: e.warmup,
                equipment,
                groups: groups_by_ex.get(&e.id).cloned().unwrap_or_default(),
            }
        })
        .collect();

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
        available_equipment,
        equipment_loads,
    })
}

/// Combine a context with a (local-tz) history slice + biometric readiness into
/// an engine input. Clones the catalog/group/inventory so the same context can
/// drive many verdicts (the back-test replays one per training day).
pub fn input_from(
    ctx: &PacingContext,
    history: Vec<SetRec>,
    last_set_at: Option<NaiveDateTime>,
    readiness: Option<Readiness>,
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
        available_equipment: ctx.available_equipment.clone(),
        equipment_loads: ctx.equipment_loads.clone(),
        readiness,
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

    let inp = input_from(&ctx, history, last_set_at, readiness);
    Ok(engine::evaluate(&inp, now_local))
}
