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
- **Degrade gracefully — and narrowly**: missing data (no biometrics, no location,
  no history) narrows the verdict, never breaks it, and *never widens it*. Absent
  information must not read as permission: no location means the engine doesn't
  know what's doable, so it declines to plan and asks — it does not fall back to
  "everything is doable" (see G8, the bug that spelling caused).
- **Illegal states unrepresentable** (2026-07-12): the safety rules are carried by
  types, not by care. A load can only come from a measured ability
  ([`Known`](../src/pacing/dose.rs)) and can only be a weight you own
  ([`Inventory`], non-empty by construction); a weighted lift *has* a load
  ([`Dose`], a sum type, not a tuple of five `Option`s). "When I don't know, I
  measure" is then a property the compiler enforces on every future edit, rather
  than a code path someone can bypass.
- **The athlete reports what happened, not how it felt** (agreed early; written
  down 2026-07-14, after it was lost — see below). This is a constraint on the
  *interface*, not on the engine. The loop is a human trainer's: **the coach gives
  an instruction, the athlete tries, the athlete records the result, and the system
  computes better numbers from the growing log — so the next instruction is
  sharper.** What the athlete records is an observation: reps, load, seconds. He is
  never asked to rate his own exertion out of ten.

  How the engine gets its accuracy is an *implementation* choice and deliberately
  unconstrained — use RPE, don't use RPE, infer effort from the residual between
  prediction and outcome (E4), whatever meets the standard. The rule is only that
  the burden does not land on the athlete. Self-rated effort is hard to judge, and
  hardest exactly when you are re-learning your own body after an illness — which
  is when this app is most needed and least able to check the answer.

  So RPE is **read if history carries one** (the imported 2024 sets do) and **never
  solicited**. Nothing breaks without it: a calibration set says "as many clean reps
  as you can, stop at form breakdown", so `rir = 0` is *true by construction*, and
  working sets progress by double progression, which needs reps and load. A missing
  RPE biases the estimate **downward** — conservative, never permissive — and the
  next session corrects it.

  *Agreed in an earlier session, never written here, and therefore lost at a context
  boundary — after which I spent a session asking him for RPEs he had already told
  me not to ask for. A decision that lives only in a conversation is a decision that
  will be un-made.*

- **The UI is the trainer's voice, not its dashboard** (2026-07-08): Today shows
  only what's needed to do the next set — one status line, the coach's one
  sentence (readiness/deload woven in server-side), the ordered plan, a log
  button. Engine internals surface on demand ("Why this?"), analysis lives in
  Balance, knobs in Settings — the mode is a stored setting (the coach's
  standing brief), not a per-visit question. Every element must pass: *does the
  user need this to train right now?*

## What exists today

Module docs are the reference (`src/pacing/*.rs`); in brief: rolling 7-day
volume per muscle group vs a target blended from a literature anchor and your
own 8-week average, tilted by mode/emphasis/days-per-week; a graded per-region
recovery ramp (G6) scaling each group's priority; biometric readiness
(sleep/HRV/RHR from health-sync) scaling volume + the day's set count and gating
progression; a burn-down nudge that spreads sets through the day. Prescription
derives from the **ability model** (G1, shipped): an RPE-aware,
staleness-decayed e1RM estimate per exercise, from which the working load is
autoregulated and snapped to the weights you own.

The verdict is a **session**, not a single suggestion (G8, shipped): a greedy
weighted set-cover of the day's muscle-group need, so each exercise appears once
with the set count it earned, ordered by training tier. It opens with a warm-up
block — mobility drills for the groups the session trains, plus a half-load
ramp-in set of the first weighted lift — which credits no volume and is drawn
only from catalog moves tagged `warmup`. Where the engine can't prescribe, it
says so rather than quietly narrowing the plan:

- an exercise whose ability estimate is untrusted becomes a **calibration** item
  (G3) — the logged set *is* the measurement;
- kit that's present but has no registered weights yields no honest load, so
  those lifts are dropped and **named in a notice** rather than guessed at;
- a group whose top-ranked movement is genuinely blocked carries a
  **substitution** naming the ideal and the blocker (absent kit, or kit with no
  weights). It is only ever set when that movement is actually blocked — the
  cover routinely picks something other than a group's best exercise, and
  reporting *that* as missing kit told the athlete things were absent that were
  standing in front of him.

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
  decayed estimates** *within the most-recent training block*. Decaying per set
  *then* maxing (rather than max-then-decay) makes ability provably monotone under
  idleness while still trusting a genuine old PR down to the floor.
- **Training-block reset** (safety, labelled `BLOCK_GAP_WEEKS = 8`): a break in an
  exercise's history longer than eight weeks splits it into a new block, and only
  the **most-recent block** estimates ability. So after a real interruption — a
  long layoff, a health setback — your current level is read from your *return*,
  never from a pre-break PR (even decayed) that no longer describes you: the
  estimate can't sit above what you've shown since, so a recovering body is never
  prescribed its old loads. Continuous training is one block (unchanged
  behaviour), and same-day sets never split (the chimera guard holds). Proven in
  `tests/ability.rs` — a long break resets to the return level; a light set
  *within* a block never erases a heavier one — and back-tested as a no-op on
  continuous history.
- **Confidence**: `High` ≥ 3 sessions of the exercise in the last 6 weeks
  (a session = a distinct local day with ≥ 1 set of it), `Medium` 1–2, `Low`
  only-stale data, `None` never done. Confidence — not cold-start defaults —
  decides whether the engine prescribes or assesses (G3, next stage).
- **Cross-exercise prior** (attempted, shelved — *blocked on `difficulty` data*):
  the intent — a `None`-confidence exercise inherits a first estimate from a
  sibling (same pattern + same primary group), so a first incline press doesn't
  start from zero when the flat press is known. A first cut (a flat 0.85
  variation discount over any pattern+group sibling, prescribing when the sibling
  was `High`) was built and immediately **back-tested against the real history
  (E1)** — which caught it producing unsafe/degenerate prescriptions: a *62 kg
  Good morning* prescribed as work off the RDL estimate (a never-performed,
  higher-risk movement at 85 % of a different lift), and a *0 kg Farmers walk*
  (a sibling with a zero-load entry). Root cause: a flat ratio across
  loosely-defined siblings doesn't capture that RDL→Good-morning or
  deadlift→farmers-walk share a group but *not* a strength/risk profile — that
  distinction lives in the unpopulated `difficulty` field (G5/G7). And
  auto-prescribing a weight for a never-performed movement violates the "measure
  when unsure" principle regardless. **Reverted**; correct sequence is: populate
  `difficulty` (G5 tail) → derive real variation ratios → and even then, a prior
  only *seeds the assessment's starting load*, never skips the assessment. E1
  earned its keep on its first real use.

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
<!-- Superseded in part by G8: selection + sizing are now a greedy set-cover, not
     a group loop with a deficit-share split. Ordering (the tier rule) stands. -->


**Gap.** The engine emits a single "next up". You can't see today's session, in
what order, or what's left after this set. There is no warm-up concept at all.

**Design.** The engine builds an ordered `plan: Vec<Suggestion>` for the day,
recomputed statelessly on every call (logging a set shifts the plan; the old
"next up" is simply the head). **Shipped** (`engine::build_plan`), except the
warm-up block (G2a), which waits on catalog curation (G5, next stage):

- **Selection** ✅ — *superseded by G8*. This shipped as a per-group loop with the
  `day_target_sets` budget apportioned by deficit share, which is what produced the
  duplicate items; selection and sizing are now a greedy set-cover over the group
  need vector. Read G8 for the current design.
- **Ordering** ✅ (classic, deterministic tiers, `engine::tier`): ① warm-up
  block (G2a, pending) → ② skill/hold work while the nervous system is fresh →
  ③ heavy compound weighted work → ④ bodyweight/isolation accessories →
  ⑤ core/conditioning finishers. Within a tier: deficit desc, id asc.
  (Not yet: swapping adjacent same-group items apart — a later refinement.)
- **G2a Warm-up block** ✅ (shipped, `engine::build_warmup`), derived from the
  plan: mobility drills (catalog `warmup: true` moves — see G5) for the session's
  primary muscle groups, one per group so drills don't stack, doable at the
  location; plus a ~50 % ramp-in set on the first heavy lift. Warm-up sets credit
  no training volume, so they never eat the day's targets, and warm-up-tagged
  moves are excluded from work selection. The block leads the plan (tier 1);
  `suggestion` (the nudge/Android head) points at the first *training* item, not
  the warm-up. (The current cover rule is "one drill per session group that has a
  warm-up move" + the ramp-in; a strict `cover ⊇ primaries` for *every* region
  awaits catalog mobility moves for legs/arms — the ramp-in covers those for now.)
- **Wire** ✅: `PacingNow.plan: Vec<Suggestion>` (reusing `Suggestion` + its
  `SuggestionKind`, now `Warmup | Work | Assess`). The Today page renders the
  ordered list as the session; `suggestion` stays as its head for the nudge
  + Android trigger.

### G3 — When the engine doesn't know, it should ask ✅ *shipped*

**Gap.** Unknown or stale ability was papered over with defaults instead of
being measured.

**Design** (shipped, `engine::assess` + `SuggestionKind`). When the chosen
exercise's ability confidence is `Low`/`None`, the suggestion is emitted as kind
`Assess` with a calibration protocol per metric — and the assessment *is* the
training (it's still a set for that group, no wasted day):

- weighted: "build up to a hard-but-clean set of ~5, log load/reps/RPE" (a
  starting load offered from any decayed estimate, else the lightest owned);
- reps: "AMRAP, stop at form breakdown" (open rep fields);
- hold: "one max hold" (open hold field).

No new tables: the logged set is the measurement — ability (G1) recomputes from
history, so the very next verdict prescribes from it. The Today card frames an
assess distinctly (a "Calibrate" pill + the metric-specific instruction, derived
from the catalog metric). Re-assessment triggers automatically: staleness pushes
confidence to `Low` over time (a 6-week-idle exercise re-enters calibration), and
(later stage) persistent prediction error — you keep beating or missing
prescriptions — forces a re-measure. This is the "gets finer over time" loop:
measure → prescribe → observe → correct, all deterministic.

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
marked them as such — they'd credit volume like any set (fixed: `warmup` flag).
`unilateral` is stored but still unused (a flat `sets = 3` means half the volume
per side). `difficulty` is wired through schema and API but null on **every**
entry and read by nothing (G7 needs it). "skill" used to be inferred from
hardcoded equipment slugs (`gymnastic_rings`/`parallettes`) — now a catalog
`skill` flag (magic strings gone).

**Design.** Bounded catalog curation. **Shipped**: `warmup: true` on the mobility
/ activation moves (9 tagged) + `skill: true` on the ring/parallette work (22
tagged), both seeded and reconciled via new `exercises.warmup`/`exercises.skill`
columns (migration 0015); the service reads `skill` from the catalog and the
slug sniff is gone; warm-up-tagged moves are excluded from work selection and
credit no volume (10 tagged as of 2026-07-13). The tagged moves cover push/pull
and the trunk; a session whose groups have no mobility move says so in a notice
rather than opening with a silently empty warm-up. **`difficulty` now populated** (1–5, relative within a movement
family = pattern + primary group; all 119 rated in `data/catalog/exercises.json`,
carried by the seeder — struct field + insert + reconcile — so corrections reach
prod rows). It reads coherently as ladders (e.g. push-up 2 → rings 3 → pseudo-
planche 4; supine row 1 → pull-up 3 → rings 4 → typewriter 5) and is a first
draft pending Pippijn's review; nothing reads it yet, so it changes no behaviour
until G7.

**Muscle model re-tagged (2026-07-08).** The imported muscle data was broken: **12
exercises had no muscles at all** (barbell bench press, pistol, snatch, box
jump/step-up, archer/plyo push-up, …) → *invisible* to the engine (never
selectable, credited no volume), and only **11 %** modelled any secondary muscle,
so synergist volume went uncredited and the balance engine over-prescribed
isolation. All 119 are now tagged **primary + secondary + stabilizer** (723
muscle links vs 350; every exercise has a primary; all slugs validated against the
46-muscle taxonomy), carried by the seeder's existing M:N reconcile. The engine
now weights the three roles distinctly — primary **1.0**, secondary **0.5**,
stabilizer **0.25** (was: primary 1.0, everything-else 0.5) — so a press credits
triceps/front-delt as synergists without treating a plank's core as half a working
set. Back-tested: 14/16 day-verdicts shifted (fewer neglected-looking groups, more
of the catalog reachable) — a real balance-accuracy change, not a no-op.

**The catalog is now genuinely authoritative** (2026-07-14). It wasn't: the seed's
hash gate watched `exercises.json` alone, so an edit to `equipment.json` or the
muscle taxonomy left the fingerprint unchanged and the seed short-circuited — the
change was committed, was in the image, and never reached a row. And the reconcile
wrote back only `skill`/`warmup`/`difficulty`/`implements`, so the two corrected
`youtu.be` demo links changed the hash, re-ran the seed, and left prod's rows
exactly as broken. A field the catalog owns but the reconcile skips is a field the
catalog only *appears* to own. The whole bundle is fingerprinted now and every
scalar it carries is written back; `tests/db.rs` holds both properties (a
correction reaches an existing row; an unchanged catalog doesn't re-seed).

**Still pending — warm-up coverage.** The 10 warm-up moves reach 7 muscle groups
(deltoids, rotator cuff, forearms, quads, obliques, deep core, lower back). The
other **12 have no drill at all**: abdominals, adductors, biceps, chest, glutes,
hamstrings, hip flexors, lats, lower leg, trapezius, triceps, upper back. The
engine names them in a notice rather than opening with a silently empty warm-up,
but this is the largest hole in the warm-up block, and it is pure catalog
authoring — blocked on a demo source, since every entry carries a real demo URL
and image and inventing them is not an option.

**`unilateral` — resolved, and the design note here was wrong** (2026-07-14).

This section used to say that `unilateral` was unused and that "a flat `sets = 3`
means half the volume per side". That is a correct deduction from a false premise.
It assumed a logged set was *one side*. It isn't: the standard convention — and
the one Pippijn uses — is that a single-arm movement's numbers are **per side**,
and doing both sides is **one set**. `3 × 10` on a suitcase carry is ten reps with
each arm. Coaches write `3 × 10/side` for exactly this reason.

So the volume credit the engine already gives a unilateral set is **right**, and
implementing the note as written would have introduced the bug it claimed to fix:
halving the credit for every single-arm movement, and prescribing twice the work
needed to close the gap.

What was actually missing is that nothing *said* "per side" — so a prescription of
`3 × 10` on a single-arm movement was half a session or a double one depending on
how the athlete read it. The plan card, the calibration instruction and the log
sheet's field labels now say it. No engine change; `unilateral` is a **display**
fact, not a volume one.

The one thing it still doesn't buy: a left/right **asymmetry** is invisible,
because both sides land in one set. Measuring the sides separately would show it,
at the cost of doubling the log. Worth revisiting if a side difference ever matters
clinically — but it is a *new* feature, not the fix this note pretended to be.

**Not a gap, though it reads like one**: 19 `*_legacy` rows carry no muscles, no
equipment and no demo. They are the placeholder built-ins from migration 0002,
retired by 0006 (`is_active = 0`, slug freed for the real catalog entry that
supersedes them) rather than deleted, so their ids and any references survive.
No set references them, pacing lists only active exercises, and so does the
library — they surface only if fetched by id directly. Leave them alone; adding
equipment or muscles to them would resurrect shadows of the movements the
catalog already owns.

### G6 — Recovery is a binary gate ✅ *shipped*

**Gap.** ≥ 3 effective sets within 36 h blocked a group outright, the same for
delts as for quads. Real recovery is graded and size-dependent.

**Design** (shipped). A group's recent load is now age-weighted: each set counts
fully when fresh and ramps linearly to zero over its region's **recovery
horizon** (`recovery_horizon`: legs 72 h, back/chest 60 h, shoulders 48 h,
arms/forearms/core 36 h). The recovery **fraction** = `1 − unrecovered/RECOVERY_SETS`
scales the group's deficit, so a half-recovered group is a half-priority and the
old hard gate falls out as the fraction-≈0 case (the binary-gate tests still
pass). And the biometric `recovery_scale` now also multiplies `day_target_sets`,
so a low-readiness day is *fewer* sets, not just lighter ones — the bug this
section flagged.

### G8 — The plan was built by walking groups, not by covering need ✅ *shipped*

**Gap.** Three bugs that looked unrelated were one mismodelling. `build_plan`
iterated the in-deficit **muscle groups** and asked each "which exercise fills
you?" — but the domain truth runs the other way: *one set of one exercise credits
many groups at once* (primary 1.0 / secondary 0.5 / stabilizer 0.25 — the muscle
model G5 populated). So:

- an exercise covering two in-deficit groups was emitted **twice** (dips once for
  Chest, again for Triceps), reading as a stutter — and "2 × dips" was never
  representable, only "dips" twice;
- set counts came from a separate deficit-share heuristic (`WORK_MIN_SETS`, a
  proportional split) bolted on after selection;
- the warm-up block re-solved the same covering problem with its own ad-hoc rule.

Two more bugs came from the *shape of the data*, not the loop. The prescription
was a `(i32, Option<i32>, Option<i32>, Option<f64>, Option<i32>)` tuple — 32
representable shapes, ~3 legal — and `snap()` fell back to inventing a weight
when the inventory was empty. Result in prod: a **1 kg overhead press** (the
lightest dumbbell in the room standing in for an unknown ability) and weighted
lifts prescribed at locations with no registered weights at all. And
`available_equipment: Option<HashSet>` consulted via `is_none_or` meant *no
location ⇒ everything is doable*: a missing location silently switched the safety
filter off, and the coach suggested trap-bar deadlifts in a living room.

**Design** — *shipped*. Selection is a **coverage problem**
([`pacing/cover.rs`](../src/pacing/cover.rs)): the day's need is a vector over the
group space (`ByGroup<f64>`, indexed by a dense `GroupIx` so a group index and an
exercise id can't be confused), one set of an exercise is a vector that pays part
of it down, and the day's set budget is a cardinality constraint. Maximising
coverage under it is monotone submodular, so greedy marginal gain — repeatedly
take the set that pays down the most *remaining* need — is the standard
(1 − 1/e)-of-optimal algorithm, and it's deterministic (ties → lower exercise id).

What stops being a special case:

- **Duplicates are unrepresentable.** The accumulator is keyed by exercise, so
  "2 × dips" is one item with a count. Proven for *every* history by a property
  test, not just the ones we thought of.
- **Set counts are earned.** A second set of dips is worth less than a first row
  once the first paid down chest and triceps — because the need vector clamps at
  zero. Diminishing returns is the clamp. `WORK_MIN_SETS`-as-apportionment is
  gone; what remains is a *minimum effective dose* (`MIN_WORK_SETS = 2`): a
  movement worth setting up for is worth more than one set, so the cover commits
  rather than fragmenting the day across eight movements at a single set each.
- **The budget is exact.** One set per greedy step, at most `budget` steps — the
  old "+2 spill" slack in the property test is gone with the heuristic that needed it.
- **Style ranks, but never qualifies.** The gate (`MIN_PAY`, half an effective
  set) is on the *need paid down*, in physical units; mode-fit and novelty only
  break ties among things that all genuinely need doing. Gating on `pay × weight`
  would let a merely fashionable exercise clear the bar on a group already at
  target — the athlete simulation (E3) caught exactly that.

The type work that closes the other two ([`pacing/dose.rs`](../src/pacing/dose.rs)):
`Inventory` (non-empty by construction) makes `snap` **total** — it always returns
a weight you own, and there's no empty-inventory branch to invent 13.5 kg from; a
weighted lift with no registered weights is simply *not selectable*, and the
verdict carries a **notice** naming the kit to fix rather than leaving a silent
hole. `Dose`/`Measure` are sum types per metric, so a weighted lift *has* a
`load: f64`. `Known` is an ability the engine trusts, its only constructor checks
confidence, and `prescribe` takes one **by type** — so G3's safety rule ("when
unsure, measure", the thing keeping a returning athlete off their pre-illness
numbers) cannot be bypassed by a later edit. `Kit` replaces the permissive
`Option<HashSet>`: absent kit means absent kit, and no location yields a *narrower*
verdict (no plan, and a request for one) rather than a wider one.

**Also fixed, at the root**: Balanced mode scored *every* exercise a flat 0.8 —
not a preference but the absence of one, which handed the decision to an arbitrary
tie-break on exercise id. That is how a missing lat pull-down once got
"substituted" by an L-sit hold: the engine genuinely could not tell a rep-out from
an isometric. An earlier fix patched the substitution site; the real fix is to say
what Balanced prefers (rep and weighted work 1.0, holds 0.6 — a narrower accessory
stimulus), so the tie-break never has to decide it.

**Back-tested (E1)**: the walk-forward replay itself carried the same bug — it ran
"location-agnostic", which sounded neutral but *was* the permissive path, so it had
been validating prescriptions against equipment the athlete doesn't own. It now
runs at a real location (`BACKTEST_LOCATION`, else the default), mirroring the app.

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

### G9 — The equipment vocabulary can't describe a commercial gym

**Gap.** The kit taxonomy is 13 items: dumbbell, barbell, kettlebell, trap bar,
band, cable machine, medicine ball, pull-up bar, rings, parallettes, bench, GHD,
yoga ball. That is a home gym and a calisthenics rig. It has no lat pulldown, leg
press, chest press, seated row, leg curl or extension, smith machine, or squat
rack — so in a gym full of them, **the coach cannot be told they are there**, and
plans free-weight and bodyweight work only. Nothing is wrong in the verdict; it
is simply blind to most of the room.

**Half-fixed** (2026-07-14): *which kit carries a load* is now a catalog fact
(`weighted`) rather than a guess from its category. Reading it as
`category = free_weight` was right about a bench and wrong about a pulley: a cable
stack is a `machine`, so the coach could put no weight on the one machine in the
gym whose entire purpose is the weight on it — and all five cable movements were
modelled as bodyweight reps, progressing by adding reps, forever. They are
weighted lifts now, and a stack's pin positions are enterable as what they are: a
ladder of discrete weights (`loads.rs` already handled that shape — the fixed
weights of a dumbbell rack and the pin positions of a stack are the same list).

**Remaining, and it is not a code problem.** Adding the machines themselves is
cheap: an entry in `equipment.json` and, for a selectorised machine, weights
registered at the location. What is *not* cheap is that each new machine movement
needs a catalog entry, and a catalog entry carries a real demo video and a real
image. That is the same authoring bottleneck as the mobility drills (G5), and the
same rule applies: no invented URLs.

## Engineering rigor — how we know it's right

The gaps above are the product design. These are the mechanisms that make
"provably correct" and "gets finer over time" *verified properties* rather than
aspirations. All of them exist because the engine is a pure function — none
would be possible with a stored plan.

### E1 — Back-test every engine change against real history ✅ *shipped*

Any change to the engine (a constant, a formula, a new stage) can be **replayed
walk-forward over the real logged history**. `src/bin/backtest.rs` loads the
history-independent context (the exact `service::context` the live verdict uses,
so there's no assembly drift) and, for each training day, evaluates the verdict
the coach *would* have given that morning given **only the prior days** — then
prints the ordered plan (kind, exercise, sets/reps/load, confidence). Output is
deterministic, so `backtest > before.txt`, change the engine, `backtest >
after.txt`, `diff` shows exactly what the change did to *real* prescriptions. No
heuristic gets tuned blind.

Privacy by construction: the real history never enters the repo. `scripts/prod-dump.sh`
dumps prod (read-only) into the gitignored `.dev/`, `scripts/backtest.sh` loads
it into the local dev DB and runs the walk. What's committed is the harness, not
the data — so there's no committed golden (the data is private), and the
regression check is a local before/after diff.

First run over the real corpus (287 sets, 16 training days, Sep–Oct 2024)
confirmed the arc it's meant to show: a `Fresh`, all-`Assess` cold start
resolving into real load prescriptions (e.g. `RDL 2×10 @ 74 kg [High]` with a
ramp-in warm-up) as the ability model fills in — 45 calibration → 18 work
prescriptions, 5 exercises reaching `High`. It also surfaced a real property of
*this* athlete's data: 76 distinct exercises over 6 weeks means few clear the
"≥3 sessions in 6 weeks" bar, so most stay `Medium`/`None` — the confidence model
behaving correctly on high-variety training, now visible rather than assumed.

### E2 — Property tests for the invariants ✅ *shipped*

Table tests pin behaviours; **property tests** (`proptest`, arbitrary histories —
`tests/engine_props.rs` + `tests/ability_props.rs`) pin the *invariants* that must
hold for every input, not just the ones we thought of. Shipped:

- determinism: same input → byte-identical verdict (serialised);
- suggested loads ∈ the owned-weights set (when known); rep targets sane
  (`1 ≤ lo ≤ hi ≤ 25`, sets ≥ 1);
- ability never exceeds the best *real* set's e1RM (no chimera) and is monotone
  under idleness — more time off never raises it;
- work volume ≤ the day budget (+ one trailing item's fixed spill); a non-empty
  plan always carries a training item (never warm-ups alone).

Still worth adding: logging a set never *increases* that group's deficit; a
strict warm-up cover ⊇ plan primaries (awaits full mobility catalog); degradation
never *widens* claims (no load suggestion once the inventory vanishes).

### E3 — Athlete simulation: convergence as a regression test ✅ *shipped*

A deterministic **virtual athlete** (`tests/athlete_sim.rs`): a closed-form
dose-response model — Epley reps-to-failure at a load — that performs each
prescription honestly (as many clean reps as it has, an integer RPE reporting
the reserve). No randomness, so every run is reproducible. The engine sees only
the logged sets and must *recover* the hidden true ability from them. Proven:

- **convergence**: from a cold, deliberately-too-light first assessment, the
  estimated e1RM climbs to within 6 % of a true 1RM sitting off the weight grid,
  and holds there — through weight-snapping and integer-RPE quantization;
- **honesty**: the RPE-aware estimate never materially *exceeds* true ability
  (no chimera — you can't invent strength the sets don't demonstrate);
- **stability**: once converged, the prescribed load spans ≤ one plate step (no
  ping-ponging);
- **tracking**: when true ability *grows* (a saturating gain curve), the estimate
  follows it up, staying in a tight lag band below the moving target;
- **bounded ramp**: planned volume never exceeds the day's set budget;
- **recovery honesty**: recovery is graded (G6), so a mostly-recovered group can
  take light work; nothing below the effective-recovery gate
  (deficit × recovery) is ever prescribed — asserted via each item's own
  explanation trace (E5).

This is what makes "becomes a close-to-perfect trainer over time" a tested
property of the system rather than a hope.

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

### E5 — Explanations as data, not prose ✅ *shipped*

Each work/assess suggestion carries a structured **`Explanation`** (deficit,
recovery fraction, ability confidence, e1RM, readiness band) — the factors the
verdict already computed, surfaced as data rather than buried in prose.
`Confidence` is now a wire type. Today renders an inline "Why this?" toggle that
expands the rationale in plain language, and tests assert on the trace instead of
string-matching sentences — so "why did it tell me to do X?" is answerable
exactly. (Not yet folded in: `recency`/`mode-fit` contributions and the top-line
`reason` string rendering *from* the trace — a later tidy.)

### E6 — The queries run against a real database ✅ *shipped*

E1–E5 all test the *thinking*, and the engine being a pure function is what makes
them possible. None of them can see the layer between the code and the database —
and for a long time nothing did: coach had no test that executed a single query.

That gap has a body count of one, and it was a bad one. `EquipmentRow` grew a
`loadable` field; of the two SELECTs that build it, one was updated. A `FromRow`
struct binds its columns **by name at runtime**, so the drift compiled, passed
every test, deployed, and 500'd on every exercise that has equipment — 82 of 119,
i.e. most of the library, live, in the gym. The structural fix was to share the
column list (`eq_cols!`). The other half is `tests/db.rs`: a scratch database,
migrated and seeded from `data/catalog/`, that runs the read paths against it —
the whole catalog through the join that broke, plus `service::now` end to end from
a real location and real logged sets. Reintroducing the bug fails it with the
production error verbatim.

Both gates provide the database (`verify.sh` starts a throwaway MariaDB if one
isn't up; CI gets a `mariadb` service), and the tests **fail loudly with no
server rather than skipping** — a test that quietly passes when it cannot run is
worse than no test, because it reports coverage it isn't providing.

This is also the prerequisite for moving the catalog out of SQL (see below): that
migration rewrites the one table holding data that cannot be regenerated.

## Staging

Each stage ships alone and keeps every existing test green.

1. **Ability model (G1)** — ✅ *shipped*. Pure `ability.rs` (staleness + RPE),
   `engine::prescribe` derives the autoregulated load from it, `LastPerf` and
   the chimera query deleted. Killed the stale-PR and chimera-top-set bugs.
2. **Assessment items (G3)** — ✅ *shipped*. `SuggestionKind::{Work, Assess}` on
   the wire; untrusted confidence → a calibration set; Today frames it as such.
3. **Session plan + ordering (G2, sans warm-up)** — ✅ *shipped*. Engine emits
   `plan: Vec<Suggestion>` tiered + sized to deficits; Today renders the session
   list; `suggestion` stays as the head.
4. **Warm-up block + catalog curation (G2a, G5)** — ✅ *mostly shipped*: the
   warm-up block, `warmup`/`skill` catalog flags + migration 0015, volume
   exclusion, `difficulty` populated (unreviewed, and read by nothing until G7).
   Remaining: `unilateral` per-side sets; the missing mobility moves; the two
   catalog entries with no equipment links.
5. Independent refinements, any order: **graded recovery (G6)** ✅ *shipped*;
   **feedback progression + plateau (G4)**, **variation ladders (G7)**,
   **cross-exercise priors (G1 tail)** — pending.

Rigor: **E2 (property tests)** ✅, **E5 (explanation trace)** ✅,
**E3 (athlete simulation)** ✅, and **E1 (back-test against real history)** ✅
*shipped* — convergence/stability/recovery tested against a deterministic virtual
athlete, and every engine change now diffable walk-forward over the real logged
corpus. Still to come — **E4 (residual ledger)** feeding G4 + per-user
calibration.
