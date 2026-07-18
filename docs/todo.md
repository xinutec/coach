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
- **Pictures for kneeling lat reach and heel-toe rocks** — the last two
  image-less exercises (the other warm-up drills added 2026-07-16 got theirs);
  `scripts/add-image.py <slug> <url-or-file>` seeds each one. Demo videos are
  covered by the `coachctl todo` bullet above. Superseded entirely once
  [anatomy-renders.md](anatomy-renders.md) reaches M5.

## Agreed, not built

- **Anatomy renders** — generate exercise illustrations from a 3D anatomical
  model, muscle colouring driven by the catalog. Plan and milestones in
  [anatomy-renders.md](anatomy-renders.md).
