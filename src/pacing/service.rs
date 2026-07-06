//! Assemble the dynamic engine's input from the DB and run it. All timezone
//! handling lives here: `logged_at` is stored UTC, everything the engine sees is
//! the user's local tz. No program is loaded — the engine works off history +
//! the active mode.

use std::collections::HashSet;

use anyhow::{Result, anyhow};
use chrono::{Duration, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use sqlx::MySqlPool;

use crate::equipment::repo as equipment_repo;
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
    ExerciseInfo, GroupMeta, LastPerf, PacingInput, PacingNow, PacingSettings, Readiness, SetRec,
};

/// The coach verdict for the user right now. `location_id` makes the suggestion
/// location-aware; `mode_override` picks a mode for this call (else the user's
/// saved default). `readiness` is the biometric recovery signal (health-derived,
/// best-effort — `None` when unavailable, and the engine degrades gracefully).
pub async fn now(
    pool: &MySqlPool,
    user_id: &str,
    location_id: Option<i64>,
    mode_override: Option<Mode>,
    readiness: Option<Readiness>,
) -> Result<PacingNow> {
    let s = settings_repo::get(pool, user_id).await?;
    let tz: Tz = s.timezone.parse().unwrap_or(chrono_tz::Europe::London);
    let now_local = Utc::now().with_timezone(&tz).naive_local();
    let to_local =
        |utc: NaiveDateTime| Utc.from_utc_datetime(&utc).with_timezone(&tz).naive_local();

    let settings = PacingSettings {
        window_start_hour: s.window_start_hour,
        window_end_hour: s.window_end_hour,
        min_rest_min: s.min_rest_min,
    };
    let mode = mode_override.unwrap_or(s.mode);

    let available_equipment = match location_id {
        Some(id) => location_repo::equipment_ids(pool, user_id, id)
            .await?
            .map(|ids| ids.into_iter().collect::<HashSet<i64>>()),
        None => None,
    };
    // Discrete owned weights per equipment at this location (for load snapping).
    let equipment_loads = match location_id {
        Some(id) => location_repo::equipment_loads(pool, id).await?,
        None => std::collections::HashMap::new(),
    };

    // Exercise metadata: equipment ids, muscle-group contributions, skill flag.
    let equip_by_ex = ex_repo::equipment_by_exercise(pool).await?;
    let groups_by_ex = ex_repo::muscle_groups_by_exercise(pool).await?;
    let skill_equip: HashSet<i64> = equipment_repo::list(pool)
        .await?
        .into_iter()
        .filter(|e| e.slug == "gymnastic_rings" || e.slug == "parallettes")
        .map(|e| e.id)
        .collect();
    let exercises: Vec<ExerciseInfo> = ex_repo::list(pool, false)
        .await?
        .into_iter()
        .map(|e| {
            let equipment = equip_by_ex.get(&e.id).cloned().unwrap_or_default();
            let is_skill =
                e.metric == Metric::Hold || equipment.iter().any(|id| skill_equip.contains(id));
            ExerciseInfo {
                id: e.id,
                name: e.name,
                pattern: e.pattern,
                metric: e.metric,
                is_skill,
                equipment,
                groups: groups_by_ex.get(&e.id).cloned().unwrap_or_default(),
            }
        })
        .collect();

    // History over the trailing 8 weeks (logged_at is UTC). Convert to local for
    // the engine's date/hour math.
    let since_utc = Utc::now().naive_utc() - Duration::weeks(8);
    let raw = workout_repo::list_since(pool, user_id, since_utc).await?;
    let history: Vec<SetRec> = raw
        .iter()
        .map(|w| SetRec {
            exercise_id: w.exercise_id,
            logged_at: to_local(w.logged_at),
            reps: w.reps,
            load_kg: w.load_kg,
            hold_s: w.hold_s,
        })
        .collect();
    let last_set_at = raw.iter().map(|w| w.logged_at).max().map(to_local);

    let last_perf = workout_repo::last_performance_by_exercise(pool, user_id)
        .await?
        .into_iter()
        .map(|(id, lp)| {
            (
                id,
                LastPerf {
                    reps: lp.reps,
                    load_kg: lp.load_kg,
                    hold_s: lp.hold_s,
                },
            )
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

    let inp = PacingInput {
        mode,
        days_per_week: s.days_per_week,
        emphasis: s.emphasis,
        exercises,
        history,
        last_perf,
        last_set_at,
        settings,
        groups,
        available_equipment,
        equipment_loads,
        readiness,
    };
    Ok(engine::evaluate(&inp, now_local))
}
