# To-do

Work that's agreed but not built. The trainer model's own gaps live in
[trainer.md](trainer.md) — this is for everything else, and for things waiting on
data only Pippijn can supply.

## Waiting on Pippijn

- **Demo videos** for the movements that have none — `./scripts/coachctl.py todo`
  is the live list. A movement is tracked without one (that's deliberate; see the
  catalog notes in trainer.md), but a missing demo shouldn't become permanent.
- **The cable stack's pin ladder** at the office. The kit is registered but has no
  weights, so the coach drops all five cable movements and says so. One line of
  `coachctl weights` fixes it.
- **An authoring pass over `difficulty`** (1–5 per exercise). The variation
  ladder (G7, built in field-test round 4) reads it to pick "the next-harder
  version of this": same pattern, shared primary muscle group, next difficulty
  up. A wrong value is a wrong step-up, so the rungs within each pattern +
  primary-group family deserve a deliberate look — they were authored before
  anything consumed them.
- **Pictures for the 11 warm-up drills added 2026-07-16** (arm swing crossovers,
  scapular push-up, kneeling lat reach, scapular squeeze, cat-cow, biceps wall
  stretch, overhead triceps stretch, both leg swings, glute bridge, heel-toe
  rocks). They shipped image-less so every muscle group has a drill *now*;
  `scripts/add-image.py <slug> <url-or-file>` seeds each one. Demo videos are
  covered by the `coachctl todo` bullet above.
