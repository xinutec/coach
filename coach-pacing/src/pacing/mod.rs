//! The pure pacing engine. `engine::evaluate` is a total function of its input
//! and `now`; the DB assembly + timezone live in the std shell (`coach::pacing`).

pub mod ability;
pub mod cover;
pub mod dose;
pub mod engine;
pub mod readiness;
pub mod residual;
pub mod types;
