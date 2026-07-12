//! Turn a loadable implement (a barbell, or an adjustable dumbbell handle) into
//! the discrete set of total loads you can actually build on **one** of them — the
//! "here are the weights you can load" contract the pacing engine consumes. Pure +
//! unit-tested; the service calls it per exercise, so snapping and progression only
//! ever step through totals the athlete can physically assemble.
//!
//! Four physical facts, none of which the old unlimited-plates model captured:
//!
//! - **Plates load in pairs.** A bar or dumbbell loaded unevenly isn't a lighter
//!   lift, it's a wrist injury — there's no unbalanced case worth modelling. A
//!   total is `implement + 2 × (per-side sum)`, and a disc size is only usable if
//!   you own two of them.
//! - **You own a finite number of discs.** With one pair of 2.5s, 2.5-per-side is
//!   reachable and 5-per-side is not. Suggesting a weight the athlete can't build
//!   is the same class of bug as inventing a load for kit with no weights at all.
//! - **A pair of dumbbells splits the disc budget.** Four of each disc is *two*
//!   per dumbbell when the movement needs two — so a both-arms press tops out far
//!   below what the same discs reach on a single goblet-squat dumbbell. This is why
//!   loads are computed per *exercise* (which knows how many implements it uses),
//!   not per equipment.
//! - **A sleeve has finite space.** Past `slots` discs a side, nothing more fits,
//!   however many you own.
//!
//! `qty: None` / `slots: None` mean "plenty" — a gym rack, the pre-0016 assumption.

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
/// Cap the per-side sum we enumerate (kg). `implement + 2 * this` bounds the
/// heaviest suggested total well past anything realistic, keeping the set finite.
const MAX_PER_SIDE_KG: f64 = 150.0;
/// Work in centikilograms (integer) so plate arithmetic is exact and hashable.
const CENTI: f64 = 100.0;

/// Total loads (kg, ascending) buildable on **one** implement of weight
/// `implement`, when the movement uses `implements` of them (2 = a pair of
/// dumbbells, which halves each disc size's budget per dumbbell) and each sleeve
/// takes at most `slots` discs.
///
/// Always includes the bare implement as the floor. Empty when the discs can't
/// even be shared out — but the bare implement alone is always buildable, so the
/// result is never empty.
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
            let kg = (p.kg * CENTI).round() as i64;
            if kg <= 0 {
                return None;
            }
            let pairs = match p.qty {
                Some(q) => q / implements / 2,
                None => u32::MAX, // plenty
            };
            (pairs > 0).then_some((kg as usize, pairs))
        })
        .collect();

    let cap = (MAX_PER_SIDE_KG * CENTI) as usize;
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
        let usable = (cap / size) as u32;
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

/// The loads buildable for a movement that uses `implements` of this kit — one
/// dumbbell (a goblet squat) or two (a bench press). Ascending, deduped.
///
/// A fixed 5 kg dumbbell serves a one-dumbbell movement but not a two-dumbbell one
/// unless you own two of them; a pair of adjustable handles splits the disc budget
/// between them. Both sources union, so an athlete with a fixed 5 kg *and* an
/// adjustable handle gets both sets of weights.
///
/// Empty = this kit cannot be loaded for this movement at all (no weights
/// registered, or not enough implements to go round), and the caller must not
/// prescribe it rather than guessing a load.
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
