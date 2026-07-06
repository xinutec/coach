//! coach backend library. The binary (`src/main.rs`) is a thin wrapper; tests
//! live in `tests/` and exercise this public surface.

pub mod config;
pub mod db;
pub mod equipment;
pub mod error;
pub mod exercise;
pub mod import;
pub mod location;
pub mod muscle;
pub mod nextcloud;
pub mod pacing;
pub mod program;
pub mod routes;
pub mod seed;
pub mod session;
pub mod settings;
pub mod state;
pub mod workout;
