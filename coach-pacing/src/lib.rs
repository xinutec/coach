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
// --- totality ---------------------------------------------------------------
// `no_std` above makes the engine *pure* — it cannot reach the world. These deny
// the other half: a **total** function is defined for every input, so it may not
// bail out at runtime. Together they are what a Lean translation needs, and what
// lets the engine be trusted without one.
//
// Every finding these produced was a real representation weakness, and fixing
// them made the code shorter: an `Inventory` whose non-emptiness was a comment
// rather than its shape, a `filter(is_some)` re-asserted six lines later by
// `unwrap`, three parallel arrays that only needed to be one. Where a rule
// cannot be satisfied without contorting the code instead — `Index` has no total
// form to write — the exemption is local and says why (see `cover::ByGroup`).
//
// Deliberately NOT denied, since here they would cost clarity and buy nothing:
//   * `arithmetic_side_effects` — 43 sites, nearly all `chrono` durations and
//     small bounded `i32`s. Denying it means `checked_*` on ordinary arithmetic,
//     which buries the formulas this file exists to make legible.
//   * `integer_division` — the two sites are deliberate: a `(n + d - 1) / d`
//     ceiling and a `/ 7` days-to-weeks, both with constant non-zero divisors.
//     The lint is about accidental truncation, not division by zero.
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
pub mod pacing;
