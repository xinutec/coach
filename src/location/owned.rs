//! What the athlete owns to load a movement with, to check a *logged* load against: a
//! load far beyond anything buildable is a mistyped field, and since ability is a max
//! it would become a PR the engine cannot unlearn.

use std::collections::HashSet;

use anyhow::Result;
use sqlx::MySqlPool;

use crate::equipment::repo as equipment_repo;
use crate::exercise::repo as ex_repo;
use crate::exercise::types::Exercise;
use crate::location::{loads, repo as location_repo};
use coach_pacing::domain::{EquipmentId, ExerciseId};

/// The heaviest load `exercise` can be built to with this user's weights, across
/// **every** location, since a logged set carries no location. `None` when there is
/// nothing to check against (an unloaded movement, no locations, no registered
/// weights): an under-configured athlete is not lying.
pub async fn heaviest_buildable(
    pool: &MySqlPool,
    user_id: &str,
    exercise: &Exercise,
) -> Result<Option<f64>> {
    if !exercise.metric.takes_load() {
        return Ok(None);
    }
    let equip_by_ex = ex_repo::equipment_by_exercise(pool).await?;
    let Some(ex_equipment) = equip_by_ex.get(&ExerciseId(exercise.id)) else {
        return Ok(None);
    };
    // The catalog's `weighted` flag, not a guess from the category — the same
    // rule the engine uses (a cable stack is a machine that certainly bears load).
    let bears_load: HashSet<EquipmentId> = equipment_repo::list(pool)
        .await?
        .into_iter()
        .filter(|e| e.weighted)
        .map(|e| EquipmentId(e.id))
        .collect();
    let implements = u32::try_from(exercise.implements).unwrap_or(1).max(1);

    let mut heaviest: Option<f64> = None;
    for loc in location_repo::list(pool, user_id).await? {
        let kit_loads = location_repo::kit_loads(pool, loc.id).await?;
        for eq in ex_equipment.iter().filter(|e| bears_load.contains(e)) {
            let Some(kit) = kit_loads.get(eq) else {
                continue;
            };
            // Ascending, so the last is the heaviest this kit can be built to.
            if let Some(top) = loads::loads_for(kit, implements).last().copied() {
                heaviest = Some(heaviest.map_or(top, |h: f64| h.max(top)));
            }
        }
    }
    Ok(heaviest)
}

/// How far past the heaviest owned weight a logged load may sit before it is queried:
/// half again. An improvised weight lands within a fraction; a digit slip (40 → 140)
/// lands far outside.
pub const IMPLAUSIBLE_FACTOR: f64 = 1.5;

/// Is `load` so far beyond the athlete's heaviest owned weight that it is worth
/// one confirmation? `heaviest` of `None` (nothing registered) always answers no
/// — an unknown rack is not evidence of a typo.
pub fn implausible(load: f64, heaviest: Option<f64>) -> bool {
    heaviest.is_some_and(|h| h > 0.0 && load > h * IMPLAUSIBLE_FACTOR)
}
