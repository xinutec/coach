# Anatomy renders

Generate the exercise illustrations ourselves from a 3D anatomical model instead
of sourcing them one by one: an écorché figure per exercise, primary muscles dark
red, secondaries lighter red, consistent style — and the colouring **derived from
the same catalog data the engine uses**, so an image can never disagree with the
muscle model.

Status (2026-07-18): **M1 done and working** — the CI pipeline renders a shaded,
catalog-coloured, *unposed* écorché (see M1). **M2 (posing) attempted and
blocked** on rig/muscle alignment (see M2). Current recommendation: ship the
unposed écorché; treat posing as a separate effort.

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
  per-muscle geometric precision and the colouring is only as good as the paint
  map.
- **D — pause.** 134/136 exercises already have sourced images; the marginal
  value is consistency, not coverage. Keep the M1 findings and revisit later.

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
  scaffold. **Recommendation: ship the unposed écorché (M1) and greenlight
  posing separately if it's worth the effort.** Dev loop used capped isis
  (systemd-run MemoryMax=6G) for fast iteration; final renders stay in CI.
- **M3 — pilot end-to-end.** Glute bridge from pose file to
  `data/catalog/images/`, judged against the current sourced image.
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
