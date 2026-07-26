# Field test — a simulated session, played as the athlete

On 2026-07-16 Claude played a full workout through the production UI as Pippijn
would: warm-ups, calibrations, work sets, honest fatigue (a target missed by a
rep late in the session), logged through the plan cards and the manual dialog.
Eleven sets across eight movements. This file records what a human athlete runs
into, each finding's root cause, and its status. Findings are ordered by how much
they damage the coaching, not by where they live in the code.

Round 2 (same day, after the round-1 fixes shipped) is below under
[Round 2](#round-2--the-fixed-app-judged-against-a-real-coach).

## 1. The plan is memoryless within a session — FIXED

Every logged set re-solves the whole day from scratch, treating a set logged
seconds ago as ordinary history. One session produced all of these:

- A **calibration was immediately re-prescribed**: an "as many clean reps as you
  can" leg raise gave an honest max of 4, and the next verdict asked for
  2 sets · 5–12 of the same movement — more than the just-demonstrated max,
  minutes after form breakdown. Then 3 sets. Same for hamstring curls (AMRAP 8 →
  "9–12", sets inflating 2 → 3 → 4 as the session went on).
- The **best+1 progression ratcheted set-over-set**: one pull-up set of 4 raised
  the ask to 5 before the next set. Session-over-session logic ran inside a
  single session.
- **Movements vanished mid-exercise**: dips disappeared from the plan after 1 of
  its 2 sets (its muscles read "recovering"), triceps extension likewise;
  push-up — in the opening plan — vanished without a single set done. The
  athlete cannot finish what the coach asked for.
- The **day target drifted** (13 → 14 sets) mid-session.

Fix: a session, once begun, is a commitment. The engine evaluates ability,
recovery, the day target and the novelty budget **as of session start** (today's
sets excluded), and today's sets only report progress against that plan — done
counts on the same cards — never a bigger ask. A calibration is complete after
its one measurement. `evaluate` stays pure: "what was true at session start" is
derived from the same history by cutting at today's first set.

## 2. Novelty churn — FIXED

The novelty cap bounds *concurrently pending* never-done movements, so each
completed calibration freed a slot and a new untried movement slid in — side
plank appeared, vanished unperformed, reappeared; toe raises materialised near
the end. By session end three untried movements were queued at the point of
maximum fatigue. Fix: count movements *introduced today* (first-ever set today)
against the cap, so finishing a calibration spends the slot instead of recycling
it.

## 3. Mid-session messaging — FIXED

- "**Just trained 0m ago — take a breather.**" after every set: the
  between-sessions rest gate firing between sets. Mid-session the banner should
  report session progress instead.
- "Why this?" bullets still say "**a good day to push**" (the word purged from
  the headline survived in the explanations) — four lines above "**67%
  recovered**". State readiness without prescribing a push.
- "**100% below this week's target**" is maths-speak for "untrained this week".
- **Substitution attribution flaps mid-session**: pull-up was "swapped in for
  Lat pull down", later "swapped in for Row (barbell)"; a triceps extension once
  claimed to stand in for a *Good morning*. The greedy cover's "first stand-in
  for the group" trace is unstable once the day is partially done; a swap note
  should only name an ideal blocked by kit for the group the pick actually
  labels to.

## 4. The log dialog dies under your finger — FIXED

The background re-plan closed an open log dialog. Twice a "Log set" tap landed
on the bottom-nav **History** tab beneath it; once an edit didn't land and the
dialog silently logged the *prefilled* value instead of the typed one — wrong
data, not just lost data. And cards went stale in the other direction too: a
logged calibration kept offering "Log the calibration set" until a manual
reload.

Fix: an open dialog is never closed by a background refresh, and the sheet keeps
a safe margin from the bottom nav.

## 5. Warm-ups are inert — FIXED

Warm-up cards have no dose (how many? how long?), no way to be marked done, the
same generic copy on every card, and the "Next up" chip points at them all
session. Zero warm-up sets exist in the entire history because only the manual
+ dialog can log one. Fix: warm-up cards carry a dose, log from the card, show
done, and Next up advances past them.

## 6. Small UI faults — FIXED

- "**10–10 reps**" renders when low == high; should read "10 reps".
- The manual + dialog defaults to "Arm circles" (alphabetical); it should default
  to the next planned movement.
- A manually chosen location reverts to the detected one on reload, silently
  changing the plan's loads (4 kg ↔ 5 kg); the choice should stick for the day.
- No per-card set progress: after finishing both prescribed curl sets the card
  still reads "2 sets · 10–10 reps" with nothing done. Cards should show sets
  done.

## What the test confirmed is right

Instant re-planning as an architecture; honest kit-blocked swap notes ("no Trap
bar here"); calibration copy ("one honest max. Both sides — the numbers are per
side."); prescription-prefilled log dialogs; the demo sheet; History's per-day
movement grouping; Balance. The engine's per-decision taste was sound — the
failures were all in what it remembered (nothing) about the session it was
already coaching.

# Round 2 — the fixed app, judged against a real coach

Same method, later the same day: a full Home session (16 sets — 3 warm-ups,
5 work movements, 3 calibrations) played through prod, this time asking of every
screen "would a good human coach say this?". The session-commitment engine held:
the plan never reshaped mid-session, done counts landed on the right cards, the
sheet survived the whole run, and the closing banner fired. The findings below
are what still separates it from a good coach.

## R2-1. A hidden stale load was written into the record — FIXED

Switching the log sheet's exercise (biceps curl prefill → Squat sky reach) hides
the load field, but the component kept `loadKg = 4` and sent it: two bodyweight
mobility drills were stored as "10 reps · **4.0 kg**" (sets #322/#323). Nothing
on screen showed a load at log time — silent data corruption, and the server
accepted a load on a `reps`-metric exercise without complaint.

Root cause, two layers: the sheet posts whatever the (possibly hidden) fields
hold, and the API trusts it. Fixed in both: switching the sheet's exercise
re-derives every field (the plan's prescription for a planned movement, blank
otherwise — a 5 from dips can no longer become a hamstring-curl calibration),
the payload only carries the fields the metric owns, and the server 400s any
load/reps/hold the exercise's metric cannot carry (`NewSet::shape_error`).

## R2-2. The day counter double-books its own plan — FIXED

The plan header finished at "**14 / 13 sets**". The denominator (13) excludes
the 3 warm-up slots; the numerator excludes the two mobility drills but *counts
the ramp-in curl* (it shares an exercise with a work item), so completing the
plan exactly reads 14/13 — and mid-session, after all three warm-ups, it read
1/13 while three cards showed Done. One plan, two bookkeeping rules. Fixed:
the header sums the plan's own cards — sets and done both, warm-ups included —
so it cannot disagree with them and finishing the plan is N/N by construction.
(The engine's `dayTargetSets` keeps its estimator meaning; it sizes the plan
and drives the nudge, and no longer doubles as the header.)

## R2-3. The warm-up doesn't warm up the session it precedes — FIXED

The session's heavy work was dips, pull-ups, push-ups — and the warm-up spent
two of its three slots loosening **Obliques twice** (Squat sky reach + Side
bend) plus one biceps ramp-in. Shoulders, elbows and wrists went into dips cold.
Two independent defects:

- **No dedup by target group**: two drills for the same muscle in one warm-up
  is a wasted slot by construction.
- **Coverage isn't driven by the day's plan**: the picker warms what it has
  drills for, not what the committed session most needs. Arm circles (Deltoids)
  exist in the catalog — the Office plan picked them the same morning — and the
  Home plan left them out while programming three pressing/hanging movements.

Fixed: warm-up coverage now follows the committed plan's *load* — effective
sets per group, primaries and secondaries both, ranked heaviest first — one
drill per group, each card labelled with the group it was picked for (so two
cards can never both read "loosen up Obliques"). The catalog gaps (no drill at
all for Chest/Lats/Triceps/Hamstrings/…) were closed after round 3: every
muscle group now has at least one equipment-free drill, so the "I don't know a
warm-up for X" fallback can no longer fire. The new drills await pictures and
demos ([todo](todo.md)).

## R2-4. Isolations are programmed before the compounds they sabotage — FIXED

Plan order: … biceps curl → triceps extension → **pull-up** → **push-up**.
Curling to near-failure immediately before pull-ups (and triceps extensions
before push-ups) is a sequencing error no human coach makes: the isolation
pre-fatigues the smaller muscle that the compound needs as a link, so the
compound reads artificially weak — and this session's compound numbers *are*
ability measurements. Fixed: session order now goes by movement *breadth*
(muscle groups trained at primary/secondary credit) — three or more is a
compound and leads; being weighted no longer is what puts a movement early.

## R2-5. Rest guidance is a shrug — FIXED

Two gaps a coach fills without being asked:

- "Rest a moment — then: 2 × Dips" fired **after mobility drills**. Warm-up
  sets need no rest gate; the prompt teaches the athlete to ignore the banner.
- "Rest a moment" never says how long. Strength work wants concrete numbers
  (2–3 min after a hard compound set, ~90 s after an isolation), and the gap is
  knowable from the set just logged.

Fixed: a mobility drill starts no rest clock at all, and a rest prompt now
carries a length from the movement just done — "Rest 2–3 min" after a compound
set, "Rest 90 s" after an isolation.

## R2-6. "3–12 reps" reads as a floor of 3 — FIXED

Dips prescribed "3–12 reps" and the log sheet prefilled **3** (pull-ups: 4).
The diagnosis in the field notes ("prefill anchors at the range floor") turned
out wrong on inspection: the low end already *is* the engine's aim — the
demonstrated best ± the day's adjustment — and the high end is the mode's
style ceiling, so prefilling it was correct. What was broken is that nothing
said so: "3–12" reads as "at least 3", which invites the minimum. Fixed in the
card copy: a work item now reads "aim 3, up to 12 reps".

## R2-7. The banner and the plan disagree on "next" — FIXED

Fresh session: banner says "Next up: 2 × Dips" while the Next-up pill sits on
Squat sky reach (warm-up #1). The suggestion skips warm-ups; the pill doesn't.
A coach doesn't name two different next things. Fixed: the banner speaks from
the same "first unfinished plan item" the pill points at — "Warm up first:
Squat sky reach — 10 slow reps." — and the rest prompt names that item too.

## R2-8. Small frictions

- The log sheet's exercise dropdown was ~120 items in **catalog order** — a
  mid-workout scroll hunt. FIXED: today's planned movements first, in plan
  order, then everything else alphabetically. (No search field yet; the
  Library page has one if you're browsing.)
- Dips headlined "(Serratus)" — the label fell to the neediest group it touched
  at all. FIXED: the headline is now the movement's neediest *prime mover*; a
  synergist can only ever label a movement the catalog gives no primary.
- Logging reflowed the page under an open tap twice during the round (once
  ending on the Library page). Not reproducible on inspection — the plan list
  re-renders in place and both incidents are consistent with automation tap
  timing rather than a layout fault. Watching, no code change.
- Triceps extension has no catalog image (placeholder icon). OPEN — needs an
  actual picture; `scripts/add-image.py <slug> <url-or-file>` seeds it once one
  is found.

## What round 2 confirmed is right

The committed plan held perfectly through 16 sets — no re-prescription, no
ratchet, no vanishing movements, target fixed at 13, done counts landing on the
right cards in plan order (ramp-in credited before the work sets of the same
exercise). The sheet stayed open across a whole run of sets and its per-metric
fields adapt on exercise switch. Warm-up doses, "Measured — locked in", the
kit-blocked swap notes, and the closing "That's the session — nice work." all
behaved. The remaining gap to a human coach is knowledge (warm-up coverage,
ordering, rest), not machinery.

# Round 3 — the Office session, verifying the fixes

Same method, same day, third pass: a full Office session (18 sets — 5 warm-up
drills, 13 work sets across 7 movements) played through prod on the round-2
build, at the location with the other drill pool and kit. Every round-2 fix
held up live:

- **Warm-up** (R2-3): five drills, five distinct groups — wrists first
  (forearms carry the most load in a session of pull-ups, two carries and
  curls), then shoulders, quads, lower back, deep core. Nothing doubled, and
  the heavy pressing/hanging work no longer starts cold.
- **Order** (R2-4): pull-ups → carries → squats → push-up, curls and
  extensions last.
- **Banner = pill** (R2-7): "Warm up first: Wrist flexor stretch — 10 slow
  reps." while the pill sat on the same card; after a compound set the banner
  read "Rest 2–3 min — then: …" (R2-5).
- **Counter** (R2-2): closed at exactly **18 / 18** with "That's the session —
  nice work." A duplicated warm-up set (mis-tap) capped against its card
  instead of inflating the count, and a set logged through the CLI attributed
  to the right card.
- **Stale fields** (R2-1): switching the sheet between a carry (kg + seconds),
  a squat (kg + reps) and mobility drills re-derived the fields every time —
  every bodyweight set in the round landed with no load attached.
- **Labels** (R2-8): "Next up: 2 × Pull-up (bar) (Lats)" — prime mover, not
  synergist. The picker listed the plan first, in plan order.

## R3-1. No plausibility bounds on logged values — FIXED

A fat-fingered edit (35 → "3530", an append instead of a replace) logged a
**12 kg farmers walk of 3 530 seconds** — a fifty-nine-minute carry — and
nothing blinked. The set was stored and would have fed the carry ability
estimate as a demonstrated max. A coach hearing "I carried it for an hour"
asks you to repeat that with a straight face.

The metric-shape validation (R2-1) checked *which* fields a set carries, not
whether their values are humanly possible. Fixed: `NewSet::shape_error` now
also bounds the values — reps 1–100, seconds 1–600, load 0–300 kg, RPE 1–10 —
generous ceilings no honest set exceeds, so a real outlier day is never
refused. The log sheet shows the server's objection under the fields instead
of failing silently (a swallowed rejection reads exactly like a logged set).

## R3-2. The budget remainder under-doses a movement — FIXED

The plan carried "Push-up — 1 set" mid-session: the cover's last pick got
`min(MIN_WORK_SETS, budget left) = 1`. The engine's own constant says a lone
set of a work movement wastes its setup. Fixed in the cover: entering a
movement now requires budget for its full minimum dose; a too-small remainder
tops up a movement already in the session (re-ranked like any other set) or
goes honestly unspent. A one-set calibration still fits a one-set remainder —
one set *is* its full dose.

## R3-3. Movement families aren't deduplicated — FIXED (round 4)

Farmers walk *and* Farmers walk (waiter) were both planned — cousins differing
only in where the kettlebell sits. Same class as Hamstring curls vs its
single-leg variant in round 1. Closed in round 4: the cover admits one entry
per catalog family per session (the family is the catalog's base name), and
the freed budget goes to whatever else still pays.

## Automation note

One warm-up drill was logged twice and another skipped mid-round: the exercise
dropdown auto-scrolls to keep the selected option visible, so scripted taps at
remembered coordinates hit neighbours. Recovered in-flow (the card capped the
duplicate; the skipped drill logged from its own card). Not an app defect —
but it is the same "targets move under a committed tap" family a rushed human
thumb meets, and the plan-first picker section made the recovery easy.

# Round 4 — simulated futures: the coach against a virtual athlete

Rounds 1–3 played single sessions. This round asked the question a session
can't: **is it a good coach over weeks?** The back-test can't answer it either
— replayed history never responds to the coach — so round 4 built E3
(`src/bin/simulate.rs`): a deterministic virtual athlete grows forward from
the real logged history in the dev DB, performing each day's verdict exactly
as the cards present it (instruct → try → record, never reporting an RPE) as
well as a hidden true ability allows, for eight simulated weeks per
temperament — `improver` (steady gains), `plateauer` (two weeks of gains,
then flat), `badweek` (an improver whose week 3 goes badly). Everything below
came out of the first traces and is pinned by tests.

## R4-1. A failing ask was re-asked verbatim, forever — FIXED

The improver was asked "4 pull-ups" and did 3 — then asked 4 again next
session, and the next, for weeks, with the ledger reading *zero* misses: the
ledger compares a session against the **estimate**, and matching your best
while failing the +1 is a Met. Ability is a max, so nothing ever moved. The
plateauer collected **163 missed cards in 56 sessions** — a coach who never
listens.

Fixed: asking for more is now a **probe**, earned by a session that beat the
estimate or periodic after every third quiet session; the sessions between
consolidate at the demonstrated best (and a consolidation ask never rounds up
past what the sets have shown). Missed cards: improver 118 → 53, plateauer
163 → 62, badweek 110 → 62 — while the improver's end-of-trace estimates came
out *equal or higher* (supine pull-up reached the range top instead of
stalling below it). Calmer and faster at once.

## R4-2. A movement at its wall was prescribed into it forever — FIXED

The supine pull-up sat at the rep range's top for six weeks (true ability well
above); Lateral raise and Dead bug plateaued and were re-served anyway. The
`difficulty` field existed for exactly this and was read by nothing (G7).

Fixed, in two pieces. **Detection:** a month of sessions with nothing beaten
(and no slump — misses are the back-off's business) is a plateau; the rep
range's ceiling at `High` confidence is the same wall reached sooner.
**Response:** the rung steps out of candidacy while a harder doable variation
of the same pattern + primary group exists, measuring that successor becomes a
need of its own (without this the step stayed a nagging notice for weeks while
the cover kept covering the group with other trusted work — the first
implementation did exactly that), and the step is said out loud until the
successor has an estimate. The traces now show the sequence a human coach
would run: *"Lateral raise has stopped progressing — stepping up to single-arm
overhead press"*, an Assess of the press in the same session, and the press in
the work rotation two days later. Other rungs taken: single-leg hamstring
curls → Nordic eccentric, Dead bug → knee tuck.

## R4-3. A coarse rack turned the estimate into a phantom miss — SUPERSEDED (round 5)

The overhead press estimate (≈5.8 kg e1RM, measured at 5 kg × 5) computed a
working load between the owned 4 kg and 5 kg bells, and snapping to the
*nearest* rung chose 4 kg — where the rep range's top can only demonstrate
≈5.3 kg. Every session read as a miss **no matter how well it went**; misses
held progression, two stepped down, three re-opened the measurement, and the
loop repeated. Fixed at the time by rounding the working load **up** between
rungs, so the reps could demonstrate the estimate. **Round 5 reverted that** and
fixed the cause instead — the phantom miss was the ledger judging sessions
against the ceiling rather than the ask (R5-1), and once the ask is judged at
the load actually used, the nearest rung is honest. Rounding up turned out to be
actively harmful: it asked for reps below the mode's range, and it made the load
oscillate between two rungs session after session. The diagnosis here was right;
the fix was a workaround.

## What round 4 confirmed is right

- **The badweek regression is answered exactly as designed.** Week 3's dip
  produced genuine misses, and the trace shows the ledger working: hold, then
  a rung down, met at the reduced number, rebuilt as the athlete recovered.
- **The ladder picks the rungs a human coach would name** — no absurd
  step-ups; pattern + shared prime mover + next difficulty is a good
  definition of "the harder version of this".
- **Family dedup holds in the real corpus**: the back-test no longer plans
  Pull-up (L-sit) beside Pull-up (bar), and a cold-start day assesses one
  Dips variant, not two.

## Surfaced, not fixed

- **No rest day in 56 days**, all three temperaments. The volume model spreads
  weekly need across daily micro-sessions, and in production it is biometric
  readiness (absent from the simulation) that scales down-days. Worth watching
  in real use before inventing a deload scheduler the reactive design doesn't
  want.
- **A rack ceiling still has no voice.** An athlete whose true curl outgrows
  the heaviest dumbbell owned just tops out; the plateau will eventually hand
  over to a harder variation where one exists, but "you've outgrown your
  heaviest bell" would be a better sentence. Kit-limit notices are future
  work (G9 territory).
- **`difficulty` is now load-bearing.** The ladder reads it, so a wrong rung
  is a wrong step-up; the values deserve an authoring pass per pattern +
  primary group (see [todo](todo.md)).

# Round 5 — the coach marking its own easing as your failure

Round 4 closed with one thing flagged and unexamined: an eased session might read
as a miss. It does, and it was worse than flagged. The test is one an engine of
pure functions makes cheap — play a session through `evaluate`, have an athlete do
**exactly** what the card says, and ask the ledger what it made of that:

| session | the card | complied | ledger said |
| --- | --- | --- | --- |
| normal day | 5 @ 40 kg | 5 @ 40 kg | Met |
| low-readiness day | 5 @ 37.5 kg | 5 @ 37.5 kg | **Missed** |
| back-off day (after 2 real misses) | 6 @ 35 kg | 6 @ 35 kg | **Missed → streak 3** |

## R5-1. The back-off fed itself — FIXED

The third row is the serious one. Two genuine misses ease the ask down a rung to
rebuild from; the eased session then reads as miss number three, which trips
`REMEASURE_AFTER` and throws the exercise back to calibration. So the "back off
and rebuild" behaviour written up in round 4 **never rebuilt** — every real slump
slid into a re-measurement, and the rung it backed off to never got the chance to
prove anything. The badweek simulation hid it: that athlete's true strength really
did drop, so the misses looked earned.

Cause: the ledger compared each session against the athlete's **ceiling** (the
ability estimate), while the engine deliberately prescribes **below** the ceiling
whenever it eases — and a set logged without an RPE is taken at face value, as if
it were taken to failure. The ledger could not tell "eased on purpose" from "fell
short". Same family as R4-1 and R4-3: the coach not understanding its own
instructions.

Fixed: the ledger reconstructs **what was asked** and judges compliance with it.
It walks sessions forward, so at each step it holds exactly the feedback the engine
held that morning, and it recomputes the ask at **the load actually logged** — so
the weight rack never has to be reconstructed, and an improvised weight is judged
honestly rather than as a shortfall. The dose constants both sides read moved into
`dose.rs`: two copies would have the coach asking one number and the ledger marking
another. A genuine shortfall against an eased ask still escalates — the fix must not
buy its calm by going deaf.

Missed cards across the eight-week traces: improver 53 → **28**, plateauer 62 →
**42**, badweek 62 → **43**, while more distinct movements got trained (45 → 47) and
spurious re-measures fell. The back-test moved only where R4-3 was reverted.

## R5-2. A badly-slept night was recorded as your failure — FIXED

The second row of that table: low readiness eases the ask too, and readiness comes
from health-sync, so it was not reconstructible from set history. The coach asked
for less because the athlete was under-recovered, then marked them down for
complying — and because a miss holds progression, sleeping badly on Tuesday made
Thursday's ask smaller too.

Fixed across both services. health-sync grew
`GET /internal/recovery/history?from&to`: the same raw streams as `/internal/recovery`
(which already queried 28 days and discarded all but the summary), projected over a
range and answered **as of** each day, so nothing the backfill has since filled in
leaks backwards into a morning that didn't know it. Both endpoints now share one
implementation. Coach composes the score from those raw numbers exactly as it does
for today — health stays unopinionated, so there is one definition of readiness —
and hands the ledger what the coach knew each morning. A day health can't answer for
is judged full-effort: a missing signal must never invent an easing that didn't
happen, and that mirror case is tested too.

Rejected on the way: coach storing the readiness score it computed, per day. It
would be zero-drift by construction, but it makes readiness the one input that stops
re-deriving when the formula is tuned while every other number in the engine moves —
and when a past morning's sleep data is corrected, that morning's readiness genuinely
*was* different from what we believed. Re-deriving from the source of truth is the
honest answer.

## What round 5 confirmed is right

- **The simulator earns its keep.** Every bug in rounds 4 and 5 lives in the loop
  between the coach's own prescription and its own reading of the result; no unit
  test and no back-test can see that seam, because replayed history never responds
  to the coach.
- **A fix that needs a workaround elsewhere is the wrong fix.** R4-3 was a real
  diagnosis with a symptomatic cure, and it survived a back-test, a full suite and
  three simulated futures. What exposed it was fixing the actual cause and watching
  the workaround start fighting it (the convergence property test caught the load
  oscillating between rungs).

# Round 6 — one athlete was never the test

Rounds 4 and 5 ran three athletes: `improver`, `plateauer`, `badweek`. All three
are the *same person* — Pippijn's own logged history — on three ability curves,
and all three do exactly what the card says, on every day it says to. That is one
cell of a two-axis space, and it is the cell least likely to break anything.

So round 6 opened both axes. **Temperament** (how strong they are, and where that
goes) gained `novice` — opens at 55 % of what the history claims and climbs fast,
the deconditioned return or the app changing hands; `strong` — opens at 250 %,
someone who trained elsewhere all year and only started logging last week; and
`injured` — an improver whose shoulder goes in week 2 and stays gone, 40 % on
every movement the deltoids lead. **Behaviour** (whether they do what they are
told) is new, because a set that fell short and a set that never happened reach
the ledger as different signals: `skipper` trains three days a week whatever the
plan says, `partial` leaves after 60 % of the cards, `overachiever` doesn't stop
at the ask, `improviser` grabs the bell below the one on the card, and `layoff`
trains a fortnight then vanishes for three weeks.

Fifteen cells (`scripts/simulate-matrix.sh`), eight weeks each, same soil. The
compliant baselines reproduced round 5 (improver 29 missed cards against its 28),
so the engine has not drifted. Everything below is what the other fourteen cells
found. **All of it is open.**

## R6-1. A weighted lift cannot progress at all — FIXED

The `strong` athlete is two and a half times stronger than the coach believes and
does exactly what it asks. Over eight weeks the coach asked for **9 reps at 9 kg,
six times**, and never once asked for ten:

```
Biceps curl (one arm):   10@9  9@9  9@9  9@9  9@9  9@9     true e1RM 30.0 → 33.1 kg
Triceps extension:       10@4 10@4 10@4 10@4 10@4          estimate frozen at 11.7
Row (both sides):         9@9  9@9  8@9  7@9  7@9          the ask *fell* while complying
```

Across all fifteen traces, a weighted load stepped **up** 0–1 times per eight-week
run for every athlete who did what they were told, and stepped **down** at least
as often as up in every one of those cells. The
`strong` athlete's loads moved neither up nor down: 0 and 0, frozen solid. The
only cells where loads climbed are the two `overachiever` runs (5 up / 1 down,
4 up / 0 down) — because exceeding the ask is the sole way new information ever
enters the system.

The cause is a fixed point, and it is exact. `prescribe` picks the load whose
*top-of-range* reps the estimate supports: `load = e1rm / (1 + high/30)`. An
athlete who does `high` reps at load `L` produces `e1rm = L × (1 + high/30)` — which
prescribes `L` again, forever. Double progression's ceiling reproduces its own
floor. The documented rule ("the load steps only when logged sets raise the
estimate past the next owned weight") describes a state the loop cannot reach.

What makes it a fault rather than a trade-off is the asymmetry with every other
metric. `Loaded::Reps` probes with `best + 1`. `Loaded::Hold` probes with
`base + HOLD_STEP_S`. `Loaded::WeightedHold` tops out its clock and then takes
`inv.next_above(c.load)` — the exact shape that is missing. `Loaded::Weighted`
alone has no `next_above` anywhere in its branch; its "probe" only swaps
`floor(raw)` for `round(raw)`, so it can never ask for more than the estimate
already supports. The one downward path (`back_off` → `next_below`) works fine.
The rack has a way down and no way up.

**Why five rounds and a convergence test missed it.** `tests/athlete_sim.rs`
asserts exactly this property — "the estimated e1RM climbs to within a few percent
of true and stays there" — and it passes. It passes because its athlete logs
`rpe: Some(rpe)` on every set, and the estimator is RPE-aware:
`e1rm = load × (1 + (reps + rir)/30)`. A compliant athlete stopping short of
failure has `rir > 0`, which lifts the estimate *above* the load that produced it
and breaks the fixed point. Without RPE, `rir = 0` and the estimate lands exactly
on it.

But the product never collects RPE — deliberately, and it is a stated principle:
"the athlete reports what happened, not how it felt… he is never asked to rate his
own exertion". `simulate.rs` logs `rpe: None` for that reason, which is why the
matrix sees the freeze and the unit test does not. **The one test that certifies
"converges on a good trainer over time" is certifying a mode the app does not
run in.** Whatever fixes R6-1, that test needs a no-RPE case alongside its
current one, or it will keep passing while the shipped engine stands still.

**The fix: the rung belongs to the ledger.** A weighted lift now progresses along
a `Rung` — the weight the coach last sent the athlete to, and the reps shown there.
Reps climb at the rung; the weight steps only once they top the range and a probe
is due, and then the reps restart at the floor. The rule itself is a single
function (`dose::weighted_ask`) called by both the coach and the ledger, because
the two asking different numbers is the failure this area keeps rediscovering
(R4-1, R5-1, and this).

*Where* the rung is derived from is the entire difficulty, and two obvious answers
are both wrong. Deriving it from `e1rm`, as before, is the fixed point. Deriving it
from the athlete's **latest session** escapes the fixed point — and was tried first
(branch `r6-1-weighted-progression`) — but hands the weight to the athlete: the
coach follows a bad patch, or a lighter bell picked up because the right one was in
use, straight down, and the miss ladder stops escalating because every shortfall
becomes the next target. Six `pacing_engine` tests pinned exactly that and went red.
Any *max*-based rule fails differently: it either anchors on an old heavy single or
discards a genuine step up as "less work than before".

The rung is a fact about what the **coach** asked, so it is derived by replaying the
coach forward — which `residual::ledger` was already doing for outcomes. It now
carries the rung alongside them, and takes the location's weights so it can name the
rack it is judging against. The athlete moves it only by doing more *at that weight*;
work at some other weight leaves it where it is.

Two smaller rules fell out and are load-bearing:

- **The baseline never follows a short session down.** A shortfall is re-asked once
  (the hold) and a second one steps the rung down — the ladder, working as designed.
  Letting it track the athlete is precisely what broke the first attempt.
- **On the lightest weight owned there is no rung to drop to**, so a back-off takes
  reps off the ask instead, below the range floor if needed. Without this an injured
  athlete sat at `6 @ 1.5 kg` doing 3, with nowhere for the coach to go.

Measured on the matrix: the `strong` athlete goes from **0** weighted lifts with a
growing ask to **7**, with the ledger's own miss streak unchanged (worst 1, sum 2).
Good mornings walk `15 → 16 kg` and triceps extensions `4 → 5 kg` on the improver
too. Elsewhere the worst ledger streak rises from 1 to 2 — the back-off firing once
and recovering, which is the response working rather than a grind.

**A correction to the round-6 write-up above.** The real back-test showing
`Row (both sides): 10@9` on six consecutive days was cited here as the fixed point
appearing in Pippijn's own logged data. That was overstated: the back-test replays
history that never responded to the coach, so six identical *suggestions* are equally
consistent with the movement simply not having been taken. The trace is unchanged by
this fix, exactly as `trainer.md` predicts it must be — "replayed history never
responds to the coach" is why E3 exists. The simulation is the evidence for R6-1; the
back-test cannot be.

Two second-order effects fall out of the same place. The `floor(raw)` on a
consolidation session drops any fractional rep every time, so the ask erodes —
`9 → 8 → 7` on the strong athlete's row, a coach concluding you are getting weaker
*because* you did what it asked. And the erosion then hides the problem from the
plateau detector, which fires on the rep range's ceiling: an ask that has drifted
to 9 out of 10 never looks topped out, so the variation ladder never gets its turn
either. For `Biceps curl` both variants are `difficulty: 1`, so there is no harder
rung to step to even in principle — that movement is terminally stuck.

## R6-2. The miss response can't tell a near-miss from a rout — FIXED

The `novice` opens at 55 % of what the history says, which is what a new user with
someone else's data, or a return from three months off, actually looks like. The
bodyweight side handled it well (see below). The weighted side did this:

| session | the card | did | the coach's answer |
| --- | --- | --- | --- |
| 1 | 10 @ 9 kg | **1** | hold — same ask, 8.5 kg |
| 2 | 10 @ 8.5 kg | **1** | step down — 7.5 kg |
| 3 | 10 @ 7.5 kg | **1** | re-open the measurement |
| 4 | build up to a hard 5 | 5 @ 6 kg | correct, at last |

Three sessions and six sets of a weight the athlete can lift *once*, and the
first response to a 90 % shortfall was to take 0.5 kg off and ask again. The
ledger counts misses — one holds, two step down, three re-measure — and a miss is
a boolean. Missing ten by one rep and missing ten by nine are the same event.
Triceps extension ran the identical sequence at 4 → 4 → 3.5 kg.

The count-based escalation is right in shape; it just has no magnitude term. A
session that comes in at a fraction of the ask is not a bad day, it is a wrong
number, and the athlete has already supplied the correction.

**Fixed** by giving the ledger a fourth outcome. A session delivering less than
`ROUT_FRACTION` of the work asked is a `Rout` rather than an ordinary `Missed`, and
one of those re-opens the measurement on its own instead of waiting for three.

Two details carry the weight. It is measured as **volume** — load × reps, seconds,
reps — not as the Epley figure the miss bands are computed from: one rep of a weight
against ten of it is a 22 % difference in implied 1RM and a 90 % difference in what
the athlete managed, and only the second number is something a coach reacts to. And
the threshold is a **third**, pinned between two real cases: the novice managing one
rep of ten is about a tenth, while dropping from 40 kg × 5 to 30 kg × 5 against a
ten-rep ask is a little over four tenths and has to stay an ordinary miss — that is
precisely the shape the hold → back-off → re-measure ladder exists to walk.

The novice's curl now reads `asked 9 @ 9 kg, did 1` → **measure**, where it used to
grind three sessions and six sets first. One repeat survives, and it is R6-3's
doing rather than this one's: the honest low measurement that comes back is
discarded by the max, so the next prescription is nearly as heavy again.

## R6-3. Getting weaker is unrepresentable, and the loop never closes — OPEN

The worst result in the round. The `injured` athlete loses 60 % of the deltoids in
week 2 and never gets it back. At the end of week 7:

```
Face pull:      belief 7 reps (true 5)   [High]   miss-streak 15
Reverse fly:    belief 7 reps (true 5)   [High]   miss-streak 13
Lateral raise:  belief e1rm 2.7 (true 1.7) [High] miss-streak  9
```

**Fifteen consecutive missed sessions**, and the estimate has not moved — while
the confidence label still reads `High`. Face pull was sent back to calibration
**13 times** in eight weeks; in the healthy improver run no movement is assessed
more than once. The coach spent the second half of the run measuring the same
three movements over and over and learning nothing from any of it.

It is a closed loop, and every step of it is working as designed. Ability is a
**max** over decayed estimates, so a genuine decline cannot lower it — the honest
low measurement is discarded by the same `max` that protects a real old PR. Decay
floors at 60 % of the peak; the injury is at 40 %, so *time cannot fix it either*.
The block reset needs an 8-week gap that never comes, because the athlete keeps
turning up. Three misses re-open the measurement, the measurement returns 4, the
max keeps 8, the next prescription asks 7, and around it goes.

The control is in the same trace: **Overhead press**, the movement the ladder
stepped up to *after* the injury, sits at belief 1.7 against true 2.5 with a
miss-streak of **0**. Same athlete, same joint, same fortnight. The movement with
no pre-injury history is fine; only the ones with history are broken. That isolates
the cause to the estimator, not to anything about the injury.

`trainer.md` already names this shape — it is the argument for the load
plausibility check, "a mistyped load becomes a PR the engine cannot unlearn". The
round-6 finding is that a mistyped load is the *benign* version. An injury, an
illness, a bad year, or simply getting older produces the same unlearnable ceiling,
and none of them are input errors anyone can validate at the log sheet.

**R6-2 left a residue that belongs to this.** A rout now re-opens the measurement
after one bad session instead of three, but the honest low number that comes back is
still discarded by the max, so the next prescription is nearly as heavy again and the
novice takes a second rout before it settles. Whatever fixes R6-3 closes that too.

### The approach to try

**Cap ability at a multiple of recent demonstrated best** — roughly 1.5× the best set
of the last three or four sessions. Ability stays a max over decayed estimates; the
cap only ever lowers it.

- It is a **pure function of set history**, which the obvious alternative is not. The
  tempting move is to let a sustained miss run split the training block, reusing the
  layoff machinery — but the block split lives in `ability::estimate` and the miss run
  lives in the ledger, and the ledger *calls* the estimator. That is circular; this
  isn't.
- **One bad day still cannot drop you**, because the cap reads the best of several
  sessions rather than the latest. That is the property the max exists to provide, and
  it survives.
- **A layoff is unaffected** — the recent window is all pre-layoff, so nothing moves —
  while a genuine decline pulls the ceiling down within a few sessions.
- Both pinned invariants hold by construction: the estimate still never exceeds the
  best real set, and still never rises with more idle time, since a cap is monotone
  downward.

Not yet built. The number (1.5×, three or four sessions) wants choosing against the
matrix rather than guessing — too tight and it will chase noise, too loose and the
injured athlete stays stuck.

## R6-4. The plan never learns that you always leave early — OPEN

`partial` leaves after 60 % of the cards, every session, for eight weeks. Session
order is by movement breadth, and it is deterministic, so the tail is the same
movements every time. **Pallof press and Body saw were offered 20 times each and
performed zero times.** Nine movements were offered and never once done.

The abandoned cards correctly do *not* count as misses (27, against the compliant
run's 29) — silence is not failure, and the ledger holds that line. The fault is
upstream: nothing notices that the last two cards have a 0 % completion rate over
eight weeks. Worse, it self-reinforces — a group that never gets trained keeps its
deficit at maximum, so the cover keeps selecting it, and its low breadth keeps
putting it last, where it keeps not happening. A human coach who watched you walk
out before the core work twenty times would move the core work to the front.

### Attempted 2026-07-27: a group-level signal, and why it can't work

The appeal of the paragraph above is that it names the correction — *move the
work to the front* — without needing the offered plan stored. If a group is
chronically starved, promote its movements out of the tail; whether they were
never offered or offered-and-never-reached, the fix is the same. No schema, no
write path.

Built it, measured it, **reverted it**. Three cuts, each measured against the
matrix, each a no-op:

1. **Starved = zero volume across the trailing history.** No real athlete meets
   this. On the prod dump the thinnest groups still had 1–6 sets in eight weeks
   (Lower back 1, Abdominals 2, Adductors 2). Traces byte-identical.
2. **Starved = weekly rate under 15 % of target.** The flag fired, and still
   nothing moved: on a real athlete the *whole tail* is thin, so it all promoted
   together and the stable sort preserved its order relative to the non-starved
   isolation work it was joining. Traces byte-identical again.
3. **Sort key `(tier, !starved)`** so a promoted movement overtakes within its
   new tier rather than merely joining it. Byte-identical a third time.

The instrumented run says why. Mid-simulation the thinnest groups sit at
**55–65 % of target** — `Deep core 3.00/5.4`, `Obliques 3.50/5.7`. Nothing is
starved at any threshold worth having; a 50 % threshold fires on nothing.

**The groups are not neglected. The movements are.** Abdominals gets its volume;
*Body saw* is what never happens. The clearest case is `Pistol squat
(Quadriceps)` — offered 8 times, performed 0 — while Quadriceps is the
best-served group in the whole dump at 24 sets. No group-level statistic can see
that, because at group level there is nothing wrong.

So R6-4 needs the offered plan persisted after all. "Offered 20 times, performed
0 times" is a fact about *cards*, and a card is not derivable from the set
history — the set history is, by construction, the record of what did happen.
The write path also has to record only plans the athlete actually saw, or the
Android geofence poller will manufacture skips on days he never opened the app;
the rule that avoids this is to count non-completion only on days with sets,
which is exactly the population this finding is about.

`scripts/sim-neglect.py` came out of the investigation and stays: it turns a
trace into offered-vs-performed per movement, which is the number the run
summary can't give ("128 cards abandoned" doesn't say whether that was 128
movements once each or the same seven every session).

## R6-5. Readiness bottoms out and still books a full session — OPEN

R4 surfaced "no rest day in 56 days" across three temperaments and left it, on the
grounds that biometric readiness was absent from the simulation and would scale
down-days in production. Round 6 supplied the readiness and swept fifteen cells:
**every one of them trained on every available day. 15/15, zero rest days.**

Readiness does work as far as it goes — through the poor-sleep week the plan
shrinks from 5–8 movements to 3–4. But the score sits at **0.00**, the absolute
floor, for seven consecutive days, and the coach still programmes a session on
every one of them — including an 8-movement day on 2026-08-01, bigger than several
of that fortnight's fully-rested days. The one input designed to say *not today*
is pinned at zero and still says yes.

## R6-6. The coach follows an improvising athlete down the rack — FIXED

`improviser` takes the bell below the one on the card and completes every ask.
No misses, an improving athlete — and the coach walks the weight down:

```
card 9 kg → used 8.5 → card 8.5 → used 7.5 → card 7.5 → used 6.5
card 15 kg → used 14 → card 14 → used 13.5 → card 13.5 → used 12.5
```

R5-1 made the ledger judge the ask *at the load actually logged*, so these sessions
are correctly not marked as failures. But the **ability estimate** still reads the
lighter set at face value, the heavier bell is never touched again, and the max
decays out from under it. This is R6-1 seen from the other side: the estimate can
only ever be as good as the hardest thing the coach asked for, and the coach only
ever asks for what the estimate says. The strong athlete shows the loop as a
freeze; the improviser shows it as a descent. Nothing anywhere applies upward
pressure, and nothing ever says "the card said 15".

**Fixed by R6-1's rung.** The weight is now the coach's, not the athlete's: work at
a weight other than the one on the card leaves the rung exactly where it was, so
there is nothing to walk down. What an improvised session *does* do is fail to pay
down the work asked — judged as work rather than as a rep count, since "same reps,
lighter bell" is not compliance — which feeds the ordinary miss ladder. So a run of
lighter sessions still eases the rung, but by the visible route ("eased off" reads as
a decision), not by silent tracking.

(This behaviour is deliberately adversarial — a real athlete would not do it every
session for eight weeks. It is here because it makes the direction of the loop
visible, not because the athlete is the problem.)

## What round 6 confirmed is right

- **The bodyweight side coaches the novice well.** Misses were answered within a
  session or two, and the *downward* ladder fired unprompted and correctly:
  "Pull-up (bar) is too hard to build reps on right now — stepping back to Row
  (both sides)". Rep, hold and carry work all have real probes and all of them
  climbed; R6-1 is specifically a weighted-lift fault, not a general one.
- **Silence is not read as failure.** The skipper (32 days away), the layoff
  (21 days away) and the partial athlete (121 abandoned cards) all logged miss
  counts in line with the compliant run. The engine does not back off from an
  athlete who simply wasn't there — the R5 rule held under three new kinds of
  absence.
- **The return from three weeks off is calm and correct.** Every card in the first
  session back was met; staleness decay had the ask exactly where it should be.
  (The simulated athlete does not detrain over the layoff, so this tests the coach's
  re-entry after silence, not its handling of lost fitness.)
- **The injury's step-up was the right call.** Faced with a shoulder that stopped
  responding, the ladder moved to Overhead press and estimated it correctly from
  scratch. The response was right; only the estimator behind it was stuck.
