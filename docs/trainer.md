# The trainer model

What coach computes, why it is built that way, and what is not built yet.

The goal in one line: a **deterministic personal trainer** — no ML, every number
derivable from logged history by a pure, tested function — that gives you today's
session in order, knows what you can currently do, measures when it doesn't, and
gets sharper the more you log.

## Principles

These hold today and must keep holding. Everything below is downstream of them.

**Stateless.** The verdict is `evaluate(history, settings, now)`. There is no stored
plan and nothing to drift out of sync; logging a set changes the next verdict.

**Anchored to your own history.** Population numbers appear only as cold-start
anchors, never as landmarks to hit.

**When it doesn't know, it measures.** An untrusted estimate produces a
*calibration set*, not a guessed number. The logged result is the measurement.

**Degrade narrowly.** Missing data (no biometrics, no location, no history) narrows
the verdict, never widens it. Absent information is not permission: with no
location the engine does not know what is doable, so it declines to plan and asks —
it does not assume everything is doable.

**Illegal states unrepresentable.** The safety rules are carried by types rather
than by care. A load can only come from a measured ability ([`Known`]) and can only
be a weight you own ([`Inventory`], non-empty by construction); a weighted lift
*has* a load ([`Dose`], a sum type). "When I don't know, I measure" is a property
the compiler enforces on every future edit, not a code path an edit can bypass.

**Labelled heuristics.** Every coefficient is a named constant with a comment
saying what it is and why (top of [`engine.rs`]). Tunable, not magic.

**The athlete reports what happened, not how it felt.** The loop is a human
trainer's: the coach instructs, the athlete tries, the athlete records the result,
and the system computes better numbers from the growing log so the next instruction
is sharper. What he records is an observation — reps, load, seconds. He is never
asked to rate his own exertion.

This constrains the *interface*, not the engine: how the maths gets its accuracy is
an implementation choice. RPE is read when history carries it and never solicited.
Nothing depends on it — a calibration set is "as many clean reps as you can, stop at
form breakdown", so `rir = 0` is true by construction, and working sets progress by
double progression. A missing RPE biases the estimate downward: conservative, never
permissive.

**The UI is the trainer's voice, not its dashboard.** Today shows only what you need
to do the next set: one status line, the coach's one sentence, the ordered plan, a
log button. Engine internals surface on demand ("Why this?"), analysis lives in
Balance, knobs in Settings. Every element must pass: *does the athlete need this to
train right now?*

## How a verdict is computed

`service::now` assembles the input from the database and calls the pure engine. In
order:

### 1. Ability — what you can do today

[`pacing/ability.rs`] turns logged sets into an estimate per exercise.

- **Per metric.** Weighted work: an estimated 1RM per set (Epley,
  `e1rm = load × (1 + (reps + rir) / 30)`). Rep work: best effective reps. Holds:
  best seconds. Loaded carries: the best (weight, seconds) pair, which travel
  together because neither means anything alone.
- **Staleness decay.** Each set's estimate is scaled by *its own* age — full trust
  for two weeks, then the detraining slope to a 60 % floor — and ability is the max
  of the decayed estimates. Decaying per set *then* maxing makes ability monotone
  under idleness while still trusting a genuine old PR down to the floor.
- **Training-block reset.** A gap longer than `BLOCK_GAP_WEEKS` (8) splits an
  exercise's history into blocks, and only the most-recent block estimates ability.
  After a layoff or a health setback your level is read from your *return*, never
  from a pre-break PR that no longer describes you.
- **Confidence.** `High` ≥ 3 sessions in the last 6 weeks, `Medium` 1–2, `Low`
  only-stale data, `None` never done. Confidence — not a default — decides whether
  the engine prescribes or measures.

### 2. Need — what today should chase

Rolling 7-day volume per muscle group against a target blended from a literature
anchor and your own weekly rate, tilted by mode, emphasis and days-per-week.

The weekly rate is a **shrinkage estimate**: it starts at an anchor and each
*observed* week pulls it toward your own. It divides by the weeks you have actually
trained, not by the width of the window — so logging can only ever raise it, and
never makes the coach believe you train less than it did before you logged.

**Recovery is graded, not a gate.** A group's recent load is age-weighted: a set
counts fully when fresh and ramps to zero over its region's horizon (legs 72 h,
back/chest 60 h, shoulders 48 h, arms/forearms/core 36 h). The recovery fraction
scales the group's priority, so a half-recovered group is a half-priority.

**Readiness.** Biometric recovery from health-sync (sleep, HRV, RHR) scales both the
volume target and the day's set count, and gates progression — a low-readiness day
is *fewer* sets, not merely lighter ones. Without biometrics a volume-spike proxy
stands in: this week measured against the weeks *before* it. With no prior weeks
there is no spike to claim.

### 3. Cover — which movements, and how many sets

Selection is a **weighted set-cover** ([`pacing/cover.rs`]), not a walk over groups.
The domain truth is that *one set of one exercise credits many muscle groups at
once* (primary 1.0, secondary 0.5, stabilizer 0.25). So the day's need is a vector
over the group space, one set is a vector that pays part of it down, and the set
budget is a cardinality constraint. Greedy marginal gain — repeatedly take the set
that pays down the most *remaining* need — is (1 − 1/e)-optimal for a monotone
submodular objective, and deterministic (ties break to the lower exercise id).

Three things fall out rather than being special-cased:

- **Duplicates are unrepresentable.** The accumulator is keyed by exercise, so an
  exercise covering two groups is one item with a count.
- **Set counts are earned.** A second set of dips is worth less than a first row
  once the first paid down chest *and* triceps, because the need clamps at zero.
  Diminishing returns is the clamp. What remains is a minimum effective dose
  (`MIN_WORK_SETS = 2`): a movement worth setting up for is worth more than one set.
- **Style ranks but never qualifies.** The gate (`MIN_PAY`) is on need paid down, in
  physical units; mode-fit and novelty only break ties among movements that all
  genuinely need doing.
- **One movement per family.** Variations of one movement — the catalog's base name:
  both hamstring-curl entries, the three farmers-walk carries — train the same thing
  the same way, so a session admits one entry per family; the second cousin is
  redundant stimulus wearing a different label, and its budget goes to whatever else
  still pays.

Only movements that are actually doable are candidates: the kit must be present, and
a weighted lift must have registered weights at this location. A lift dropped for
want of weights is **named in a notice**, not silently omitted.

**The variation ladder.** `difficulty` (1–5, relative within a pattern + primary
group) ranks a movement's variations. A movement the athlete has **topped out** (the
rep range's ceiling, at `High` confidence — the ask is clamped there, so "keep doing
12s" would be forever) or **plateaued** on (a month of sessions with nothing beaten)
has stopped producing progress; while a harder doable variation of the same pattern
and primary group exists, that rung steps out of candidacy, measuring its successor
becomes a need of its own (the same mechanism as confirmation, so it qualifies even
when the group's volume is covered), and the step is announced until the successor
has an estimate of its own. "You've outgrown incline push-ups" is something this
coach can now say.

### 4. Dose — what to actually do

[`pacing/dose.rs`] carries this in types. `Dose` is a sum type per metric, so a
weighted lift *has* a load and a carry *has* both a weight and a time.

- **Weighted:** the load whose top-of-range reps the estimate supports (inverse
  Epley), snapped to the nearest weight you own. Double progression follows: reps
  climb to the top of the range, and the load steps only when logged sets raise the
  estimate past the next owned weight — never a blind +2.5 kg the reps don't support.
- **Reps:** climb the range off the decayed best.
- **Hold:** off the best hold.
- **Loaded carry:** the same double progression with seconds where the reps go —
  climb the clock to the ceiling, then take the next weight you own and reset it.

**Asking for more is a probe, and the sets answer it.** Every prescription is a
prediction, and the prediction-error ledger ([`pacing/residual.rs`]) recomputes from
history alone how each session turned out. One miss **holds** the numbers; two in a
row **step down** a rung; three **re-open the measurement** — a repeatedly wrong
estimate is a wrong number, not a run of bad luck. And the +1 rep, the +5 s, the next
bell is **earned**: by a session that beat the ask, or periodically after every third
quiet session — the sessions in between consolidate at the demonstrated best.
Matching your best while failing the ask moves nothing (ability is a max), so without
the cadence the same failing +1 was re-asked verbatim every session for weeks.

**The ledger judges the ask, not the ceiling** — and this is the load-bearing
distinction. The engine does not always ask for everything the estimate supports:
whenever it is holding or backing off, and on an under-recovered day, it deliberately
asks for *less*. Judged against the ceiling,
full compliance with that reduced ask read as failure — and the back-off fed itself,
because two real misses eased the ask and the eased session then became miss number
three, sending a perfectly good estimate back to calibration. "Back off and rebuild"
could never rebuild. So the ask is reconstructed from the same numbers `prescribe`
used, at **the load the athlete actually logged** — which means the owned-weight rack
never has to be reconstructed, and an improvised weight is judged honestly instead of
as a shortfall. A session is judged on its best set (the third set's fatigue is not a
miss), and a session sharing no metric with the estimate is not evidence either way —
the engine must never back off from silence. The dose constants both sides read live
in one place ([`pacing/dose.rs`]): two copies would mean the coach asking one number
and the ledger marking another, with the athlete taking the blame for the gap.

Readiness is the half of the ask that isn't in the set history, so it is reconstructed
by asking health what it knew that morning (`/internal/recovery/history`, composed
into a score by coach exactly as today's is — health stays unopinionated, and there is
one definition of readiness). A day health can't answer for is judged full-effort: a
missing signal must never invent an easing that didn't happen. Coach deliberately does
**not** store the score it computed. That would be the one input that stops re-deriving
when the formula is tuned, while every other number in the engine moves — and if a past
morning's sleep data is later corrected, that morning's readiness genuinely *was*
different from what the coach believed. Re-deriving from the source of truth is the
honest answer, not the lossy one.

When confidence is `Low`/`None` the item is a **calibration** instead, with a
protocol per metric (build up to a hard set of ~5; AMRAP; one max hold; carry it and
log the weight and the seconds). The calibration *is* the training — it is still a
set for that group — and the next verdict prescribes from it.

### 5. The session

The plan is ordered by training tier: warm-up → skill and hold work while the
nervous system is fresh → heavy compound weighted work → accessories → core and
conditioning finishers.

The **warm-up block** leads it: mobility drills for the committed session's
heaviest-loaded groups (one drill per group, so drills don't stack) plus a half-load
ramp-in of the first weighted lift. The block is **sized to the session** — one
drill per ~3 committed sets, never more than 6 — because with a drill for every
group, every loaded group would claim a slot and the warm-up outgrows the work it
warms up for; the tail groups get their prep from general movement and the
ramp-ins. Warm-up sets credit no volume and are drawn only from catalog moves
tagged `warmup`, so they never eat the day's targets. A group the coach *wanted*
to warm but has no available drill for is **named**, not silently skipped; a group
left out by the size cap is triage, not ignorance, and carries no note.

Where a group's top-ranked movement is genuinely blocked, the item carries a
**substitution** naming the ideal and the blocker. It is set only when that movement
is actually blocked — the cover routinely picks something other than a group's best
exercise, and reporting that as missing kit would name things that are standing in
front of you.

## The catalog

`data/catalog/` is the **source of truth** for the training library: equipment, the
muscle taxonomy, and the exercises with their muscles, kit, flags and pictures. The
seeder loads it at boot, and reconciles every scalar it carries onto rows that
already exist — a field the catalog owns but the reconcile skips is a field the
catalog only *appears* to own.

What an exercise carries: `pattern`, `metric`, `unilateral`, `implements`,
`difficulty`, `skill`, `warmup`, a cue, a demo video and an image.

- **`metric`** is what the movement is measured in: `reps`, `weighted_reps`, `hold`,
  or `weighted_hold` (a loaded carry — weight *and* time, since neither alone
  describes a farmer's walk).
- **`implements`** is how many of the kit the movement uses. It decides which loads
  are buildable: a two-dumbbell press can't be built from a weight you own one of,
  and a pair of adjustable handles splits the disc budget between them.
- **`unilateral`** is a **display** fact, not a volume one. The convention is the
  standard one: a single-arm movement's numbers are **per side**, and doing both
  sides is **one set**. `3 × 10` on a suitcase carry is ten reps with each arm. The
  volume credit is therefore the same as a bilateral set; what changes is that the
  plan, the calibration instruction and the log sheet all say "each side".
- **A movement may exist before anyone has filmed it.** `demo_url` and the image are
  optional, and "no demo yet" is a visible state (`coachctl todo` lists them). The
  alternative — refusing to track a movement you actually did, or pointing it at an
  approximate video — is worse: a wrong demo is a wrong instruction, in a gym, alone.
- **Pictures are rendered on the way into the database** ([`seed/render.rs`]). The
  bundle keeps what it was given, alpha included, because a flattened source cannot
  be un-flattened. What the app is served is what the app can display: a transparent
  anatomy diagram is composited onto white (it would vanish on a dark theme) and a
  picture far from the hero's 16:9 shape is padded, never cropped (a portrait figure
  would be cropped to a band across its stomach).

**Locations** hold the kit you have and the weights you own of it — fixed weights,
bars and their plates, machine stacks. `weighted` marks kit that carries a load,
which includes a cable stack: a pulley is a machine, but the weight on it is the
whole point. Loads are resolved **per exercise**, not per equipment, because what is
buildable depends on how many implements the movement needs.

The 19 `*_legacy` rows are retired placeholders (`is_active = 0`), superseded by
real catalog entries. Nothing references them and nothing lists them. Leave them.

## How we know it's right

The engine is a pure function, which is what makes all of this possible.

**Back-test against real history** (`src/bin/backtest.rs`). Any engine change can be
replayed walk-forward over the real logged corpus: for each training day, the verdict
the coach *would* have given that morning knowing only the prior days. Output is
deterministic, so `backtest > before.txt`, change the engine, `diff`. No heuristic is
tuned blind. The data never enters the repo — `prod-dump.sh` puts it in the gitignored
`.dev/`; what's committed is the harness.

**Property tests** (`tests/engine_props.rs`, `tests/ability_props.rs`) pin the
invariants for *every* input, not the cases we thought of: determinism; suggested
loads are always weights you own; ability never exceeds the best real set and is
monotone under idleness; volume never exceeds the day's budget.

**Athlete simulation** (`tests/athlete_sim.rs`). A deterministic virtual athlete with
a closed-form dose-response performs each prescription honestly; the engine sees only
the logged sets and must recover the hidden true ability. This is what makes
"converges on a good trainer over time" a tested property rather than a hope — it
asserts convergence to within 6 % of a true 1RM off the weight grid, stability once
converged, tracking when ability grows, and that the estimate never exceeds truth.

**Simulated futures** (`src/bin/simulate.rs`, run via `scripts/simulate.sh`). The
test above is one athlete on one exercise; this plays a whole athlete against the
whole engine for simulated weeks, growing forward from the real history in the dev
DB: each day's verdict is performed as well as a temperament's hidden true ability
allows — `improver`, `plateauer`, `badweek` — logged, and the walk continues on the
grown history. It exercises the loop the athlete actually lives in (prescribe →
perform → re-estimate → prescribe), which the back-test structurally cannot: replayed
history never responds to the coach. Deterministic, so traces diff across engine
changes. The probe cadence, the plateau/ladder pair and the coarse-rack rounding rule
all came out of its first traces.

**Explanations as data** (`Explanation`). Every work and calibration item carries the
factors that produced it — deficit, recovery, the pay that qualified it, confidence,
e1RM, the ledger's miss streak, readiness — so "why did it tell me to do X?" is
answerable exactly, and tests assert on the trace rather than string-matching
sentences.

**Real SQL against a real database** (`tests/db.rs`). The pure tests cannot see the
layer between the code and the database. A `FromRow` struct binds its columns *by
name at runtime*, so a SELECT that drifts from it compiles, passes every pure test,
and fails in production. The DB tests migrate and seed a scratch database and run the
read paths against it, including the whole catalog through the join most likely to
drift. Both gates provide the database, and the tests fail loudly when there is none
rather than skipping — a test that quietly passes when it cannot run reports coverage
it isn't providing.

## Not built yet

**Cross-exercise priors.** A never-done exercise starts from nothing. A first attempt
— a flat variation discount over any same-pattern, same-group sibling — was built and
back-tested, which caught it prescribing a 62 kg good morning off an RDL estimate: a
flat ratio doesn't capture that two movements can share a muscle group and not a
strength or risk profile. Correct sequence: derive real ratios from `difficulty`, and
even then a prior may only *seed a calibration's starting load*, never skip the
calibration. *(G1 tail.)*

**Per-athlete calibration of the labelled constants.** The prediction-error ledger
(built — see §4) makes it possible to *choose*, from a small labelled grid, the
constants that minimise historical residual for this athlete. Calibration, not
learning: the model form never changes, and the back-test shows exactly what a
switch does. *(The E4 tail.)*

**A commercial gym is mostly unrepresentable.** The kit taxonomy has no lat pulldown,
leg press, chest press, seated row, leg curl or extension, smith machine or squat
rack — so in a gym full of them the coach cannot be told they are there, and plans
free-weight and bodyweight work only. Adding the kit is cheap; each machine movement
then needs a catalog entry. *(G9.)*

**Left/right asymmetry is invisible**, because both sides land in one set. Measuring
the sides separately would show it, at the cost of doubling the log. A new feature,
not a fix.

<!-- Code comments refer to gaps and rigor mechanisms by the IDs above: G1 ability,
     G2 session plan, G3 calibration, G4 feedback progression, G5 catalog data,
     G6 graded recovery, G7 variation ladders, G8 set-cover selection, G9 equipment
     vocabulary; E1 back-test, E2 property tests, E3 athlete simulation, E4 residual
     ledger, E5 explanation trace, E6 database tests. -->

[`Known`]: ../src/pacing/dose.rs
[`Inventory`]: ../src/pacing/dose.rs
[`Dose`]: ../src/pacing/dose.rs
[`engine.rs`]: ../src/pacing/engine.rs
[`pacing/ability.rs`]: ../src/pacing/ability.rs
[`pacing/residual.rs`]: ../src/pacing/residual.rs
[`pacing/dose.rs`]: ../src/pacing/dose.rs
[`pacing/cover.rs`]: ../src/pacing/cover.rs
[`pacing/dose.rs`]: ../src/pacing/dose.rs
[`seed/render.rs`]: ../src/seed/render.rs
