//! Equipment catalog queries (global). SQL as `&'static str` literals.

use anyhow::Result;
use sqlx::MySqlPool;

use super::types::{Equipment, EquipmentRow};

/// The columns [`EquipmentRow`] is built from, in one place because two modules
/// select it: here, and the exercise detail (which joins through
/// `exercise_equipment`). They were written out twice, and when the row grew a
/// `loadable` field only this one was updated — so every exercise *with* equipment
/// 500'd on `no column found for name: loadable`. A `FromRow` struct binds by
/// name at runtime, so a column list that drifts from it is a bug the compiler
/// cannot see. Sharing the fragment is what makes the drift impossible.
///
/// Every query using this must alias the equipment table as `eq`.
macro_rules! eq_cols {
    () => {
        "eq.id, eq.slug, eq.name, eq.category, eq.loadable"
    };
}
pub(crate) use eq_cols;

pub async fn list(pool: &MySqlPool) -> Result<Vec<Equipment>> {
    sqlx::query_as::<_, EquipmentRow>(concat!(
        "SELECT ",
        eq_cols!(),
        " FROM equipment eq ORDER BY eq.category, eq.name"
    ))
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Equipment::try_from)
    .collect()
}
