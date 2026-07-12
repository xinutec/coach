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

/// A dense index into the group space (`0..groups.len()`), assigned from the
/// group list's order. Distinct from a muscle-group *id* by type.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GroupIx(pub usize);

/// A dense vector over the muscle-group space. One allocation, O(1) indexing.
#[derive(Clone, Debug, PartialEq)]
pub struct ByGroup<T>(Box<[T]>);

impl<T: Copy> ByGroup<T> {
    pub fn filled(len: usize, v: T) -> Self {
        ByGroup(vec![v; len].into_boxed_slice())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Every index paired with its value — the only way to enumerate, so the
    /// index type is never lost.
    pub fn iter(&self) -> impl Iterator<Item = (GroupIx, T)> + '_ {
        self.0.iter().enumerate().map(|(i, v)| (GroupIx(i), *v))
    }
}

impl<T> std::ops::Index<GroupIx> for ByGroup<T> {
    type Output = T;
    fn index(&self, i: GroupIx) -> &T {
        &self.0[i.0]
    }
}

impl<T> std::ops::IndexMut<GroupIx> for ByGroup<T> {
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
    /// What ONE set pays into each group (role credit × that group's recovery).
    pub credit: ByGroup<f64>,
    /// Style preference: mode fit + novelty. Scales rank; never qualifies.
    pub weight: f64,
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
    pub pays: f64,
}

/// Greedily fill `budget` sets from `cands`, each time taking the set that pays
/// down the most *remaining* need. Returns one [`Chosen`] per exercise, in the
/// order they were first picked. Stops early when nothing left clears
/// [`MIN_PAY`].
///
/// Deterministic: ties break to the lower exercise id.
pub fn select(cands: &[Candidate], need: &ByGroup<f64>, budget: i32) -> Vec<Chosen> {
    let mut need = need.clone();
    let mut sets = vec![0i32; cands.len()];
    let mut first_pay = vec![0.0f64; cands.len()];
    let mut order: Vec<usize> = Vec::new();
    let mut left = budget.max(0);

    while left > 0 {
        let mut best: Option<(usize, f64, f64)> = None; // (index, pay, rank)
        for (i, c) in cands.iter().enumerate() {
            if sets[i] >= c.cap {
                continue;
            }
            // What this set actually pays down, in effective sets. The gate.
            let pay = need.dot(&c.credit);
            if pay < MIN_PAY {
                continue;
            }
            // Style breaks the tie between things that all genuinely need doing.
            let rank = pay * c.weight;
            let wins = match best {
                None => true,
                Some((bi, _, br)) => {
                    rank > br + EPS || ((rank - br).abs() <= EPS && c.id < cands[bi].id)
                }
            };
            if wins {
                best = Some((i, pay, rank));
            }
        }
        let Some((i, pay, _)) = best else { break };
        let c = &cands[i];
        // Committing to a movement takes its minimum dose; adding to one already in
        // the session takes a single set (its marginal gain was just re-checked).
        let take = if sets[i] == 0 {
            order.push(i);
            first_pay[i] = pay;
            c.min.min(c.cap)
        } else {
            1
        }
        .min(c.cap - sets[i])
        .min(left);

        for _ in 0..take {
            need.saturating_sub(&c.credit);
        }
        sets[i] += take;
        left -= take;
    }

    order
        .into_iter()
        .map(|i| Chosen {
            index: i,
            sets: sets[i],
            pays: first_pay[i],
        })
        .collect()
}
