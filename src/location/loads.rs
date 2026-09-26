//! The discrete total loads you can build on **one** loadable implement (a barbell or
//! an adjustable dumbbell handle), computed per exercise so progression only steps
//! through weights the athlete can assemble:
//!
//! - **Plates load in pairs:** a total is `implement + 2 × per-side sum`, and a disc
//!   size is usable only if you own two.
//! - **Discs are finite:** one pair of 2.5s reaches 2.5 per side, not 5.
//! - **A pair of dumbbells splits the discs,** so loads are per *exercise* (which knows
//!   how many implements it uses), not per equipment.
//! - **A sleeve is finite:** past `slots` discs a side, nothing more fits.
//!
//! `qty: None` / `slots: None` mean "plenty", as in a gym.

use coach_pacing::num::natural;

/// A plate size you own, and how many discs of it — *in total*, across all the
/// implements that share the pool. `None` = plenty (a gym rack).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plate {
    pub kg: f64,
    pub qty: Option<u32>,
}

/// With no plates entered we fall back to the classic 1.25 kg plate (a 2.5 kg
/// total step) so a bar still has a sane increment rather than pegging at the
/// empty-bar weight.
const DEFAULT_PLATE_KG: f64 = 1.25;
/// Work in centikilograms (integer) so plate arithmetic is exact and hashable.
const CENTI: f64 = 100.0;
/// Cap the per-side sum we enumerate: 150 kg, in centikilograms. `implement + 2 *
/// this` bounds the heaviest suggested total well past anything realistic,
/// keeping the set finite.
const MAX_PER_SIDE_CENTI: usize = 15_000;

/// Total loads (kg, ascending) buildable on one implement of weight `implement`, for a
/// movement using `implements` of them, with at most `slots` discs a sleeve. Never
/// empty: the bare implement is always buildable.
pub fn reachable_loads(
    implement: f64,
    plates: &[Plate],
    implements: u32,
    slots: Option<u32>,
) -> Vec<f64> {
    let implements = implements.max(1);
    let fallback = [Plate {
        kg: DEFAULT_PLATE_KG,
        qty: None,
    }];
    let src: &[Plate] = if plates.is_empty() { &fallback } else { plates };

    // Per side, per implement: how many discs of each size can actually go on.
    // Owning `q` discs of a size means `q / implements` reach each dumbbell of the
    // pair, and of those only whole pairs (`/ 2`) can be loaded symmetrically.
    let per_side: Vec<(usize, u32)> = src
        .iter()
        .filter_map(|p| {
            let kg = natural((p.kg * CENTI).round());
            if kg == 0 {
                return None;
            }
            let pairs = match p.qty {
                Some(q) => q / implements / 2,
                None => u32::MAX, // plenty
            };
            (pairs > 0).then_some((kg, pairs))
        })
        .collect();

    let cap = MAX_PER_SIDE_CENTI;
    // Bounded knapsack, minimising *discs used* per reachable per-side sum: a sum
    // is only really buildable if some combination reaching it also fits in the
    // sleeve's slots, and the disc-minimal combination is the one most likely to.
    const UNREACHABLE: u32 = u32::MAX;
    let mut discs = vec![UNREACHABLE; cap + 1];
    discs[0] = 0;
    for &(size, pairs) in &per_side {
        // More pairs than fit under the cap can never be used, so the "plenty" case
        // (u32::MAX) collapses to the same bound — and the unbounded knapsack is a
        // single forward pass rather than a loop over four billion phantom discs.
        let usable = u32::try_from(cap / size).unwrap_or(u32::MAX);
        if pairs >= usable {
            for s in size..=cap {
                if discs[s - size] != UNREACHABLE && discs[s - size] + 1 < discs[s] {
                    discs[s] = discs[s - size] + 1;
                }
            }
        } else {
            // Bounded: one pair at a time, backwards, so a pair isn't reused within
            // this pass. The counts are tiny (you own four discs, not four hundred).
            for _ in 0..pairs {
                for s in (size..=cap).rev() {
                    if discs[s - size] != UNREACHABLE && discs[s - size] + 1 < discs[s] {
                        discs[s] = discs[s - size] + 1;
                    }
                }
            }
        }
    }

    let slots = slots.unwrap_or(u32::MAX);
    discs
        .iter()
        .enumerate()
        .filter(|&(_, &d)| d != UNREACHABLE && d <= slots)
        .map(|(s, _)| implement + 2.0 * (s as f64 / CENTI))
        .collect()
}

/// A loadable implement at a location: its own weight, how many you own, and how
/// many discs fit on each sleeve.
#[derive(Clone, Copy, Debug)]
pub struct Bar {
    pub kg: f64,
    /// How many of this bar/handle you own. `None` = plenty.
    pub qty: Option<u32>,
    /// Discs that fit on one sleeve. `None` = unlimited.
    pub slots: Option<u32>,
}

/// Everything at a location that can put weight on one piece of kit: the fixed
/// weights you own of it (with how many of each), the loadable bar/handle if it is
/// one, and the plates that fit it.
#[derive(Clone, Debug, Default)]
pub struct KitLoads {
    /// Fixed free weights: `(kg, how many you own)`. `None` = plenty.
    pub fixed: Vec<(f64, Option<u32>)>,
    pub bar: Option<Bar>,
    pub plates: Vec<Plate>,
}

/// The loads for a movement using `implements` of this kit, ascending and deduped:
/// fixed weights you own enough of, plus what the handles can build from the discs.
/// Empty means this kit can't be loaded for this movement, and the caller must not
/// prescribe it.
pub fn loads_for(kit: &KitLoads, implements: u32) -> Vec<f64> {
    let implements = implements.max(1);
    let enough = |owned: Option<u32>| owned.is_none_or(|q| q >= implements);

    let mut out: Vec<f64> = kit
        .fixed
        .iter()
        .filter(|(_, qty)| enough(*qty))
        .map(|(kg, _)| *kg)
        .collect();

    if let Some(bar) = kit.bar
        && enough(bar.qty)
    {
        out.extend(reachable_loads(bar.kg, &kit.plates, implements, bar.slots));
    }

    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    out
}
