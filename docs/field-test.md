# Field test — a simulated session, played as the athlete

On 2026-07-16 Claude played a full workout through the production UI as Pippijn
would: warm-ups, calibrations, work sets, honest fatigue (a target missed by a
rep late in the session), logged through the plan cards and the manual dialog.
Eleven sets across eight movements. This file records what a human athlete runs
into, each finding's root cause, and its status. Findings are ordered by how much
they damage the coaching, not by where they live in the code.

## 1. The plan is memoryless within a session — OPEN

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

## 2. Novelty churn — OPEN

The novelty cap bounds *concurrently pending* never-done movements, so each
completed calibration freed a slot and a new untried movement slid in — side
plank appeared, vanished unperformed, reappeared; toe raises materialised near
the end. By session end three untried movements were queued at the point of
maximum fatigue. Fix: count movements *introduced today* (first-ever set today)
against the cap, so finishing a calibration spends the slot instead of recycling
it.

## 3. Mid-session messaging — OPEN

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

## 4. The log dialog dies under your finger — OPEN

The background re-plan closed an open log dialog. Twice a "Log set" tap landed
on the bottom-nav **History** tab beneath it; once an edit didn't land and the
dialog silently logged the *prefilled* value instead of the typed one — wrong
data, not just lost data. And cards went stale in the other direction too: a
logged calibration kept offering "Log the calibration set" until a manual
reload.

Fix: an open dialog is never closed by a background refresh, and the sheet keeps
a safe margin from the bottom nav.

## 5. Warm-ups are inert — OPEN

Warm-up cards have no dose (how many? how long?), no way to be marked done, the
same generic copy on every card, and the "Next up" chip points at them all
session. Zero warm-up sets exist in the entire history because only the manual
+ dialog can log one. Fix: warm-up cards carry a dose, log from the card, show
done, and Next up advances past them.

## 6. Small UI faults — OPEN

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
