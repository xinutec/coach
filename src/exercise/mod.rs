//! Exercise catalog: a global library imported from the training base (seeded at
//! boot by `crate::seed` from data/catalog/), plus user-added custom movements.
//! An exercise carries its muscles and required equipment (M:N); movement
//! `pattern` doubles as the recovery grouping used by the pacing engine.

pub mod image;
pub mod repo;
pub mod types;
