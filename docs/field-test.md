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

## R2-1. A hidden stale load was written into the record — OPEN

Switching the log sheet's exercise (biceps curl prefill → Squat sky reach) hides
the load field, but the component kept `loadKg = 4` and sent it: two bodyweight
mobility drills were stored as "10 reps · **4.0 kg**" (sets #322/#323). Nothing
on screen showed a load at log time — silent data corruption, and the server
accepted a load on a `reps`-metric exercise without complaint.

Root cause, two layers: the sheet posts whatever the (possibly hidden) fields
hold, and the API trusts it. Fix: the sheet re-derives visible fields from the
selected exercise's metric and nulls the rest on switch (stale reps are also
cleared — a 5 from dips must not become a hamstring-curl calibration), and the
server rejects load/hold values the exercise's metric cannot carry.

## R2-2. The day counter double-books its own plan — OPEN

The plan header finished at "**14 / 13 sets**". The denominator (13) excludes
the 3 warm-up slots; the numerator excludes the two mobility drills but *counts
the ramp-in curl* (it shares an exercise with a work item), so completing the
plan exactly reads 14/13 — and mid-session, after all three warm-ups, it read
1/13 while three cards showed Done. One plan, two bookkeeping rules. Fix: count
plan items — done and target both over the committed plan's sets, warm-ups
included, so finishing the plan is by construction N/N.

## R2-3. The warm-up doesn't warm up the session it precedes — OPEN

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

Fix: warm-up selection dedups by group and ranks groups by their load in the
committed plan, so the highest-loaded groups with any known drill get covered
first. The catalog gaps (no drill at all for Chest/Lats/Triceps/Hamstrings/…)
remain the standing "12 warm-up drills" item in [todo](todo.md).

## R2-4. Isolations are programmed before the compounds they sabotage — OPEN

Plan order: … biceps curl → triceps extension → **pull-up** → **push-up**.
Curling to near-failure immediately before pull-ups (and triceps extensions
before push-ups) is a sequencing error no human coach makes: the isolation
pre-fatigues the smaller muscle that the compound needs as a link, so the
compound reads artificially weak — and this session's compound numbers *are*
ability measurements. Fix: order work items compounds-first (multi-group
movements before single-group isolations of a muscle the compound uses).

## R2-5. Rest guidance is a shrug — OPEN

Two gaps a coach fills without being asked:

- "Rest a moment — then: 2 × Dips" fired **after mobility drills**. Warm-up
  sets need no rest gate; the prompt teaches the athlete to ignore the banner.
- "Rest a moment" never says how long. Strength work wants concrete numbers
  (2–3 min after a hard compound set, ~90 s after an isolation), and the gap is
  knowable from the set just logged.

Fix: no rest prompt after warm-up-kind sets; rest prompts carry a duration from
the logged set's kind (compound/isolation/calibration).

## R2-6. The range floor is the prefill — OPEN

Dips prescribed "3–12 reps" and the log sheet prefilled **3** (pull-ups: 4).
Anchoring at the floor invites the minimum; the range's low end is a bail-out,
not a target. Fix: prefill the expected reps — last comparable performance at
this ability, not `repLow`.

## R2-7. The banner and the plan disagree on "next" — OPEN

Fresh session: banner says "Next up: 2 × Dips" while the Next-up pill sits on
Squat sky reach (warm-up #1). The suggestion skips warm-ups; the pill doesn't.
A coach doesn't name two different next things. Fix: the banner names the
warm-up when that's what's next ("Warm up first — 10 slow Squat sky reaches"),
keeping one notion of "next" everywhere.

## R2-8. Small frictions

- The log sheet's exercise dropdown is ~120 items in **catalog order** — no
  search, no alphabetical order, no "today's plan first" section. Mid-workout
  selection is a scroll hunt. (The Library page has search; the sheet doesn't.)
- Dips headlined "(Serratus)" — the label falls to the neediest group, not the
  movement's prime mover; a coach says "dips: chest/triceps". Known label-drift,
  belongs with the need-ranked-headline work.
- Logging reflows the page under an open tap twice this round (a "Why this?"
  expansion shifted every button; a post-log refresh moved a card mid-tap and
  navigated to the Library). Refresh-in-place should preserve scroll/layout of
  untouched cards.
- Triceps extension has no catalog image (placeholder icon).

## What round 2 confirmed is right

The committed plan held perfectly through 16 sets — no re-prescription, no
ratchet, no vanishing movements, target fixed at 13, done counts landing on the
right cards in plan order (ramp-in credited before the work sets of the same
exercise). The sheet stayed open across a whole run of sets and its per-metric
fields adapt on exercise switch. Warm-up doses, "Measured — locked in", the
kit-blocked swap notes, and the closing "That's the session — nice work." all
behaved. The remaining gap to a human coach is knowledge (warm-up coverage,
ordering, rest), not machinery.
