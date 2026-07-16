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
all for Chest/Lats/Triceps/Hamstrings/…) remain the standing "12 warm-up
drills" item in [todo](todo.md).

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

## R3-1. No plausibility bounds on logged values — OPEN

A fat-fingered edit (35 → "3530", an append instead of a replace) logged a
**12 kg farmers walk of 3 530 seconds** — a fifty-nine-minute carry — and
nothing blinked. The set was stored and would have fed the carry ability
estimate as a demonstrated max. A coach hearing "I carried it for an hour"
asks you to repeat that with a straight face.

The metric-shape validation (R2-1) checks *which* fields a set carries, not
whether their values are humanly possible. Wanted: per-metric sanity bounds at
the API (reps, seconds, kg each have a ceiling no honest set exceeds), with
the client surfacing the rejection kindly — or, gentler, an outlier check
against the athlete's own history that asks before storing.

## R3-2. The budget remainder under-doses a movement — OPEN

The plan carried "Push-up — 1 set" mid-session: the cover's last pick got
`min(MIN_WORK_SETS, budget left) = 1`. The engine's own constant says a lone
set of a work movement wastes its setup — the remainder should instead top up
an already-planned movement (whose marginal value was just re-ranked) or the
budget should round to full doses.

## R3-3. Movement families aren't deduplicated — OPEN (known)

Farmers walk *and* Farmers walk (waiter) were both planned — cousins differing
only in where the kettlebell sits. Same class as Hamstring curls vs its
single-leg variant in round 1; the standing family-dedup item in
[todo](todo.md).

## Automation note

One warm-up drill was logged twice and another skipped mid-round: the exercise
dropdown auto-scrolls to keep the selected option visible, so scripted taps at
remembered coordinates hit neighbours. Recovered in-flow (the card capped the
duplicate; the skipped drill logged from its own card). Not an app defect —
but it is the same "targets move under a committed tap" family a rushed human
thumb meets, and the plan-first picker section made the recovery easy.
