# Anatomy renders

Generate the exercise illustrations ourselves from a 3D anatomical model instead
of sourcing them one by one: an écorché figure per exercise, primary muscles dark
red, secondaries lighter red, consistent style — and the colouring **derived from
the same catalog data the engine uses**, so an image can never disagree with the
muscle model.

Status (2026-07-19): **M1/M3 done and shipped** — the CI pipeline renders a
shaded, catalog-coloured, *unposed* écorché and all 136 exercises carry images.
**M2 (posing) is abandoned** — a borrowed rig *can* move the figure, but the
écorché is dozens of separate muscle shells, not one skinned mesh, so any joint
bend tears and interpenetrates them into non-human shapes (see M2). Decision
(Pippijn, 2026-07-19): **use the écorché only for muscle colouring in the neutral
pose; no posing/animation.** That is exactly what ships today — nothing further
to build.

## Why

134/136 exercises have sourced images today, so the payoff is not coverage. It is:

- **Correctness by construction.** Highlighted muscles come from
  `data/catalog/exercises.json`, not an artist's opinion. Editing an exercise's
  muscle map and re-rendering keeps the picture honest.
- **One visual style** instead of a scrapbook of stock-art sources.
- **Independence** from scraping (dead hosts, DNS blocks, format surprises).
- **New exercises never ship image-less** — a pose file is the only authoring cost.

## Decisions (approved)

- **Base asset: Z-Anatomy** — an open-source Blender anatomy atlas with every
  muscle as a separate named mesh, built on the BodyParts3D dataset. We cannot
  sculpt an accurate human ourselves; accuracy comes from the dataset.
- **License: CC-BY-SA is acceptable.** Derived renders inherit it; the app
  carries attribution (see below).
- **Illustration quality, not biomechanics.** The reference stock images are
  stylized; muscles need to *read* correctly in a pose, not simulate.
- **Renders are judged by Pippijn.** Pose quality is a visual call; the loop is
  render → deliver → critique. Nothing ships to the catalog unreviewed.

## M1 findings (2026-07-18) — asset inspected

Both archives are Blender application templates; the model is in `Startup.blend`.
Inspected headless on isis:

- **Z-Anatomy** — 7,184 objects (4,569 meshes) in TA2 anatomical naming.
  Collections: Skeletal / Muscular insertions / Joints / Muscular system (894
  objs) / Cardiovascular / Lymphoid / Nervous / Visceral / Regions of human body
  / Bonus. Muscles are split per head (e.g. "Acromial part of deltoid muscle")
  with `.l`/`.r` sides and `.ol`/`.or`/`.el`/`.er` variants, so **one catalog
  slug maps to several meshes** — the muscle-map is one-to-many.
- **No armature** in the anatomy file.
- **No skin mesh anywhere.** There is no integumentary layer. "Regions of human
  body" (343 objs) is `.g`/`.j` label markers (text + leader lines), not a body
  surface.
- **Z-Biomechanics** — a bones-only build: a real **237-bone armature** (plus
  `AnatPoseToTPose` / `TPoseToAnatPose` retarget rigs) aligned to the same
  skeleton. But it carries only the skeleton, and **muscles have zero vertex
  groups** — nothing is skinned to it. It is rigid bone-posing, not a
  muscle-deforming rig.

Consequences: catalog-driven muscle colouring is well-supported (every muscle is
a named mesh). The **skinned-figure aesthetic is not** — the asset gives an
écorché (bare muscle/bone), not the grey-skin-with-a-face look of the reference
stock art. Getting that look needs a separate body-surface mesh registered to
these proportions, which is a project of its own. This is a decision fork —
recorded below, awaiting direction. **Supersedes the earlier claim that
Z-Anatomy includes skin/face; it does not.**

## Aesthetic — OPEN (was: skinned grey figure)

The reference stock art is a grey **skinned** male figure with a face, target
muscles shown red. M1 established Z-Anatomy cannot produce that directly (no skin
mesh). Direction is undecided; options in "Decision fork" below.

## Decision fork (after M1)

- **A — Z-Anatomy écorché.** Bare muscle/skeleton, catalog-driven colouring,
  pose via the Z-Biomechanics skeleton (bind muscles ourselves). Best muscle
  accuracy, fully open-source, but a clinical/specimen look — does *not* match
  the reference art (no skin, no face).
- **B — Z-Anatomy muscles + a separate skin body.** Register a CC0/MakeHuman
  body surface to the Z-Anatomy proportions, composite muscles showing through.
  Matches the references, but adds a two-mesh registration sub-project that must
  hold through every pose — the largest option.
- **C — single skinned body, painted muscle regions.** Abandon separable meshes:
  one rigged male body (e.g. MakeHuman/SMPL), muscle regions painted as
  vertex-colour/texture, recoloured per exercise from the catalog. Directly gives
  the reference look and rigs trivially (one standard humanoid mesh), but loses
  per-muscle geometric precision. **The paint map does not have to be painted** —
  see "Fork C: the derived paint map" below.
- **D — pause.** 134/136 exercises already have sourced images; the marginal
  value is consistency, not coverage. Keep the M1 findings and revisit later.

## Fork C: the derived paint map

The objection to C was that hand-painting muscle regions hands the colouring
back to an artist's opinion, which is the one thing this pipeline exists to
avoid. It does not have to be painted. Both figures can stand in the same pose
at the same scale, and each body vertex can take the name of the anatomy whose
surface is nearest to it. muscle_map.json then resolves a catalog slug onto
those same Z-Anatomy names, exactly as it does for the écorché meshes, so the
catalog still decides what is red.

Built (2026-09-03), unposed:

- `render/za.py` — Z-Anatomy naming and the red/flesh palette, shared so the two
  renderers cannot drift into disagreeing about what a picture means.
- `render/stage.py` — orthographic camera, sun rig and Cycles settings. An
  écorché and a skinned body only compare if the light is the same.
- `render/skin/label-body.py` — registers the MB-Lab body onto the écorché and
  writes one `mus:`-prefixed vertex group per Z-Anatomy base name.
- `render/skin/render-skin.py` — the fork-C counterpart of `render.py`: same
  catalog, same map, same light, colour carried per vertex instead of per mesh.
- `render/skin/debug-regions.py` — colours each region distinctly so the
  transfer can be looked at rather than inferred from a tally.

`render.py` now shares `za.py` and `stage.py`. Excluding annotation meshes (see
below) moves its counts — visible muscle meshes 666 -> 581, head-fill bones
324 -> 87 — with no visible change: heel_toe_rocks re-renders with a solid head
and red calves as before.

**What it establishes:** a goblet-squat render on the skinned body puts red on
both quadriceps and neutral flesh everywhere else, from the catalog, on a figure
with skin and a face. That is the reference look the écorché cannot reach.

**What it does not:** region boundaries are ragged, because assignment is
per-vertex and hard on a 17,996-vertex body. 4,095 of those vertices resolve to
bone rather than to any muscle, the skin and the atlas figure being different
builds that height registration alone does not reconcile.

### Posing (2026-09-03)

**The body poses without tearing**, which is the structural failure that ended
M2 for the écorché. A squat bends hip, knee, ankle, spine, shoulder and elbow
with no interpenetration and no non-human shapes: one continuous skinned mesh
deforms where dozens of separate muscle shells could not. The muscle colours
travel with the deformation, because a label is a vertex index rather than a
position.

- `render/skin/poses.json` — named poses as per-bone XYZ euler degrees, with the
  rig's measured axis conventions written down in the file.
- `render-skin.py` takes a pose name as its fourth argument, and **stands the
  figure on the floor itself**: bending the hips and knees moves the feet but not
  the root, so a posed figure otherwise hangs in the air at whatever height its
  rest pose left it.

`label-body.py` no longer bakes the arms-down measuring pose as the rest. That
left the rig inconsistent — bones arms-down, mesh still in its authored T — and
the body then rendered in a T-pose whatever its bones said.

### Making the flesh move like flesh (2026-09-03)

MB-Lab's body ships a deformation stack — corrective smooth, subdivision at
render level 3, a displacement texture — and `label-body.py` switches all of it
off so the evaluated mesh keeps its vertex count while distances are measured.
It then saved the blend that way, so every render was the raw 18k cage under
plain linear-blend skinning. The stack is restored before saving now, and the
armature deforms with **preserve volume** (dual quaternion): linear blend
collapses a bent hip or knee inward and twists a forearm, and those are joints
every exercise uses. Subdivision also interpolates the colour attribute, which
is what smoothed the ragged region boundaries.

Two lighting faults, both of which read as rendering bugs rather than as light:

- **Exposure.** A key sun of 4.0 on a 0.80-albedo surface clips to white. The
  figure had no form at all — a plaster cast. Key 2.1, fill 0.7, ambient 0.15.
- **Shadow hardness.** Blender's sun defaults to a 0.526° disc. An arm held
  towards the camera laid a hard-edged wedge across the torso that looked like
  a second translucent body; deleting the écorché entirely did not remove it.
  The suns subtend 12° now, so shadows have a penumbra.

Labels come from a ray cast along the skin normal, with nearest-surface as the
fallback where the ray leaves the body — 11,900 of 17,349 by ray. This is the
anatomically meaningful question, though it barely moved the region sizes.

**Region coverage was a resolution problem, not a labelling one.** All four
quadriceps together held 217 of 17,996 vertices and the red described a band
rather than the muscle. Neither the query method nor the registration was at
fault: tallying the anterior mid-thigh specifically showed it holds **176 skin
vertices in total**, of which 90 already *were* quadriceps. The labels were
right and the canvas was too coarse.

`label-body.py` applies the body's subdivision modifier before labelling —
17,996 -> 276,437 vertices — after clearing MB-Lab's facial expression shape
keys, which block applying a modifier and which this pipeline does not use.
Quadriceps go 217 -> 3,357, pectoralis major 32 -> 538, biceps brachii 73 ->
1,047, and the highlight becomes the muscle's shape.

Renders also got *faster*: 21s against 50s, because 276k of real geometry beats
18k subdivided to ~1.1M at render time.

### Detecting a body that passes through itself

Posing has no collision: bones rotate, skin follows, and a limb swung into the
torso simply occupies the same space as it. The `stand` pose shipped with the
hands inside the pelvis and emerging at the crotch, and with the two hands
inside each other — obvious once seen, and invisible to every check we had.

`render/skin/collide.py` intersects the posed skin with itself and attributes
each intersecting triangle to the bone that drives it, then measures how far
apart those two bones are along the skeleton. **The test is not whether surfaces
intersect but which ones do**: a deep squat legitimately presses hamstring
against calf and folds skin at a closed hip, so `squat` has 16,436
self-intersecting triangle pairs and is fine. A hand seven joints from the
pelvis sharing space with it is not. Thresholds: 5 joints apart, 40 triangles.

- `render/skin/check-pose.py` reports every pose, non-zero if any is impossible.
- `render-skin.py` refuses to render one, so an impossible pose cannot reach the
  catalog the way the headless écorché did.

Verified in both directions: the three real poses pass, and a deliberately
folded-arms pose is rejected with `lowerarm_L through lowerarm_R: 431 triangles`
and no image written.

⚠ **`stand` is an A-pose, not arms-at-sides.** Rotating `upperarm` past about
-50° on Z swings the hand *inward* as it descends rather than straight down —
the bone's local axis is not the anatomical one — so arms-at-sides is not
reachable on Z alone, and every angle that approached it collided with the hip.
The écorché stands the same way, so the two styles agree.

⚠ **A pose has a view that suits it.** Arms held forward foreshorten into stubs
from the front; the squat only reads from the side. View is a per-exercise
choice, not a global default.

The one shipped écorché (`heel_toe_rocks`) was rendered under the old exposure
and has not been re-rendered, so it is a shade brighter than anything new.

### Animation (2026-09-03)

`render/skin/animate.py` renders an exercise as a seamless loop: keyframe the
bones through a list of `pose@frame` keys, plant the feet on every frame, frame
the camera on the union of every frame, render with EEVEE to MP4 through
Blender's own FFmpeg. A goblet squat comes out as 24 frames at 12fps, 33 KB.

Three things it has to get right that a still does not:

- **Check every frame, not just the keys.** Two legal poses can be joined by an
  illegal path, and this is not hypothetical: the very first loop attempted was
  rejected at frame 3 with the fingers of both hands inside both thighs, while
  `stand` at frame 0 and `squat` at frame 12 each pass on their own. Fixed by a
  `squat_reach` waypoint that brings the arms forward while the legs are still
  nearly straight, so the hands travel in front of the thighs rather than
  through them.
- **Plant the feet on every frame.** The hips drop through a rep, so one offset
  would leave the figure sinking and rising.
- **Frame on the whole rep.** Framing on whichever pose is current leaves the
  figure drifting in and out of the composition.

`--stills <dir>` writes PNGs of every key and midpoint, because a loop can only
be judged by looking and not everyone reviewing can open a video in place.

**What the app stores (decided by Pippijn, 2026-09-03): both.** The photograph
stays exactly as it is and the loop is added beside it. The sourced pictures
show what to do and took real work to gather, so a loop that comes out badly
must not be able to cost us one.

- `exercise_loops` is its own table (migration 0027), so seeding a loop cannot
  touch `exercise_images`.
- The seeder finds a loop by convention — `data/catalog/loops/<slug>.mp4` — and
  etag-compares like images, so an unchanged loop is not rewritten.
- `GET /api/exercises/{id}/loop`, ETag-cached for a year like the image.
  `hasLoop` on the detail says whether to ask; a 404 is ordinary.
- The sheet plays it muted and inline *below* the hero, `object-fit: contain`
  rather than the hero's `cover`, because the render is framed on the whole rep
  deliberately and cropping would cut the feet off at the bottom of the squat.

`squat_goblet` is the first and so far only loop in the bundle.

**Render cost, measured on the Mac at 768px (2026-09-03).** Marginal cost per
frame within one Blender process, which is what an animation pays:

| engine | first frame | steady |
|---|---|---|
| Cycles, 64 samples | 13.8s | ~21s/frame |
| EEVEE Next, 32 samples | 59.1s (GPU context) | **3.9s/frame** |

EEVEE is visually indistinguishable here — the subject is matte diffuse under
two suns, with no light transport for path tracing to win at — so a 60-frame
loop is ~5 minutes rather than ~21. `COACH_RENDER_ENGINE=EEVEE` selects it.

⚠ **Measure the marginal frame, not the invocation.** Timing one `blender -b`
run per engine says EEVEE is *slower* (63s vs 28s), because a single-frame
invocation is dominated by loading a 54 MB blend and by EEVEE's one-off GPU
setup — costs an animation pays once across all frames.

### Four faults, each of which produced a plausible wrong answer

Worth recording because every one of them looked like a result:

1. **`.j` and `.i` are annotation meshes** — 1,051 of them, 822 zero-thickness
   planes, many well off the figure's axis. `is_label` tested only `.g`. They
   are invisible in a lit render, which is why they went unnoticed for a year,
   but they are ordinary surfaces to a nearest-surface query and they wreck a
   bounding box.
2. **`bound_box` measured geometry that was not the geometry queried** — the
   pre-modifier box of a T-pose whose arms had yet to be brought down. Register
   from the actual posed vertices instead.
3. **MB-Lab parents the body mesh to its armature**, so transforming both
   compounds the scale. The body came out 9% short and sunk into the écorché;
   every lookup then answered with deep tissue, and neck skin resolved to the
   sternothyroid while thigh skin resolved to the iliotibial tract. The
   registration is checked afterwards now, and refuses rather than proceeding.
4. **The debug render used the lit rig**, which washes a saturated colour to
   white. It showed a blank grey body while 12,144 vertices were in fact
   labelled. A segmentation has to be read off an unlit surface.

Faults 1-3 each produced a *contiguous, symmetric, anatomically plausible*
segmentation. What exposed them was checking which muscles were named, not
whether the picture looked like a body: pectoralis major, biceps brachii and
sartorius were absent entirely while a deep neck strap muscle held 2,798
vertices.

## Pipeline shape

New top-level `render/` directory; the shipped artifact stays
`data/catalog/images/<slug>.png`, seeded exactly as today.

Built (2026-07-18):

- `render/fetch-asset.sh` — download + unzip Z-Anatomy into a gitignored
  `render/asset/` (raw asset not committed; idempotent so the CI cache skips it).
- `render/prepare.py` (bpy) — strip the atlas to the muscular system + skeleton
  meshes, purge orphans, save the slim `render/slim.blend` (gitignored,
  reproducible from fetch + prepare).
- `render/muscle_map.json` — committed. Catalog muscle slug → Z-Anatomy mesh
  base names (side/variant suffixes matched automatically). Authored
  incrementally; render.py exits non-zero on a primary/secondary slug with no
  entry rather than rendering an uncoloured lie.
- `render/render.py` (bpy) — load slim blend, read the exercise's muscle roles
  from `data/catalog/exercises.json`, colour via the map (dark red primary, light
  red secondary, neutral the rest), orthographic camera per view, white world +
  sun, render PNG.
- `.github/workflows/render.yml` — `workflow_dispatch` entry point (inputs:
  `exercises`, `view`); installs pinned Blender, runs the three scripts, uploads
  PNGs as an artifact.

Not yet built: `render/poses/<slug>.json` (armature pose per exercise) and
`render/props.py` (procedural dumbbell/barbell/bench). Both are M2+.

**Render host: a GitHub Actions job, never a server we run.** The Mac can't
build Blender (link error, aarch64-darwin, blender 5.1.2), and isis is
production — rendering the full atlas there once exhausted its 16 GB, hard-wedged
the box, and forced an unclean reboot (~47 min downtime, 2026-07-18). The fix is
not to render on our machines at all: a dedicated **`workflow_dispatch`**
workflow (`.github/workflows/render.yml`) runs on an ephemeral `ubuntu-latest`
runner, so a blow-up kills a throwaway VM, not a service. It is manual-only —
never on push — because renders are slow (Cycles) and rare.

Blender is a pinned download in the job (blender.org tarball, cached). The
Z-Anatomy asset is fetched from its GitHub repo and cached. Renders upload as a
build **artifact for review first**; only approved images are committed into
`data/catalog/images/`.

**Still strip to a slim blend first.** `prepare.py` reduces the 7,184-object
atlas to the muscular system (+ skeleton) — that is what keeps even the runner
from thrashing, and makes each render fast. Never point the render at
`Startup.blend`.

The isis path (and its `systemd-run` memory cap) is abandoned; it survives only
in [[reference_isis_render_memory_cap]] as the reason CI is the host.

Determinism: pinned Blender version, fixed seed, fixed light/camera rig,
versioned pose files — re-rendering an unchanged pose yields the same image, so
image diffs mean something, like the back-test.

## The hard part: rig and poses

The atlas meshes stand in anatomical position with no skeleton. Posing needs an
armature with weights, and an écorché of dozens of separate muscle shells
deforms imperfectly under automatic weighting: joint creases, interpenetration.

Plan: fit a standard humanoid armature (Rigify metarig) to the figure, bind with
automatic weights, and fix the worst deformation only where a pose exposes it.
Accepted risk — this is the step that can fail to reach acceptable quality. If
multi-shell weighting proves unusable, fallbacks in order: rigid nearest-bone
binding + corrective smooth; or a single skin mesh with muscle regions painted
as texture (loses per-muscle geometry, keeps catalog-driven colouring). Decide
at M2 with renders in hand, not in the abstract.

Pose authoring is where most of the total effort lives (~136 exercises,
minutes-to-tens-of-minutes each once the rig behaves). Poses are authored
incrementally and reviewed one by one; sourced images stay in place until their
replacement render is approved.

## Milestones

- **M1 — asset + toolchain. DONE + visually validated (2026-07-18).** Direction:
  écorché. The render-images workflow runs green on a GitHub runner (~5 min):
  fetch asset → slim blend (7,184 → 2,033 meshes) → Cycles render. The glute
  bridge render shows a shaded 3D back-view muscular figure with gluteus maximus
  in red (primary) and hamstrings in pink (secondary) — catalog-driven, correct.
  Getting there took fixing five Z-Anatomy-specific gotchas (below). Unposed;
  posing is M2. Starter muscle-map in `render/muscle_map.json`.

### Z-Anatomy render gotchas (all fixed in render.py)

The atlas is authored for interactive study, not rendering. In order of
discovery, each produced a wrong image that *looked* like a different bug:

1. **Muscles ship hidden.** The file opens on the skeleton; muscle layers are
   `hide_render`. A render coloured them but showed only the skeleton. Fix:
   reset `hide_render`/`hide_viewport` on the meshes we want.
2. **Fascia occludes the muscles.** Broad connective sheets (fascia lata,
   investing abdominal fascia, aponeuroses) wrap the body as a smooth envelope
   and hide every muscle behind a featureless silhouette. Fix: skip meshes whose
   name matches fascia/aponeurosis/retinaculum/sheath/membrane.
3. **Material slots are object-linked.** Clearing `mesh.materials` leaves the
   original muscle material rendering. Fix: replace every slot, set
   `slot.link = 'DATA'`, and reset every polygon's `material_index` to 0.
4. **A compositor node-tree + Freestyle bake a sepia "sketch" filter over every
   render** — this dominated all material/lighting changes (identical output
   across edits was the tell). Fix: `scene.use_nodes = False`,
   `scene.render.use_freestyle = False`, clear `view_layer.material_override`.
5. **Label/guide meshes** (the "Muscular system" title card, `.g` markers) float
   in the frame. Fix: skip `.g`, all-caps, and collection-title names.

Lighting: camera-relative suns (not view-relative) so the visible surface is lit
whatever the view — the first attempt lit the far side on a back view and looked
flat.
- **M2 — rig. ATTEMPTED, BLOCKED (2026-07-18).** `render/pose.py` appends the
  Z-Biomechanics 237-bone armature, strips its constraints/drivers to a clean FK
  rig, bakes object transforms, and binds all 789 muscles with automatic weights
  (0 failures). But every render — even the unposed *rest* bind — comes out
  blank: the muscles deform out of frame the moment they are bound. Root cause
  (unresolved): the armature's rest skeleton does not line up with the standing
  muscle geometry, so the bind maps muscles onto a mismatched pose and contorts
  them. Fixed along the way (all real, none sufficient): constraint/driver rig,
  rigid-fallback displacement, object-transform bind mismatch, shared `.l`/`.r`
  mirror-mesh data. What remains is aligning the armature's rest pose to the
  muscles — reverse-engineering the file's T-pose↔anatomical constraint system,
  or hand-aligning bones. That is a real rigging project, the risk flagged up
  front ("the rig is the risk"). `pose.py` + `poses.json` are kept as the
  scaffold. Dev loop used capped isis (systemd-run MemoryMax=6G) for fast
  iteration; final renders stay in CI.

  **Follow-up (2026-07-19) — a rig was made to work, and posing was still
  abandoned.** `render/rigging/` got the écorché to pose at all: an MB-Lab free
  humanoid as a *deformation donor*, KDTree nearest-vertex weight transfer onto
  the muscle shells, arms baked into the rest pose before binding (Pippijn's
  donor-rig idea). Torso/legs deformed and the arm gap was closable. But the
  renders were **not human-possible shapes** — because the écorché is separable
  muscle shells, not continuous skinned tissue, every bent joint tears and
  interpenetrates the shells no matter how good the weights are. Structural, not
  a tuning problem. **Verdict: posing/animation is off the table; the écorché is
  a neutral-pose "muscles worked" illustration only.** The `render/rigging/`
  scripts stay as a record of what was tried and why it doesn't generalise.
- **M3 — unposed écorché shipped to the catalog (2026-07-18).** Full 42-slug
  muscle-map authored (`render/muscle_map.json`) and validated across muscle
  groups/views (push-up→pecs, squat→quads, curl→biceps, row→upper back,
  lat-reach→lats, heel-toe→calves — all correct). The two last image-less
  warmups (kneeling lat reach, heel-toe rocks) took écorché images; **all
  136 exercises have images.** The pipeline can now render any exercise
  unposed.

  **Kneeling lat reach went back to a sourced illustration (2026-07-22)** — a
  GymVisual-set drawing that shows the kneeling position *and* colours the lats.
  The only slug where the two styles competed head to head, and the écorché lost
  on the same ground as below: it says which muscle, not what to do. Worth
  weighing when M5 decides replace vs supplement.

  Remaining choice for a wider rollout: **replace vs supplement** — an
  unposed écorché is a "muscles worked" view, not a how-to demo, so overwriting
  the sourced demo photos for the other 134 would trade movement info for muscle
  info. That is a product decision to make deliberately (a second image field is
  the supplement option); not done unilaterally.
- **M4 — props + a loaded lift.** Dumbbell RDL: two dumbbells in hands, hinge
  pose. Proves prop parenting and equipment-driven selection.
- **M5 — scale.** Batch pose authoring, review loop, progressive replacement of
  sourced images. Full muscle-map authored (every catalog slug).

## Attribution

Renders derive from Z-Anatomy (CC-BY-SA 4.0, github.com/Z-Anatomy), itself based
on BodyParts3D / Anatomography (CC-BY-SA 2.1 JP). The male figure lives in the
`Models-of-human-anatomy` repo; the atlas template in `The-blend`. The app carries this attribution on the same
surface that credits exercise media today; the exact placement is decided when
the first render ships (M3).

## Open questions

- Whether nixpkgs Blender works headless on darwin — M1 verifies; isis is the
  fallback runner.
- Slim-blend size and rebuild time — if prepare is slow, cache the slim blend as
  a build artifact rather than rebuilding per render.
- Whether Z-Anatomy's surface aesthetic (clinical atlas) reads well enough next
  to the current fitness-illustration style — judged at M1/M2 renders.
