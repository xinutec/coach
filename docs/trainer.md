# The trainer model — design + roadmap

Where the coach engine is, where it falls short of the goal, and the design that
closes each gap. The goal, in one line: a **deterministic personal trainer** —
no ML, every number derivable from logged history by a pure, unit-tested
function — that presents **today's plan in order**, **knows your current
abilities**, **asks (via calibration tasks) when it doesn't**, and gets finer
the more you log.

Principles that already hold and must keep holding:

- **Stateless**: the verdict is `evaluate(history, settings, now)` — no stored
  plan, nothing to drift out of sync. Logging a set changes the next verdict.
- **Labelled heuristics**: every coefficient is a named constant with a comment
  saying what it is and why (`src/pacing/engine.rs` top). Tunable, not magic.
- **Anchored to your own history**, not absolute landmarks; population numbers
  only as cold-start anchors.
- **Degrade gracefully**: missing data (no biometrics, no location, no history)
  narrows the verdict, never breaks it.

## What exists today

Module docs are the reference (`src/pacing/*.rs`); in brief: rolling 7-day
volume per muscle group vs a target blended from a literature anchor and your
own 8-week average, tilted by mode/emphasis/days-per-week; a 36 h recovery gate
per group; biometric readiness (sleep/HRV/RHR from health-sync) scaling volume
and gating progression; one greedy suggestion — best exercise for the biggest
recovered deficit, doable at the current location, loads snapped to owned
weights — progressed by double progression off the last top set; a burn-down
nudge that spreads sets through the day.

## Gaps and designs

### G1 — There is no ability model, only "the last top set"

**Gap.** `LastPerf` is the top set of the most recent session, however old.
After 18 months off, the engine happily prescribes your 2024 number +1 rep.
RPE is logged but read by nothing. A never-done exercise gets "the lightest
weight you own" and the bottom of the rep range — a guess, not an estimate.
This is the root gap: a trainer that doesn't know what you can do today can't
plan today.

**Design.** A pure `ability(history, now) -> HashMap<ExerciseId, Ability>`:

- **Estimate per metric.** `weighted_reps`: estimated 1RM per set via Epley,
  RPE-aware — `e1rm = load × (1 + (reps + rir)/30)` with `rir = 10 − rpe`
  (missing RPE → rir 0, i.e. the set is taken at face value); the exercise's
  raw ability is the max over the recent window. `reps`: best single-set reps,
  same RPE adjustment. `hold`: best hold seconds.
- **Staleness decay** (heuristic, labelled): full trust ≤ 2 weeks idle on that
  exercise, then −1.5 %/week, floored at 60 % — the detraining curve: strength
  holds for a couple of weeks, then erodes. The decayed value is the *working
  ability* every prescription derives from.
- **Confidence**: `High` ≥ 3 sessions of the exercise in the last 6 weeks,
  `Medium` 1–2, `Low` only-stale data, `None` never done. Confidence — not
  cold-start defaults — decides whether the engine prescribes or assesses (G3).
- **Cross-exercise prior** (later stage): `None`-confidence exercises inherit a
  first estimate from a sibling (same pattern + same primary group) via a fixed
  variation-ratio table, so a first session on an incline press doesn't start
  from zero when the flat press is known.

Prescription then comes **from ability, not from the last set**: pick the load
matching the mode's rep range as a %-of-e1RM (inverse Epley), snapped to owned
weights. With fresh, consistent data this reduces to today's double progression
(+1 rep / next owned weight); with gaps, misses, or high RPE it self-corrects
instead of blindly bumping.

**Provable**: pure function, table-driven tests — stale history decays, RPE 10
counts less than RPE 7 at the same load, a 2024-only history never prescribes
above its decayed floor.

### G2 — One suggestion is not a plan, and there's no ordering

**Gap.** The engine emits a single "next up". You can't see today's session, in
what order, or what's left after this set. There is no warm-up concept at all.

**Design.** The engine builds an ordered `plan: Vec<PlanItem>` for the rest of
the day, still recomputed statelessly on every call (logging a set shifts the
plan; the old "next up" is simply the first outstanding item):

- **Selection**: today's `day_target_sets` budget distributed over the top
  recovered-deficit groups (sets per item proportional to deficit, min 2 —
  replacing today's flat `sets = 3`), each group resolved to a location-doable
  exercise exactly as now.
- **Ordering** (classic, deterministic rules): ① warm-up block (G2a) →
  ② skill/hold work while the nervous system is fresh → ③ heavy compound
  weighted work, biggest deficit first → ④ bodyweight/isolation accessories →
  ⑤ core/conditioning finishers. Within a tier: deficit desc, id asc. Adjacent
  items sharing a primary group are swapped apart when possible, so rest for
  one group is work for another.
- **G2a Warm-up block**, derived from the plan itself: the union of the main
  items' primary muscle groups must be covered by the block. Catalog moves
  tagged `warmup: true` (mobility, band work — see G5) are chosen to cover
  that set; the first heavy weighted item additionally gets a ramp-in set at
  ~50 % of its working load. Warm-up sets credit no training volume, so they
  never eat the day's targets. **Provable**: a unit test asserts
  `cover(warmup_block) ⊇ primaries(main_items)` for arbitrary plans.
- **Wire**: `PacingNow.plan: Vec<PlanItem>` where `PlanItem` = kind
  (`Warmup | Work | Assess`) + the current `Suggestion` fields. The Today page
  renders the ordered list as a checklist; the existing single suggestion card
  becomes its head.

### G3 — When the engine doesn't know, it should ask

**Gap.** Unknown or stale ability is papered over with defaults instead of
being measured.

**Design.** When the exercise chosen for a plan slot has ability confidence
`Low`/`None`, the item is emitted as kind `Assess` with a calibration protocol
per metric — and the assessment *is* the training (it's still sets for that
group, no wasted day):

- weighted: "work up to a hard-but-clean set of ~5, log load/reps/RPE";
- reps: "one AMRAP set, stop at form breakdown";
- hold: "one max hold".

No new tables: the logged set is the measurement — ability (G1) recomputes from
history, so the very next verdict prescribes from it. Re-assessment triggers
automatically: staleness pushes confidence down over time, and (later stage)
persistent prediction error — you keep beating or missing prescriptions —
forces a re-measure. This is the "gets finer over time" loop: measure →
prescribe → observe → correct, all deterministic.

### G4 — Progression ignores how the sets actually went

**Gap.** `progress()` always advances: missed reps still get +1 next time, a
grinding RPE-10 set is treated like an easy one. Plateaus are invisible.

**Design.** Feedback-aware progression rules on top of the ability model:

- top-of-range at RPE ≤ 7 → step load (as now, but allowed a double step when
  the e1RM says the next owned weight is still < the target intensity);
- missed the rep floor or RPE ≥ 9.5 → repeat, don't bump;
- two consecutive misses on an exercise → back off ~10 % (one deterministic
  step down the owned-weights ladder) and rebuild;
- plateau — no e1RM improvement over 4 weeks at `High` confidence → suggest the
  next-best variation for the group (the recency term already rotates novelty;
  this makes it explicit and explains itself in the reason line).

### G5 — Catalog data isn't rich enough to drive the above

**Gap.** Only 2 of 119 catalog entries are de-facto warm-up moves, and nothing
marks them as such — today they'd credit volume like any set. `unilateral` is
stored but unused (a flat `sets = 3` means half the volume per side).

**Design.** Bounded catalog curation: add `warmup: true` to suitable mobility /
band / activation moves (and add the few missing ones needed to cover all 7
regions); seeder reconciliation (hash-gated, already in place) carries it to
the DB. Engine: warm-up-tagged exercises are excluded from balance targets and
selectable only by the warm-up block; unilateral exercises count sets per side.

### G6 — Recovery is a binary gate

**Gap.** ≥ 3 effective sets within 36 h blocks a group outright, the same for
delts as for quads. Real recovery is graded and size-dependent.

**Design** (refinement, last stage): replace the boolean with a recovery
fraction per group — linear ramp over a per-region recovery horizon (larger
regions recover slower; labelled per-region constants) — and scale the group's
deficit by it. The gate falls out as the fraction-≈0 case; behaviour with a
fully-recovered group is unchanged (regression-tested).

## Staging

Each stage ships alone and keeps every existing test green.

1. **Ability model (G1)** — pure `ability.rs`, staleness + RPE, prescriptions
   derived from it. Kills the stale-PR bug; biggest correctness win.
2. **Assessment items (G3)** — needs only confidence from stage 1 plus the
   `Assess` kind on the wire.
3. **Session plan + ordering (G2, sans warm-up)** — engine emits the ordered
   plan, Today renders the checklist, set counts sized to deficits.
4. **Warm-up block + catalog curation (G2a, G5)**.
5. **Feedback progression + plateau (G4)**, **graded recovery (G6)**,
   **cross-exercise priors (G1 tail)** — independent refinements, any order.
