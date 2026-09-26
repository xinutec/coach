# Anatomy renders

Exercise illustrations generated from a 3D anatomical model rather than sourced
one by one: one figure style, primary muscles dark red, secondaries lighter red,
and the colouring **derived from the same catalog data the engine uses**, so it
is never an artist's opinion.

Two artifacts per exercise, independent of each other:

- a **still** — every exercise has one. Most are sourced photographs; a few are
  renders.
- a **demo loop** — some have one. `data/catalog/loops/` is the set.

A loop sits beside the photograph rather than replacing it, in its own table and
found by convention, so a bad loop cannot cost an exercise the picture it already
has. The seeder reports both on every boot: `… image(s) and … loop(s) added`.

**What the colouring guarantees.** It comes from the catalog, so it is never
invented. It is not complete: the labelling gives a body vertex the muscle its
inward ray reaches first, so a muscle lying under another is barely carried, and
several exercises colour a primary faintly or not at all. A loop therefore ships
on the strength of its MOVEMENT — the photograph beside it carries the exercise.
Showing deep muscle areas properly is a separate piece of work.

## Why

The payoff is not coverage; sourced pictures already cover the catalog. It is:

- **Correctness by construction.** Highlighted muscles come from
  `data/catalog/exercises.json`. Editing an exercise's muscle map and
  re-rendering keeps the picture honest.
- **One visual style** instead of a scrapbook of stock-art sources.
- **Independence** from scraping (dead hosts, DNS blocks, format surprises).
- **New exercises never ship image-less** — a pose is the only authoring cost.

## Decisions

- **Base asset: Z-Anatomy** — an open-source Blender anatomy atlas with every
  muscle as a separate named mesh, built on the BodyParts3D dataset. Accuracy
  comes from the dataset; we do not sculpt a human ourselves.
- **License: CC-BY-SA is acceptable.** Derived renders inherit it; see
  Attribution.
- **Illustration quality, not biomechanics.** Muscles need to *read* correctly in
  a pose, not simulate.
- **Renders are judged by Pippijn.** Pose quality is a visual call; the loop is
  render → deliver → critique. Nothing ships to the catalog unreviewed.
- **Supplement, never replace.** The photograph stays exactly as it is and the
  loop is a second artifact beside it. The sourced pictures show what to do and
  took real work to gather.

## The two figures

### The écorché (Z-Anatomy)

The atlas ships as Blender application templates; the model is `Startup.blend`.

- 7,184 objects (4,569 meshes) in TA2 anatomical naming. Muscles are split per
  head (e.g. "Acromial part of deltoid muscle") with `.l`/`.r` sides and
  `.ol`/`.or`/`.el`/`.er` variants, so **one catalog slug maps to several
  meshes** — `render/muscle_map.json` is one-to-many.
- **No skin mesh.** "Regions of human body" is `.g`/`.j` label markers (text +
  leader lines), not a body surface. The écorché is bare muscle and bone.
- **No armature** in the anatomy file. Z-Biomechanics has a 237-bone armature
  aligned to the same skeleton, but muscles have zero vertex groups.

**The écorché is never posed.** It is dozens of separate muscle shells, so every
bent joint tears and interpenetrates them into non-human shapes whatever the
weights — structural, since no weighting fixes a mesh that is not continuous.
`render/rigging/` records the attempt. The écorché is the source of the region
names and renders the neutral-pose "muscles worked" still.

### The skinned body (fork C)

Everything posed or animated is a single MB-Lab body. Its muscle regions are
*derived* from the écorché, not painted: both figures stand in the same pose at
the same scale, each body vertex takes the Z-Anatomy name its inward ray (along
the skin normal) reaches first, falling back to the nearest surface where the
ray leaves the body, and `muscle_map.json` resolves a catalog slug onto those
names exactly as it does for the écorché meshes. The catalog still decides what
is red, and the thing that bends is one continuous surface. A label is a vertex
index, not a position, so the colours travel with the deformation.

- **Subdivide before labelling.** `label-body.py` applies the body's subdivision
  modifier first (17,996 → 276,437 vertices), after clearing MB-Lab's facial
  expression shape keys, which block applying a modifier. At cage resolution the
  anterior mid-thigh holds 176 skin vertices and the quadriceps read as a band;
  subdivided, the highlight takes the muscle's shape. Rendering 276k of real
  geometry is also faster than subdividing an 18k cage at render time.
- **Keep MB-Lab's deformation stack.** Corrective smooth, subdivision and the
  displacement texture are switched off while distances are measured and
  restored before saving. The armature deforms with **preserve volume** (dual
  quaternion): linear blend collapses a bent hip or knee inward and twists a
  forearm.
- **Light for form.** Key sun 2.1, fill 0.7, ambient 0.15: a key of 4.0 on a
  0.80-albedo surface clips to white and the figure reads as a plaster cast. The
  suns subtend 12°; Blender's default 0.526° disc lays hard-edged wedges across
  the torso that look like a second translucent body.
- **Remaining limit:** some skin vertices resolve to bone rather than muscle,
  the skin and the atlas figure being different builds that registration alone
  does not reconcile.

## Files

Shared (`render/`):

- `fetch-asset.sh` — download + unzip Z-Anatomy into a gitignored `asset/`
  (idempotent, so the CI cache skips it).
- `prepare.py` — strip the atlas to the muscular system + skeleton and save the
  gitignored `slim.blend`. Never point a render at `Startup.blend`: the slim
  blend is what keeps a runner from thrashing.
- `muscle_map.json` — catalog muscle slug → Z-Anatomy base names (side/variant
  suffixes matched automatically). `scripts/check-muscle-map.py` validates every
  name against real, non-label meshes.
- `za.py` — Z-Anatomy naming and the red/flesh palette, so the two renderers
  cannot disagree about what a picture means.
- `stage.py` — orthographic camera, sun rig and render settings. The two figures
  only compare under the same light.
- `floor.py` — the ground slab for floor poses.
- `render.py` — the écorché still: read the exercise's muscle roles from the
  catalog, colour via the map, render a PNG. Exits non-zero on a primary or
  secondary slug with no map entry rather than rendering an uncoloured lie.

Skinned body (`render/skin/`):

- `label-body.py` — registers the MB-Lab body onto the écorché and writes one
  `mus:`-prefixed vertex group per Z-Anatomy base name.
- `render-skin.py` — the posed still: same catalog, map and light as
  `render.py`, colour carried per vertex instead of per mesh.
- `poses.json` — named poses as per-bone XYZ euler degrees, with the rig's
  measured axis conventions, plus the `_contact` and `_floor` tables.
- `loops.json` — how each loop is built: its view and its `pose@frame` keys.
- `animate.py` — renders a loop.
- `collide.py`, `check-pose.py` — self-intersection check; reports every pose.
- `plant.py` — floor contacts and levelling.
- `try-pose.py` — authoring harness for candidate poses (`--render` for images,
  `--no-solve` for a cheap grid).
- `debug-regions.py` — colours each region distinctly, unlit, so the transfer can
  be looked at rather than inferred from a tally.

## Posing

**Probe the rig; do not reason about it.** The euler conventions are borrowed and
not guessable. A hinge tips the pelvis forward and counter-rotates the thighs,
and that counter-rotation has the SAME sign as the pelvis (`pelvis +62` wants
`thigh +65`), because the thigh's local X runs opposite to the pelvis's. A sign
guess costs a render sweep; perturbing one bone at a time and printing where the
contacts land costs a few pose evaluations and no renders.

Measured facts about this rig:

- **`stand` is an A-pose.** Rotating `upperarm` past about −50° on Z swings the
  hand *inward* as it descends, so arms-at-sides is not reachable on Z alone and
  every angle approaching it collides with the hip. The écorché stands the same
  way.
- **Overhead:** `upperarm` +Z raises the arm laterally; +85° clears the head;
  +78° with the elbow at −100° is hands-behind-head.
- **Scapular squeeze:** rolling the clavicles forward carries the hands inward
  onto the thighs, so the arms widen — but only to −38°; past about −44° they
  swing back in.
- **Mirroring is not a sign flip.** The right-arm curl needed its own sweep
  (`ua_x=55` where the left wanted 40). Author each side against the checker.
- **Full elbow flexion is out of reach.** Past about 90° the hand drives into the
  torso. The curl shows 90° with the upper arm swung forward.
- **A front rack cannot be posed.** Both hands at the shoulders puts the forearms
  in the same space at every combination swept (worst case 503 triangles), so
  `squat_front_rack_double_kettlebell` has no render rather than a plain squat
  that would misrepresent where the load sits.

**A pose has a view that suits it.** Arms held forward foreshorten into stubs
from the front; a squat only reads from the side. For a side view the camera's
width is the figure's y extent.

**Poses are shared, not per exercise.** A squat is a squat whoever holds what, so
a slug maps onto named poses through `loops.json`. A new shape costs about half
an hour (the RDL took two numeric probes and four renders); a shape already
authored costs minutes (`good_morning_dumbbell` reuses the hinge, and the catalog
colours it differently).

## Collisions

Posing has no collision: bones rotate, skin follows, and a limb swung into the
torso occupies the same space as it. `collide.py` intersects the posed skin with
itself, attributes each intersecting triangle to the bone that drives it, and
measures how far apart those two bones are along the skeleton. **The test is not
whether surfaces intersect but which ones do**: a deep squat legitimately presses
hamstring against calf (`squat` has 16,436 self-intersecting pairs and is fine),
while a hand seven joints from the pelvis sharing space with it is not.
Thresholds: 5 joints apart, 40 triangles. `render-skin.py` refuses an impossible
pose, and `check-pose.py` exits non-zero if any pose is impossible.

**Why a triangle count and not a depth.** Measured as signed distance over whole
regions, crossed forearms resting on each other (correct) reach 2.7cm, and hands
inside the pelvis (wrong) span 1.5–7.9cm. The ranges overlap, so no depth
threshold separates contact from penetration. The reason is structural: a
rigidly skinned mesh has no flesh compression, so two limbs at rest against each
other interpenetrate by about as much as real tissue would squash.

So legitimate contact is an explicit, named, per-pose exemption: the `_contact`
table in `poses.json` names the pose, the two things allowed to touch and the
reason, and applies to the interpolated frames of any sequence whose keys include
that pose. The rule stays at full strength for every other pose.

- ⚠ **State an exemption at LIMB level.** Which bone pair meets first moves as
  the arms do, so a bone-pair entry goes stale mid-animation. A
  `LIMB:clavicle_L` token expands through the skeleton's own child links: *these
  two arms touch*.
- ⚠ **An exemption is not a licence to stop looking.** With arm-vs-arm allowed,
  the render is the only remaining check for that pose. Arms posed symmetrically
  meet head-on at mid-swing; one arm crosses in FRONT of the other.

## Floor contacts

A standing figure meets the floor in one place, so the plant is one line: find
the lowest vertex and drop the rig until it touches. A pose that declares no
contacts takes exactly that path.

A floor pose rests on two or more: a glute bridge on the shoulders **and** the
feet while the middle rises; all fours on two hands and two knees. A
single-point drop lands whichever contact is lowest and leaves the rest in the
air.

**The pose says what it rests on.** `_floor` in `poses.json` lists each floor
pose's contacts as bone names — `spine03, foot_L, foot_R` is a bridge. Geometry
cannot tell a load-bearing contact from an incidentally low one. With the
contacts named, one angle is left free — how far the figure tips along its own
length — and `plant.py` finds it by **search over the `root` bone**, measuring
the actual skinned, corrective-smoothed mesh. A closed form would have to choose
the axis "along the body", which is z standing and y lying down; fixing it
silently rotates a squat by 27.9° and a hinge by 40.2°.

Two things look like solutions and are not:

- **Tuning a pose until the heights read right.** A body that has rotated as a
  whole satisfies hip, shoulder and foot heights and renders as a diagonal plank.
  Heights cannot distinguish a bridge from a ramp.
- **Levelling alone.** A pose with a wrong torso levels perfectly (+15.9° of tip,
  0.00cm spread) with the head 9cm underground.

Hence two guards:

- **A tilt cap (5°).** A correct pose needs well under a degree. Past the cap the
  tip is the figure rotating as a whole, reported as a fault in the POSE.
- **A through-floor check that NAMES the part.** "head is 9cm under the ground"
  is the answer; "something is" sends you looking at the whole figure.
- **A slide check (2cm).** Heights say a contact is ON the floor, not WHERE:
  hands can slide across the floor between keys while every height passes.
  `plant.span` is the widest horizontal distance between contact centroids (a
  flat palm's lowest vertex jumps from wrist to fingertip with a fraction of a
  degree of tilt), and every frame of a rep must hold it to within 2cm. Match
  it at the keys while authoring: `try-pose.py` prints it.

**The rig cannot lift a hip with one bone.** The hierarchy is `root → pelvis →
{thigh, spine01} → spine02 → spine03 → neck → head`, and each contact's measured
response to +10° of X is:

| bone | head | spine03 | pelvis | foot |
|---|---|---|---|---|
| root | +26.5 | +20.8 | **+14.7** | +5.0 |
| pelvis | +13.5 | +7.7 | +1.7 | −8.1 |
| spine01 | +10.2 | +4.4 | −0.4 | 0 |
| spine02 | +8.9 | +3.2 | 0 | 0 |
| spine03 | +6.7 | +0.6 | 0 | 0 |
| neck | +2.8 | 0 | 0 | 0 |
| thigh | 0 | 0 | 0 | +10.0 |

Only `root` moves the pelvis, and it moves the shoulders MORE: the pelvis is
upstream of the spine, so no rotation raises the hips relative to the shoulders.
The table also gives the answer: hold the shoulder contact still with
`spine01 = −4.685 × root`, and the pelvis rises 16.5cm per 10° of root while the
head falls 21.2cm; `spine03` and `neck` put the head back without touching the
shoulder contact. Four bones, one linear solve.

The same topology forces counter-rotation on all fours: arching the spine moves
the shoulders, so the arms must compensate, and only the magnitude is free. Too
much compensation cancels the visible curve — check that the two keys of a pair
actually look different. Arms alone cannot also hold the hands' distance from
the knees; a pelvic tuck can: rotate `root` and move the thighs by the same
amount (same sign, as in the hinge) so the knees stay put.

**Scapular movement is clavicle X.** In a plank, clavicle X raises and sinks the
upper back between the arms (6cm for −20°) while barely moving the hands;
clavicle Z and Y mostly swing the hands along the floor. A little Z cancels X's
residual slide.

⚠ **A hip height is a difference.** "Hips 14.1cm" with the hips flat on the
ground is the mean pelvis vertex of a thick body, not a lift.

⚠ **Judge a floor pose against a floor.** Against plain grey, a body on the
ground and one hovering 15cm above it are the same picture. And the views are
orthographic and horizontal, so a flat *plane* renders edge-on as a one-pixel
line — present in the scene, invisible in the frame. `floor.py` is a slab with
thickness.

## Animation

`animate.py` renders a seamless loop: keyframe the bones through `pose@frame`
keys, plant every frame, frame the camera on the union of every frame, and
render with EEVEE to MP4 through Blender's own FFmpeg at 12fps. A two-second rep
is 24 frames.

- **Check every frame, not just the keys.** Two legal poses can be joined by an
  illegal path: `stand` → `squat` passes at both ends and puts the fingers inside
  the thighs at frame 3. A `squat_reach` waypoint brings the arms forward while
  the legs are still nearly straight.
- **Plant every frame.** The hips drop through a rep, so one offset would leave
  the figure sinking and rising. Floor poses are levelled at each key and
  verified at every frame.
- **Frame on the whole rep,** or the figure drifts in and out of the composition.
- **Render every frame.** `frame_step` is set to 1 explicitly: the source blend
  carries 2, and the encoder then silently drops every other frame after every
  check has passed. Verify the result with `ffprobe` (`nb_frames`, duration),
  not by the render's exit status.

`--stills <dir>` writes PNGs of every key and midpoint for review.

**`loops.json` is how a loop is re-made.** Given only a slug and an output path,
`animate.py` reads the view and keys from it:

```
blender -b <blend> --python animate.py -- glute_bridge out/bridge.mp4
```

Every shipped loop has an entry, and a new loop is not done until it has one.
Only specs that have been rendered and passed the checks belong in it: a spec
that differs by a frame (`squat_reach@6` for `@5` on the squat) puts the fingers
of both hands through each other. A wrong entry is worse than an absent one,
because the file exists to be trusted.

⚠ **A loop `.mp4` cannot be compared by bytes.** The container embeds the
wall-clock time of the render, so two runs of identical code differ by a few
bytes inside the time string. The regression test is the REPORTED MEASUREMENTS
— planting, the per-frame checks, the framing bounds — compared line for line. A
`git worktree` at the old commit renders the other side against the same blend.

**In the app.** `exercise_loops` is its own table (migration 0027), so seeding a
loop cannot touch `exercise_images`. The seeder finds `data/catalog/loops/<slug>.mp4`
by convention and etag-compares it like an image. `GET /api/exercises/{id}/loop`
is ETag-cached for a year; `hasLoop` on the detail says whether to ask. The
sheet plays it muted and inline below the hero, `object-fit: contain`, because
the render is framed on the whole rep and cropping would cut the feet off.

## Render hosts and cost

**Écorché stills: CI only.** `.github/workflows/render.yml` is a manual
`workflow_dispatch` job on an ephemeral `ubuntu-latest` runner with a pinned,
cached Blender download; it uploads PNGs as an artifact for review, and only
approved images are committed into `data/catalog/images/`. The atlas does not
render on our machines: the Mac cannot build Blender from nixpkgs, and rendering
the full atlas on isis once exhausted its memory and took production down.

**Skinned body: by hand on the Mac,** against a downloaded Blender 4.2.3 in
`~/Applications`, the same version the workflow pins.

**Cost** at 768px, measured as the marginal frame within one Blender process
(which is what an animation pays):

| engine | first frame | steady |
|---|---|---|
| Cycles, 64 samples | 13.8s | ~21s/frame |
| EEVEE Next, 32 samples | 59.1s (GPU context) | **3.9s/frame** |

EEVEE is visually indistinguishable here — matte diffuse under two suns, with no
light transport for path tracing to win at. `COACH_RENDER_ENGINE=EEVEE` selects
it for stills. ⚠ Timing single-frame invocations says the opposite, because one
frame is dominated by loading the blend and EEVEE's one-off GPU setup.

**Determinism:** pinned Blender, fixed seed, fixed light and camera — an unchanged
pose renders the same image, so an image diff means something.

## Traps

**⚠⚠ Blender exits 0 when the script crashes.** `blender -b --python x.py` where
`x.py` raises prints the traceback and returns 0; an explicit `sys.exit(1)` is
honoured. So a green status means "blender ran", never "the check passed". These
scripts print one line per candidate or pose: count them, or grep for
`Traceback`.

**Faults that each produced a plausible wrong answer** — a contiguous, symmetric,
anatomically plausible segmentation. What exposes them is checking *which*
muscles are named, not whether the picture looks like a body.

1. **`.g`, `.j` and `.i` are annotation meshes** — many are zero-thickness planes
   well off the figure's axis. Invisible in a lit render, but ordinary surfaces
   to a nearest-surface query, and they wreck a bounding box.
2. **`bound_box` is pre-modifier.** Register from the actual posed vertices.
3. **MB-Lab parents the body mesh to its armature,** so transforming both
   compounds the scale; a body 9% short and sunk into the écorché labels neck
   skin as a deep strap muscle. `label-body.py` checks the registration and
   refuses rather than proceeding.
4. **A segmentation has to be read unlit.** The lit rig washes a saturated colour
   to white.
5. **A map entry can name an annotation.** `Erector spinae` in the atlas is a
   2-vertex label; the muscle is its iliocostalis, longissimus and spinalis
   parts. `check-muscle-map.py` validates names against real meshes.

**Z-Anatomy is authored for study, not rendering** (all handled in `render.py`):

1. Muscles ship `hide_render`; reset `hide_render`/`hide_viewport`.
2. Fascia, aponeuroses, retinacula, sheaths and membranes wrap the body as a
   featureless envelope; skip them by name.
3. Material slots are object-linked: replace every slot, set `slot.link =
   'DATA'`, reset every polygon's `material_index` to 0.
4. A compositor node tree + Freestyle bake a sepia "sketch" filter over every
   render (identical output across material edits is the tell):
   `scene.use_nodes = False`, `scene.render.use_freestyle = False`, clear
   `view_layer.material_override`.
5. Label and title meshes float in the frame; skip `.g`, all-caps and
   collection-title names.

Suns are camera-relative, so the visible surface is lit whatever the view.

## Not built

- **Props.** No dumbbell, barbell or bench is modelled; a loaded lift renders
  empty-handed.
- **Layered labelling** for muscles that lie under others (see "What the
  colouring guarantees").

## Attribution

Renders derive from Z-Anatomy (CC-BY-SA 4.0, github.com/Z-Anatomy), itself based
on BodyParts3D / Anatomography (CC-BY-SA 2.1 JP). The male figure lives in the
`Models-of-human-anatomy` repo; the atlas template in `The-blend`.

CC-BY-SA requires the credit wherever a derived work is shown. The exercise sheet
credits it in a line under every loop, and under any picture whose catalog entry
carries an `image.credit` (`text`, optional `url`). ⚠ **A rendered still gets its
credit only from that field** — nothing detects a render — so a new render
committed to `data/catalog/images/` needs one in `exercises.json`, as
`heel_toe_rocks` has. A new place that shows a render needs the credit too.
