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
recovered deficit, doable at the current location; a burn-down nudge that
spreads sets through the day. Prescription derives from the **ability model**
(G1, shipped): an RPE-aware, staleness-decayed e1RM estimate per exercise, from
which the working load is autoregulated and snapped to the weights you own.

## Gaps and designs

### G1 — There is no ability model, only "the last top set"

**Gap.** `LastPerf` is the top set of the most recent session, however old.
After 18 months off, the engine happily prescribes your 2024 number +1 rep.
Worse, that "top set" may never have happened: it's assembled from independent
per-column maxima over the last day trained (`MAX(reps)`, `MAX(load_kg)` in
`last_performance_by_exercise`) — log 10×20 kg and 5×40 kg in one session and
the progression basis is a fictitious 10×40 kg, which double progression then
tries to beat. RPE is logged but read by nothing. A never-done exercise gets
"the lightest weight you own" and the bottom of the rep range — a guess, not an
estimate. This is the root gap: a trainer that doesn't know what you can do
today can't plan today.

**Design** — *shipped* as `pacing/ability.rs`, a pure
`abilities(history, now) -> HashMap<ExerciseId, Ability>`:

- **Estimate per metric.** `weighted_reps`: estimated 1RM per set via Epley,
  RPE-aware — `e1rm = load × (1 + (reps + rir)/30)` with `rir = 10 − rpe`
  (missing RPE → rir 0, i.e. the set is taken at face value). `reps`: best
  effective reps (`reps + rir`). `hold`: best hold seconds.
- **Per-set staleness decay** (heuristic, labelled): each set's estimate is
  scaled by *its own* age — full trust ≤ 2 weeks, then −1.5 %/week, floored at
  60 % (the detraining curve) — and the exercise's ability is the **max of the
  decayed estimates**. Decaying per set *then* maxing (rather than max-then-decay)
  makes ability provably monotone under idleness while still trusting a genuine
  old PR down to the floor rather than forgetting it.
- **Confidence**: `High` ≥ 3 sessions of the exercise in the last 6 weeks
  (a session = a distinct local day with ≥ 1 set of it), `Medium` 1–2, `Low`
  only-stale data, `None` never done. Confidence — not cold-start defaults —
  decides whether the engine prescribes or assesses (G3, next stage).
- **Cross-exercise prior** (later stage): `None`-confidence exercises inherit a
  first estimate from a sibling (same pattern + same primary group) via a fixed
  variation-ratio table, so a first session on an incline press doesn't start
  from zero when the flat press is known.

Prescription comes **from ability, not from the last set** (shipped, `engine::prescribe`):
the working load is derived from the decayed e1RM — the weight whose top-of-range
reps the estimate supports (inverse Epley at `TARGET_RIR` reserve) — then snapped
to the **nearest weight you own**. This is *autoregulated load*: a layoff decays
the estimate and eases the start automatically, and low readiness adds reserve
(a lighter day). Because the target snaps to discrete owned weights, it **earns**
the classic double-progression step: reps climb to the top of the range at the
current weight, and the load only moves up once logged sets raise the e1RM past
the next owned plate — never a blind +2.5 kg the reps don't support. Bodyweight
`reps` work climbs the range off the decayed best; `hold` work off the best hold.

Ability derives from **per-set history, not a `LastPerf` roll-up** — the old
column-wise `MAX(reps), MAX(load)` (the chimera) is deleted; the estimate maxes
over *real* sets. The service now loads 26 weeks of history (was 8) so the decay
curve sees a returning athlete's recent-ish PRs; a set older than that simply
doesn't inform today's estimate, and an exercise with no recent set falls to
`None`/`Low` confidence rather than being bumped from an ancient number.

**Proven** (pure tests, `tests/ability.rs` + `tests/pacing_engine.rs`): stale
history decays and floors, RPE 10 counts less than RPE 7 at the same load,
ability is monotone under idleness, a 10×20 kg + 5×40 kg day never yields a
10×40 kg basis, a fresh top set is prescribed at demonstrated capacity (no blind
jump), a stronger history earns a heavier *owned* weight, a 200-day-stale PR is
prescribed below its old weight, and low readiness never prescribes heavier than
a good day.

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

**Gap.** Stage 1 made prescription RPE-aware (a grinding set no longer inflates
the estimate), but the ability estimate is a **max** over decayed sets, so a
*miss* pulls nothing down — a bad day is silently ignored rather than answered.
There's no back-off after repeated misses and no plateau detection, and
`prescribe` still creeps holds +5 s and bodyweight reps +1 every advance with no
feedback other than the range ceiling.

**Design.** Feedback-aware progression rules on top of the ability model:

- top-of-range at RPE ≤ 7 → step load (allowed a double step when the e1RM says
  the next owned weight is still < the target intensity);
- missed the rep floor or RPE ≥ 9.5 → repeat, don't bump;
- two consecutive misses on an exercise → back off ~10 % (one deterministic
  step down the owned-weights ladder) and rebuild;
- plateau — no e1RM improvement over 4 weeks at `High` confidence → suggest the
  next-best variation for the group (the recency term already rotates novelty;
  this makes it explicit and explains itself in the reason line).

### G5 — Catalog data isn't rich enough to drive the above

**Gap.** Only a handful of the 119 catalog entries are de-facto warm-up moves
(arm circles, shoulder dislocates, wrist stretches, band work) and nothing
marks them as such — today they'd credit volume like any set. `unilateral` is
stored but unused (a flat `sets = 3` means half the volume per side).
`difficulty` is wired through schema and API but null on **every** entry and
read by nothing (G7 needs it). And "skill" isn't catalog data at all — the
service infers it from hardcoded equipment slugs (`gymnastic_rings`,
`parallettes` in `pacing/service.rs`), a magic-string classification the
catalog should own.

**Design.** Bounded catalog curation: add `warmup: true` to suitable mobility /
band / activation moves (and add the few missing ones needed to cover all 7
regions); populate `difficulty` (1–5, relative within a pattern — drives G7);
add `skill: true` where it belongs and drop the slug sniff. Seeder
reconciliation (hash-gated, already in place) carries it all to the DB. Engine:
warm-up-tagged exercises are excluded from balance targets and selectable only
by the warm-up block; unilateral exercises count sets per side.

### G6 — Recovery is a binary gate

**Gap.** ≥ 3 effective sets within 36 h blocks a group outright, the same for
delts as for quads. Real recovery is graded and size-dependent.

Related: the biometric `recovery_scale` reaches the per-group targets but not
`day_target_sets`, so on a low-readiness day the burn-down still demands the
same number of sets — just lighter ones.

**Design** (refinement, last stage): replace the boolean with a recovery
fraction per group — linear ramp over a per-region recovery horizon (larger
regions recover slower; labelled per-region constants) — and scale the group's
deficit by it. The gate falls out as the fraction-≈0 case; behaviour with a
fully-recovered group is unchanged (regression-tested). Scale `day_target_sets`
by the same recovery factor as the group targets.

### G7 — Bodyweight and hold work dead-end at the top of the range

**Gap.** A weighted move that tops its rep range earns the next owned weight
(the e1RM ratchet). But a `reps` move that tops its range is prescribed the top
**forever** — `prescribe` has no next step — and holds just creep (G4). There
is no notion of one exercise being the harder variation of another, so the
engine can never say "you've outgrown incline push-ups; do full push-ups".

**Design.** Deterministic variation ladders, driven by the curated `difficulty`
field (G5): when an exercise is topped out — top of range at `High` confidence
(and, once G4 lands, at RPE ≤ 8) — the planner offers the next-harder catalog
entry sharing the pattern + primary group as an explicit "level up" item, its
first prescription seeded by the cross-exercise prior (G1 tail). Nothing harder
doable at the location → hold top of range and say so in the reason line.
**Provable**: a topped-out incline push-up with a harder press variant
available yields the variant; without one, it holds.

## Engineering rigor — how we know it's right

The gaps above are the product design. These are the mechanisms that make
"provably correct" and "gets finer over time" *verified properties* rather than
aspirations. All of them exist because the engine is a pure function — none
would be possible with a stored plan.

### E1 — Back-test every engine change against real history

Any change to the engine (a constant, a formula, a new stage) can be **replayed
over the full real logged history**: evaluate the verdict at each historical
instant (every log event, plus sampled hours between), before vs after, and
diff. A small `backtest` binary + committed baseline, exactly the health-sync
golden pattern: fixtures from real data, gate = no *unexplained* drift. No
heuristic gets tuned blind; every diff in prescriptions is inspected before it
ships. This is the single highest-leverage piece — it turns "I think this
constant is better" into evidence.

### E2 — Property tests for the invariants

Table tests pin behaviours; **property tests** (`proptest`, arbitrary histories)
pin the *invariants* that must hold for every input, not just the ones we
thought of:

- determinism: same input → byte-identical verdict;
- logging a set never *increases* that group's deficit;
- suggested loads ∈ the owned-weights set (when known); reps within the mode's
  range; targets within their clamps;
- ability is monotone under decay — more idle time never raises it;
- warm-up cover ⊇ the plan's primary groups (G2a); plan set-count ≤ the day
  budget;
- degradation: stripping any optional input (readiness, location, history)
  yields a verdict, never a panic, and never *widens* claims (e.g. no load
  suggestion appears when the inventory vanished).

### E3 — Athlete simulation: convergence as a regression test

A deterministic **virtual athlete** (a simple dose-response model: true
ability per exercise, performs the prescription with an outcome derived from
true ability, fatigue, and a fixed recovery curve — all closed-form, seeded, no
randomness needed) run against the engine for simulated months. Assertions:

- **convergence**: after N assessment/prescription cycles, prescription error
  vs true ability falls below a threshold and stays there;
- **stability**: prescriptions don't oscillate (no ping-ponging between loads);
- **bounded ramp**: weekly volume growth stays under a labelled cap;
- **recovery honesty**: the simulated athlete is never prescribed work the
  model says it cannot recover from.

This is what makes "becomes a close-to-perfect trainer over time" a tested
property of the system instead of a hope.

### E4 — Prediction-error ledger (the self-correction signal)

Every prescription is a **prediction** ("you can do 8 × 40 kg"). The logged
outcome makes the residual computable from history alone — still stateless.
Surfaced per exercise, it (a) drives G3 re-assessment (persistent misses →
confidence drops → re-measure), and (b) later calibrates the labelled constants
*per user*: pick, from a small labelled grid of candidate constants (decay
rate, progression step), the one minimising historical residual. Deterministic,
reproducible, inspectable — calibration, not learning: the model form never
changes, only which labelled constant is active, and E1 shows exactly what the
switch does.

### E5 — Explanations as data, not prose

The `reason` string becomes a **structured trace**: each factor (deficit,
recovery state, readiness, recency, mode fit, ability + its confidence + its
staleness decay) with its value and contribution to the decision. The UI can
then cite the derivation of every number it shows, the prose renders *from* the
trace, and tests assert on the trace instead of string-matching sentences. Also
the debugging story: "why did it tell me to do X?" is answerable exactly.

## Staging

Each stage ships alone and keeps every existing test green.

1. **Ability model (G1)** — ✅ *shipped*. Pure `ability.rs` (staleness + RPE),
   `engine::prescribe` derives the autoregulated load from it, `LastPerf` and
   the chimera query deleted. Killed the stale-PR and chimera-top-set bugs.
2. **Assessment items (G3)** — needs only confidence from stage 1 plus the
   `Assess` kind on the wire.
3. **Session plan + ordering (G2, sans warm-up)** — engine emits the ordered
   plan, Today renders the checklist, set counts sized to deficits.
4. **Warm-up block + catalog curation (G2a, G5)** — warm-up tags, difficulty,
   skill flag, unilateral handling.
5. **Feedback progression + plateau (G4)**, **variation ladders (G7)**,
   **graded recovery (G6)**, **cross-exercise priors (G1 tail)** — independent
   refinements, any order.

Rigor lands alongside, not after: **E1 (back-test) + E2 (properties) arrive
with stage 1** and gate every later stage; **E5 (trace)** rides stage 3 (the
plan needs explaining anyway); **E3 (simulation)** follows stage 3, once there
is a plan to simulate; **E4 (residual ledger)** rides stages 2–5 — assessment
uses it first, calibration last.
