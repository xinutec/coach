//! The pure pacing core: the training-day engine and every type it computes, in
//! its own crate so it can be compiled `#![no_std]`.
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
// --- totality ---------------------------------------------------------------
// `no_std` above makes the engine *pure* — it cannot reach the world. These deny
// the other half: a **total** function is defined for every input, so it may not
// bail out at runtime. Together they are what a Lean translation needs, and what
// lets the engine be trusted without one.
//
// A finding is usually a representation weakness: fix the type, not the call
// site. Where a rule cannot be met without contorting the code — `Index` has no
// total form to write — the exemption is local and says why (see
// `cover::ByGroup`).
//
// Deliberately NOT denied, since here they would cost clarity and buy nothing:
//   * `arithmetic_side_effects` — nearly all `chrono` durations and small
//     bounded `i32`s. Denying it means `checked_*` on ordinary arithmetic, which
//     buries the formulas this crate exists to make legible.
//   * `integer_division` — the uses are deliberate ceilings and days-to-weeks
//     with constant non-zero divisors. The lint is about accidental truncation,
//     not division by zero.
//
// Casts that can lose a value are denied workspace-wide (Cargo.toml); `num` is
// where the engine converts, and says why each conversion is sound.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    clippy::exit,
    clippy::infinite_loop
)]

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
pub mod num;
pub mod pacing;
