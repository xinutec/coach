//! Session selection as **weighted set cover**. One set of one exercise credits many
//! muscle groups at once (primary 1.0, secondary 0.5, stabilizer 0.25), so today's need
//! is a vector over the groups, a set is a vector paying part of it down, and the day's
//! budget bounds the count. Coverage is monotone submodular: greedy marginal gain is
//! deterministic and (1 − 1/e)-optimal.
//!
//! What falls out:
//!
//! - **No duplicates:** the accumulator is keyed by exercise, so "dips ×2" is one item.
//! - **Earned set counts:** [`ByGroup::saturating_sub`] clamps need at zero, so a
//!   second set is worth less once the first paid its groups down.
//! - **Balance:** greedy's bound applies to the session the athlete gets.
//!
//! [`GroupIx`] is a dense index into the group list, not a group id, so the two cannot
//! be confused.

use crate::prelude::*;

use crate::domain::ExerciseId;

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

// The only slice indexing in this crate. `Index` must return `&T`, so it has no total
// form; the bound holds by provenance instead: a `GroupIx` is only minted by
// `ByGroup::iter` or by enumerating the group list these vectors are sized from.
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
    pub id: ExerciseId,
    /// The movement family (the catalog's base name). Variations of one movement
    /// train the same thing the same way, so a session takes at most one entry
    /// per family — the second cousin is redundant stimulus wearing a different
    /// label, and its slot goes to whatever else still pays (R3-3).
    pub family: String,
    /// What ONE set pays into each group (role credit × that group's recovery).
    pub credit: ByGroup<f64>,
    /// Style preference: mode fit + novelty. Scales rank; never qualifies.
    pub weight: f64,
    /// A one-time need, in effective sets, to bring a started but unconfirmed movement
    /// to a trusted baseline. Counted only on the set that enters it into the session,
    /// so it opens the gate for a just-trained group without inflating later sets. Zero
    /// for never-done movements (novelty, priced by `credit`) and trusted ones.
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

/// The least genuine need, in effective sets, a set must pay down to earn a place;
/// below half a set, a short session beats padding. Gated on the **pay**, not `pay ×
/// weight`: style may rank candidates but never qualify one. [`Candidate::confirm`]
/// counts, since firming up a started movement is a need.
pub const MIN_PAY: f64 = 0.5;

/// Float ties within this are treated as equal, so the id tie-break (not
/// accumulated rounding) decides — the verdict must be byte-identical run to run.
const EPS: f64 = 1e-9;

/// What the cover needs to know about something it can pick. The cover ranks the
/// caller's own type and hands it back, so no second vector or index travels beside it.
pub trait Ranked {
    fn candidate(&self) -> &Candidate;
}

/// A bare candidate ranks as itself, so a caller with nothing extra to carry —
/// and every test — needs no wrapper.
impl Ranked for Candidate {
    fn candidate(&self) -> &Candidate {
        self
    }
}

/// One chosen exercise: the item that won, the sets it earned, and the need
/// (in effective sets) its *first* set paid down — the number it was judged on,
/// carried through to the athlete-facing explanation.
pub struct Chosen<'a, T> {
    /// The picked item, handed straight back to whoever supplied it.
    pub item: &'a T,
    pub sets: i32,
    /// Coverage need its first set paid down (effective sets) — the *volume* it
    /// contributes. Excludes the confirmation bonus, so the explanation stays
    /// truthful about how much of the week's group deficit this actually pays.
    pub pays: f64,
    /// This pick earned its place by confirming a baseline, not by paying down
    /// volume — its coverage `pays` was below the bar and [`Candidate::confirm`]
    /// carried it in. The reason the coach gives for it differs accordingly.
    pub confirming: bool,
    /// Which slice position this pick came from. Private, and deliberately so: it
    /// is identity while the loop runs, not part of what a pick *means*.
    index: usize,
}

/// Greedily fill `budget` sets from `cands`, each time taking the set that pays down
/// the most remaining need; one [`Chosen`] per exercise, in first-picked order. Stops
/// when nothing clears [`MIN_PAY`]. A pick qualifies on **coverage** (paid down from
/// `need`) or, on the set that enters it, on its one-time **confirmation** need.
/// `novelty_cap` bounds never-done movements per session. Ties break to the lower
/// exercise id.
pub fn select<'a, T: Ranked>(
    cands: &'a [T],
    need: &ByGroup<f64>,
    budget: i32,
    novelty_cap: i32,
) -> Vec<Chosen<'a, T>> {
    let mut need = need.clone();
    // The picks so far, in the order they were first taken — both the working
    // state and the result. One structure keyed by nothing but its own order,
    // rather than parallel arrays indexed by candidate, so there is no set of
    // vectors to keep in step (and no index to get wrong).
    let mut picked: Vec<Chosen<'a, T>> = Vec::new();
    let mut left = budget.max(0);
    // Never-done movements introduced so far — bounded by `novelty_cap`.
    let mut novel_taken = 0i32;
    // Movement families already in the session — each admits one entry (R3-3).
    let mut families: alloc::collections::BTreeSet<&str> = alloc::collections::BTreeSet::new();

    // The budget bounds the rounds, since every round commits at least one set; as the
    // loop's range, termination doesn't rest on `Candidate::min` being at least 1.
    for _ in 0..budget.max(0) {
        if left == 0 {
            break;
        }

        // The greedy step: whichever next set pays down the most remaining need.
        let mut best: Option<Pick<T>> = None;
        for (index, item) in cands.iter().enumerate() {
            let cand = item.candidate();
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
            // 1 set"). The spare set instead tops up a
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
                    item,
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
                    item: pick.item,
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
struct Pick<'a, T> {
    /// Position in the caller's slice — identity for the round-to-round
    /// bookkeeping, and nothing more.
    index: usize,
    /// The caller's item, carried through to [`Chosen`].
    item: &'a T,
    /// `item`'s scoring facts, read once per round rather than at every use.
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
fn sets_taken<T>(picked: &[Chosen<'_, T>], index: usize) -> i32 {
    picked
        .iter()
        .find(|p| p.index == index)
        .map_or(0, |p| p.sets)
}
