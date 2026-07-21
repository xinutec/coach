//! What the athlete actually owns to load a movement with.
//!
//! The prescription side already snaps every suggested load to a weight you own
//! ([`loads::loads_for`]). This asks the mirror question of a set you *logged*:
//! is this a weight you could have built at all? A logged load far beyond
//! anything you own is the signature of a mistyped field, and the ability model
//! is a max over history — so one such number becomes a PR the engine cannot
//! unlearn (it decays to a 60 % floor and the block reset only fires on an
//! 8-week gap, which never comes while you keep training).

use std::collections::HashSet;

use anyhow::Result;
use sqlx::MySqlPool;

use crate::equipment::repo as equipment_repo;
use crate::exercise::repo as ex_repo;
use crate::exercise::types::{Exercise, Metric};
use crate::location::{loads, repo as location_repo};

/// The heaviest load `exercise` can be built to with the weights this user owns,
/// across **every** location they have.
///
/// The max over all locations, not today's gym, because a logged set carries no
/// location: the narrow question ("could you build this *here*?") cannot be
/// asked honestly of the data, and asking it anyway would cry typo every time he
/// trains somewhere better equipped.
///
/// `None` = we cannot say — an unloaded movement, no locations, or no weights
/// registered for the kit it needs. Then there is nothing to check against, and
/// silence must not be read as "implausible": an athlete who has registered no
/// weights is under-configured, not lying.
pub async fn heaviest_buildable(
    pool: &MySqlPool,
    user_id: &str,
    exercise: &Exercise,
) -> Result<Option<f64>> {
    if !matches!(exercise.metric, Metric::WeightedReps | Metric::WeightedHold) {
        return Ok(None);
    }
    let equip_by_ex = ex_repo::equipment_by_exercise(pool).await?;
    let Some(ex_equipment) = equip_by_ex.get(&exercise.id) else {
        return Ok(None);
    };
    // The catalog's `weighted` flag, not a guess from the category — the same
    // rule the engine uses (a cable stack is a machine that certainly bears load).
    let bears_load: HashSet<i64> = equipment_repo::list(pool)
        .await?
        .into_iter()
        .filter(|e| e.weighted)
        .map(|e| e.id)
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

/// How far past the heaviest weight you own a logged load may sit before it is
/// worth asking about. Half again heavier than anything you own is not a plate
/// you improvised with — a borrowed bell or an unregistered gym plate lands
/// within a fraction, while the digit slip this guards against (40 → 140) lands
/// far outside. Generous on purpose: this interrupts an honest athlete, so it
/// must only fire on a number that describes nothing they could have lifted.
pub const IMPLAUSIBLE_FACTOR: f64 = 1.5;

/// Is `load` so far beyond the athlete's heaviest owned weight that it is worth
/// one confirmation? `heaviest` of `None` (nothing registered) always answers no
/// — an unknown rack is not evidence of a typo.
pub fn implausible(load: f64, heaviest: Option<f64>) -> bool {
    heaviest.is_some_and(|h| h > 0.0 && load > h * IMPLAUSIBLE_FACTOR)
}
