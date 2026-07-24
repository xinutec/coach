//! The pacing engine's HTTP/DB surface. The engine itself and its types live in
//! the pure `coach-pacing` core (compiled no_std) and are re-exported here so the
//! rest of coach still says `crate::pacing::engine` / `crate::pacing::types`.
//! `service` is the std shell: it assembles the engine input from the DB and
//! applies the user's timezone.

pub mod service;

pub use coach_pacing::pacing::{ability, cover, dose, engine, readiness, residual, types};
