//! Training locations: a per-user place defined by the equipment available
//! there (home, office gym, a hotel room = nothing). The pacing engine uses the
//! selected location to decide what's doable and to substitute when a goal's kit
//! is missing.

pub mod loads;
pub mod owned;
pub mod repo;
pub mod types;
