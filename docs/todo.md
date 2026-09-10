# To-do

Work that's agreed but not built. The trainer model's own gaps live in
[trainer.md](trainer.md) — this is for everything else, and for things waiting on
data only Pippijn can supply.

## Waiting on Pippijn

- **Demo videos** for the movements that have none — `./scripts/coachctl.py todo`
  is the live list. A movement is tracked without one (that's deliberate; see the
  catalog notes in trainer.md), but a missing demo shouldn't become permanent.
  A generated 3D loop now covers some of these without a video; a real one is
  still better where you have it.
- **An authoring pass over `difficulty`** — REVIEWED 2026-09-10 and mostly fine.
  Read as ladders rather than as a list of numbers, the values are coherent
  (seated -> kneeling -> standing -> single-arm -> pike -> planche; rows ->
  pull-ups -> rings -> typewriter). Ties are frequently a real answer, not an
  undecided one. What the review DID find was a patterning fault, now fixed:
  nine movements filed under `core` whose prime movers are limbs, which split a
  ladder in two — the pistol squat was on a different ladder from the squat and
  so unreachable. Nothing outstanding unless a specific pairing feels wrong in
  use.
- **One tap on "Set home here & turn on"**, in the installed app, to confirm the
  status line flips to **On** by itself. The reminders card renders and the
  permissions are granted, which proves the message port is injected and the
  old-APK guard works — but the phone reporting the outcome when the flow settles
  is the half no test reaches. `BridgeTest` covers what the bridge admits and
  `settings.spec.ts` covers what the page does with an answer; neither can make a
  real geofence arm. If the line stays **Off** while the toast says reminders are
  on, the reply path is broken rather than the flow.

## Agreed, not built

- **A loop for each movement that has no demo video.** `coachctl todo` lists
  them; four are done. The pipeline is built (see below) and needs nothing from
  Pippijn — what it costs is pose authoring, roughly half an hour for a movement
  whose shape is new and minutes for one that reuses a shape already authored.
