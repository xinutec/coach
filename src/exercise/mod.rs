//! Exercise catalog: a global library imported from the training base (seeded at
//! boot by `crate::seed` from data/catalog/), plus user-added custom movements.
//! An exercise carries its muscles and required equipment (M:N) and a movement
//! `pattern` (classification only; recovery is tracked per muscle group).

pub mod animation;
pub mod image;
pub mod repo;
pub mod types;
