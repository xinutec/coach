//! The numeric conversions coach makes, named so each says once why it
//! is sound rather than a bare `as` saying nothing at every call site.

/// A count as the `i32` the wire and the schema use. Saturating: nothing here
/// counts 2³¹ of anything, and if it did the largest count is the honest answer,
/// not a wrapped negative one.
pub fn count(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// A whole number held as `f64` — already floored or rounded — as an `i32`.
/// Rust's float-to-int conversion saturates at the bounds and maps NaN to 0, so
/// there is no undefined value; the fraction is gone by the time it gets here.
#[allow(
    clippy::cast_possible_truncation,
    reason = "saturating by definition; callers round first"
)]
pub fn whole(x: f64) -> i32 {
    x as i32
}

/// The same for a size or an index: negative and NaN become 0.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "saturating by definition; callers round first"
)]
pub fn natural(x: f64) -> usize {
    x as usize
}
