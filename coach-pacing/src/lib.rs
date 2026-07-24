//! The pure pacing core: the training-day engine and every type it computes,
//! lifted out of the coach binary so it can be compiled `#![no_std]`.
//!
//! With `std` out of scope this crate *cannot* open a file, read the wall clock,
//! spawn a thread, or hold global mutable state — the impurity the engine must
//! never have is unrepresentable, enforced by the compiler rather than by review
//! or a lint. `engine::evaluate` takes `now` as a parameter; the std shell
//! (`coach`) reads the clock, talks to the DB, and threads the input in.
//!
//! The sole exception is the `ts` feature, which pulls in std + ts-rs to emit the
//! frontend TypeScript types (scripts/gen-types.sh). Production never enables it.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

/// no_std has no auto-imported alloc prelude, so every module globs this in for
/// the heap types it uses (`Vec`, `String`, `format!`, …). An unused glob does
/// not warn, so a file that happens to need only one of them stays clean.
pub(crate) mod prelude {
    pub(crate) use alloc::{
        boxed::Box,
        format,
        string::{String, ToString},
        vec,
        vec::Vec,
    };
}

pub mod domain;
pub mod health;
pub mod pacing;
