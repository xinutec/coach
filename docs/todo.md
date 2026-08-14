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
- **An authoring pass over `difficulty`**, per pattern and primary muscle group.
  All 136 catalog exercises carry a value, but they were authored before round 4
  made the variation ladder read it. The ladder picks "the harder version of
  this" by pattern + shared prime mover + next difficulty, so two movements
  mis-ranked against each other now send the athlete a step they aren't ready
  for. Judgement about real movements, not something the code can settle —
  [field-test.md](field-test.md) flags it under round 4.
- **One tap on "Set home here & turn on"**, in the installed app, to confirm the
  status line flips to **On** by itself. The reminders card renders and the
  permissions are granted, which proves the message port is injected and the
  old-APK guard works — but the phone reporting the outcome when the flow settles
  is the half no test reaches. `BridgeTest` covers what the bridge admits and
  `settings.spec.ts` covers what the page does with an answer; neither can make a
  real geofence arm. If the line stays **Off** while the toast says reminders are
  on, the reply path is broken rather than the flow.

## Agreed, not built

- **Anatomy renders** — generate exercise illustrations from a 3D anatomical
  model, muscle colouring driven by the catalog. Plan and milestones in
  [anatomy-renders.md](anatomy-renders.md).
