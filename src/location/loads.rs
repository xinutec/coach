//! Turn a loadable bar (bar weight + owned plate sizes) into the discrete set of
//! total loads you can actually build on it — the same "here are the weights you
//! can load" contract the pacing engine already consumes for fixed free weights.
//! Pure + unit-tested; the repo calls this when feeding a barbell's loads to the
//! engine so snapping/progression step through real, loadable totals.

/// Plates go on both ends equally, so a reachable total is `bar + 2 * s` for any
/// per-side sum `s` buildable from the owned plate sizes (unlimited pairs
/// assumed — a home/office set rarely runs out mid-progression). With no plates
/// we fall back to the classic 1.25 kg plate (2.5 kg total step) so the bar still
/// has a sane increment rather than pegging at the empty-bar weight.
const DEFAULT_PLATE_KG: f64 = 1.25;
/// Cap the per-side sum we enumerate (kg). `bar + 2 * this` bounds the heaviest
/// suggested total well past anything realistic, keeping the set finite.
const MAX_PER_SIDE_KG: f64 = 150.0;
/// Work in centikilograms (integer) so plate arithmetic is exact and hashable.
const CENTI: f64 = 100.0;

/// Total loads (kg, ascending) buildable on a bar of weight `bar` with the given
/// plate sizes. Always includes the empty bar (`bar` itself) as the floor.
pub fn reachable_loads(bar: f64, plates: &[f64]) -> Vec<f64> {
    let plate_centi: Vec<i64> = {
        let src: &[f64] = if plates.is_empty() {
            &[DEFAULT_PLATE_KG]
        } else {
            plates
        };
        src.iter()
            .map(|p| (p * CENTI).round() as i64)
            .filter(|&p| p > 0)
            .collect()
    };
    let cap = (MAX_PER_SIDE_KG * CENTI) as usize;
    // Unbounded-knapsack reachability: which per-side sums (centikg) are buildable.
    let mut can = vec![false; cap + 1];
    can[0] = true;
    for &p in &plate_centi {
        let p = p as usize;
        for s in p..=cap {
            if can[s - p] {
                can[s] = true;
            }
        }
    }
    can.iter()
        .enumerate()
        .filter(|&(_, &ok)| ok)
        .map(|(s, _)| bar + 2.0 * (s as f64 / CENTI))
        .collect()
}
