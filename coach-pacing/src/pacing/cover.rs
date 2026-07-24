//! Session selection as **weighted set cover** — the algorithmic core of the plan.
//!
//! The domain truth an earlier group-loop had backwards: *one set of one exercise
//! credits many muscle groups at once* (primary 1.0, secondary 0.5, stabilizer
//! 0.25 — the muscle model). Walking the in-deficit groups and asking each one
//! "which exercise fills you?" therefore emitted the same exercise once per group
//! it happened to cover (dips appearing twice, for Chest and again for Triceps),
//! and left set counts to a separate deficit-share heuristic bolted on afterwards.
//!
//! Selection is instead a **coverage problem**: today's need is a vector over the
//! group space, one set of an exercise is a vector that pays part of it down, and
//! the day's set budget is a cardinality constraint. Maximising coverage under
//! that constraint is monotone submodular, so greedy marginal gain — repeatedly
//! take the set that pays down the most *remaining* need — is the standard
//! (1 − 1/e)-of-optimal algorithm, and it is deterministic.
//!
//! Three things stop being special cases and simply fall out:
//!
//! - **Duplicates are unrepresentable.** The accumulator is keyed by exercise, so
//!   "dips ×2" is one item with a count — which is what it always was.
//! - **Set counts are earned, not apportioned.** A second set of dips is worth
//!   less than a first row once the first already paid down chest and triceps,
//!   because [`ByGroup::saturating_sub`] clamps the need at zero. Diminishing
//!   returns is the clamp, not a rule.
//! - **Balance is a guarantee, not a hope.** Greedy's bound applies to the session
//!   the athlete actually gets.
//!
//! The vector is indexed by [`GroupIx`] — a dense index into the group space, not
//! a muscle-group *id* — so a group index and an exercise id cannot be confused,
//! and a dot product is a flat array walk.

use crate::prelude::*;

/// A dense index into the group space (`0..groups.len()`), assigned from the
/// group list's order. Distinct from a muscle-group *id* by type.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GroupIx(pub usize);

/// A dense vector over the muscle-group space. One allocation, O(1) indexing.
#[derive(Clone, Debug, PartialEq)]
pub struct ByGroup<T>(Box<[T]>);

impl<T> ByGroup<T> {
    /// One value per group, in group order — for values that aren't `Copy`
    /// (names, ids) so they can be indexed by [`GroupIx`] like everything else
    /// rather than by a bare `usize`.
    pub fn from_vec(values: Vec<T>) -> Self {
        ByGroup(values.into_boxed_slice())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T: Copy> ByGroup<T> {
    pub fn filled(len: usize, v: T) -> Self {
        ByGroup(vec![v; len].into_boxed_slice())
    }
    /// Every index paired with its value — the only way to enumerate, so the
    /// index type is never lost.
    pub fn iter(&self) -> impl Iterator<Item = (GroupIx, T)> + '_ {
        self.0.iter().enumerate().map(|(i, v)| (GroupIx(i), *v))
    }
}

// The two impls below are the *only* places this crate indexes a slice, and the
// only places it can panic on a bad index. `Index` has to return `&T`, so there
// is no total version of it to write — the alternative is not a safer operator
// but no operator, pushing an `unwrap` on to every one of the ~30 call sites.
//
// So the bound is discharged here instead: a `GroupIx` is only ever minted by
// `ByGroup::iter` or from `enumerate()` over the very group list these vectors
// are sized from, so `i.0 < len` holds for every value that can reach this code.
// That is a fact about provenance, which the lint cannot see and a reviewer can.
impl<T> core::ops::Index<GroupIx> for ByGroup<T> {
    type Output = T;
    #[allow(clippy::indexing_slicing, reason = "GroupIx is in range by provenance")]
    fn index(&self, i: GroupIx) -> &T {
        &self.0[i.0]
    }
}

impl<T> core::ops::IndexMut<GroupIx> for ByGroup<T> {
    #[allow(clippy::indexing_slicing, reason = "GroupIx is in range by provenance")]
    fn index_mut(&mut self, i: GroupIx) -> &mut T {
        &mut self.0[i.0]
    }
}

impl ByGroup<f64> {
    /// How much of `self` (the remaining need) one application of `credit` pays.
    pub fn dot(&self, credit: &ByGroup<f64>) -> f64 {
        self.0
            .iter()
            .zip(credit.0.iter())
            .map(|(n, c)| n * c)
            .sum::<f64>()
    }

    /// Pay `credit` down against the need, clamping at zero. The clamp *is* the
    /// diminishing-returns rule: need already met contributes nothing further.
    pub fn saturating_sub(&mut self, credit: &ByGroup<f64>) {
        for (n, c) in self.0.iter_mut().zip(credit.0.iter()) {
            *n = (*n - *c).max(0.0);
        }
    }
}

/// One selectable exercise: what a single set of it pays into each muscle group,
/// how well it suits the athlete's mode/novelty (a style preference, not a need),
/// and the most sets of it that belong in one session.
pub struct Candidate {
    /// Exercise id — carried only to break ties deterministically.
    pub id: i64,
    /// The movement family (the catalog's base name). Variations of one movement
    /// train the same thing the same way, so a session takes at most one entry
    /// per family — the second cousin is redundant stimulus wearing a different
    /// label, and its slot goes to whatever else still pays (R3-3).
    pub family: String,
    /// What ONE set pays into each group (role credit × that group's recovery).
    pub credit: ByGroup<f64>,
    /// Style preference: mode fit + novelty. Scales rank; never qualifies.
    pub weight: f64,
    /// A one-time **need** — in the same effective-set units as coverage — to bring
    /// a movement the athlete has *started but not yet confirmed* up to a trusted
    /// baseline. Added to the exercise's pay only on the set that *enters* it into
    /// the session, so it opens the gate (a just-trained group has ~0 coverage need,
    /// yet the movement is still worth repeating until its estimate is solid) without
    /// inflating later sets. Zero for a movement that is either never-done (there's
    /// nothing to confirm — that's novelty, covered by `credit`) or already trusted.
    pub confirm: f64,
    /// Never trained — a brand-new movement, subject to the per-session novelty cap
    /// so a calibration day introduces a few movements to learn, not a scattershot
    /// of one-off sets across everything at once.
    pub novel: bool,
    /// Fewest sets to take *once this exercise is picked at all* — the minimum
    /// effective dose. A movement worth setting up for is worth more than one set,
    /// so the cover commits rather than spreading the day thin across eight
    /// movements at a single set each. (A calibration set is the exception: `min`
    /// = `cap` = 1, because measuring the same thing twice tells you nothing new.)
    pub min: i32,
    /// Most sets of this exercise the session may take.
    pub cap: i32,
}

/// The least *genuine need* — in effective sets — a set must pay down to earn a
/// place in the session. Below half an effective set, the group is essentially at
/// target and the stimulus isn't worth the slot; the coach would rather hand back
/// a short session than pad it with work the athlete doesn't need.
///
/// Deliberately gated on the **pay**, not on `pay × weight`: style (mode fit,
/// novelty) may *rank* candidates, but it must never *qualify* one. Otherwise a
/// merely fashionable exercise clears the bar on a group that's already done.
/// [`Candidate::confirm`] is counted into the pay here on purpose — knowing what
/// you can do on a movement you've started *is* a need, not a style, so it may
/// qualify a pick the same way coverage does.
pub const MIN_PAY: f64 = 0.5;

/// Float ties within this are treated as equal, so the id tie-break (not
/// accumulated rounding) decides — the verdict must be byte-identical run to run.
const EPS: f64 = 1e-9;

/// One chosen exercise: its index in `cands`, the sets it earned, and the need
/// (in effective sets) its *first* set paid down — the number it was judged on,
/// carried through to the athlete-facing explanation.
pub struct Chosen {
    pub index: usize,
    pub sets: i32,
    /// Coverage need its first set paid down (effective sets) — the *volume* it
    /// contributes. Excludes the confirmation bonus, so the explanation stays
    /// truthful about how much of the week's group deficit this actually pays.
    pub pays: f64,
    /// This pick earned its place by confirming a baseline, not by paying down
    /// volume — its coverage `pays` was below the bar and [`Candidate::confirm`]
    /// carried it in. The reason the coach gives for it differs accordingly.
    pub confirming: bool,
}

/// Greedily fill `budget` sets from `cands`, each time taking the set that pays
/// down the most *remaining* need. Returns one [`Chosen`] per exercise, in the
/// order they were first picked. Stops early when nothing left clears
/// [`MIN_PAY`].
///
/// Two needs qualify a pick, both in effective-set units: **coverage** (this set's
/// dot with the remaining group need) and, only on the set that first enters an
/// exercise, its **confirmation** need ([`Candidate::confirm`]) — the value of
/// turning a started-but-unproven movement into a trusted baseline. Coverage is
/// what gets *paid down* (subtracted from `need`); confirmation just opens the door
/// and is spent once. `novelty_cap` bounds how many never-done movements a single
/// session introduces, so a calibration day is a few movements learned properly,
/// not a scattershot of one-off sets.
///
/// Deterministic: ties break to the lower exercise id.
pub fn select(
    cands: &[Candidate],
    need: &ByGroup<f64>,
    budget: i32,
    novelty_cap: i32,
) -> Vec<Chosen> {
    let mut need = need.clone();
    // The picks so far, in the order they were first taken — both the working
    // state and the result. One structure keyed by nothing but its own order,
    // rather than parallel arrays indexed by candidate, so there is no set of
    // vectors to keep in step (and no index to get wrong).
    let mut picked: Vec<Chosen> = Vec::new();
    let mut left = budget.max(0);
    // Never-done movements introduced so far — bounded by `novelty_cap`.
    let mut novel_taken = 0i32;
    // Movement families already in the session — each admits one entry (R3-3).
    let mut families: alloc::collections::BTreeSet<&str> = alloc::collections::BTreeSet::new();

    // Every round commits at least one set, so the budget bounds the rounds.
    // Saying that as the loop's own range makes termination structural — bounded
    // by a number fixed before the first iteration. The equivalent `while left >
    // 0` terminated only because `Candidate::min` happens to be >= 1: a candidate
    // offering a zero-set dose would have left `left` unchanged and spun forever,
    // and nothing in the types said otherwise.
    for _ in 0..budget.max(0) {
        if left == 0 {
            break;
        }

        // The greedy step: whichever next set pays down the most remaining need.
        let mut best: Option<Pick> = None;
        for (index, cand) in cands.iter().enumerate() {
            let taken = sets_taken(&picked, index);
            if taken >= cand.cap {
                continue;
            }
            let entering = taken == 0;
            // A fresh novel movement past the cap doesn't get to start — but one
            // already in the session may still earn further sets on coverage.
            if entering && cand.novel && novel_taken >= novelty_cap {
                continue;
            }
            // A cousin of a movement already picked is redundant stimulus, not
            // variety — one entry per family per session.
            if entering && families.contains(cand.family.as_str()) {
                continue;
            }
            // Entering a movement means committing to its full minimum dose; a
            // budget remainder too small for that must not start it ("Push-up —
            // 1 set", the round-3 orphan). The spare set instead tops up a
            // movement already in the session — re-ranked below like any other —
            // or goes honestly unspent.
            if entering && left < cand.min.min(cand.cap) {
                continue;
            }
            // Coverage is what this set pays into remaining group need. Confirmation
            // is a one-time entry need — it qualifies a first set, nothing after.
            let cover = need.dot(&cand.credit);
            let pay = cover + if entering { cand.confirm } else { 0.0 };
            if pay < MIN_PAY {
                continue;
            }
            // Style breaks the tie between things that all genuinely need doing.
            let rank = pay * cand.weight;
            let wins = match &best {
                None => true,
                Some(b) => {
                    rank > b.rank + EPS || ((rank - b.rank).abs() <= EPS && cand.id < b.cand.id)
                }
            };
            if wins {
                best = Some(Pick {
                    index,
                    cand,
                    cover,
                    pay,
                    rank,
                });
            }
        }

        let Some(pick) = best else {
            break;
        };
        let cand = pick.cand;
        let take = match picked.iter_mut().find(|p| p.index == pick.index) {
            // Already in the session: one more set, its marginal gain having just
            // been re-checked against everything else.
            Some(entry) => {
                let take = 1.min(cand.cap - entry.sets).min(left);
                entry.sets += take;
                take
            }
            // Entering: commit to the minimum effective dose rather than spreading
            // the day thin across movements at a single set each.
            None => {
                let take = cand.min.min(cand.cap).min(left);
                families.insert(cand.family.as_str());
                if cand.novel {
                    novel_taken += 1;
                }
                picked.push(Chosen {
                    index: pick.index,
                    sets: take,
                    pays: pick.cover,
                    // It earned its place on confirmation, not volume, when coverage
                    // alone couldn't have cleared the bar. That's the reason the
                    // coach will give.
                    confirming: pick.cover < MIN_PAY && pick.pay >= MIN_PAY,
                });
                take
            }
        };

        for _ in 0..take {
            need.saturating_sub(&cand.credit);
        }
        left -= take;
    }

    picked
}

/// The winning candidate of one greedy round.
struct Pick<'a> {
    /// Position in the caller's candidate slice — carried through to [`Chosen`].
    index: usize,
    cand: &'a Candidate,
    /// What this set pays into the remaining group need.
    cover: f64,
    /// `cover`, plus the confirmation need on a set that enters the exercise.
    pay: f64,
    /// `pay` scaled by style preference — what the round maximises.
    rank: f64,
}

/// Sets already committed to the candidate at `index`, or 0 if it isn't in the
/// session yet. Linear in the picks, of which there are at most `budget`.
fn sets_taken(picked: &[Chosen], index: usize) -> i32 {
    picked
        .iter()
        .find(|p| p.index == index)
        .map_or(0, |p| p.sets)
}
