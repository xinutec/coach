# To-do

Work that's agreed but not built. The trainer model's own gaps live in
[trainer.md](trainer.md) — this is for everything else, and for things waiting on
data only Pippijn can supply.

## Build next

### History: a daily view that groups by exercise

The history page lists one row per logged set, so a session reads as "Triceps
extension, Triceps extension, Triceps extension" — three rows saying one thing. A
day should collapse to **one row per exercise with its set count**: `Triceps
extension — 3 sets · 6 reps · 5 kg`, not three identical lines.

Deliberately *not* in scope: grouping by time. Rounds and rest gaps are inferable
from timestamps and there is probably something useful in that later (was it a
circuit? did the sets cluster?), but guessing at it now would invent structure the
log doesn't actually record. Count the sets; leave the clock alone.

## Waiting on Pippijn

- **Demo videos** for the movements that have none — `./scripts/coachctl.py todo`
  is the live list. A movement is tracked without one (that's deliberate; see the
  catalog notes in trainer.md), but a missing demo shouldn't become permanent.
- **The cable stack's pin ladder** at the office. The kit is registered but has no
  weights, so the coach drops all five cable movements and says so. One line of
  `coachctl weights` fixes it.
- **The 12 muscle groups with no warm-up drill** (trainer.md, G5) — the largest
  hole in the warm-up block, and pure catalog authoring.
