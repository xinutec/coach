# Anatomy renders

Generate the exercise illustrations ourselves from a 3D anatomical model instead
of sourcing them one by one: a posed écorché figure per exercise, primary muscles
dark red, secondaries lighter red, consistent style, white background — and the
colouring **derived from the same catalog data the engine uses**, so an image can
never disagree with the muscle model.

Status: approved 2026-07-18, not yet built. Milestones below track progress.

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

- `render/fetch-model.sh` — download the pinned Z-Anatomy release into a
  gitignored `render/model/` (hundreds of MB; the raw asset is not committed).
- `render/prepare.py` (bpy) — strip the atlas to the muscular system + skeleton,
  normalize object naming, save a slim working `.blend` (also gitignored,
  reproducible from fetch + prepare).
- `render/muscle-map.json` — committed. Catalog muscle slug → Z-Anatomy mesh
  names (both sides). One-time authored table; the render fails loudly on an
  unmapped slug rather than rendering an uncoloured lie.
- `render/poses/<exercise-slug>.json` — committed, one per exercise: armature
  bone rotations plus camera framing and prop placement. The pose file is the
  authored source of truth for an illustration; diffs show exactly what changed.
- `render/props.py` — procedural kit meshes (dumbbell, barbell, bench, box,
  rings), parented to hand/body bones, selected by the exercise's equipment.
- `render/render.py` (bpy) — load slim blend, apply pose, colour muscles from
  the catalog (dark red primary, light red secondary, neutral the rest), fixed
  camera + three-point light, render PNG.
- `scripts/render-image.sh <slug>` — entry point; headless Blender
  (`blender -b`), toolchain from nix.

**Render host: isis, not the Mac.** nixpkgs Blender has no aarch64-darwin binary
cache and fails to compile from source locally (link error, blender 5.1.2). On
isis (Linux/NixOS) it is a cache hit. The pipeline runs over SSH on isis; the
slim blend and pose files sync there, PNGs come back into
`data/catalog/images/`. Confirmed 2026-07-18.

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

- **M1 — asset + toolchain.** Headless Blender runs on isis (Mac build failed —
  see Render host). Z-Anatomy fetched, stripped, slim blend builds. Static
  unposed render: grey skin figure with glute-bridge muscles revealed in red via
  a starter muscle-map. Proves: asset naming, skin/muscle compositing, material
  scripting, headless pipeline.
- **M2 — rig.** Armature bound; one simple pose (glute bridge: supine, knees
  bent) rendered and reviewed. Go/no-go on deformation quality; fallback
  decision if needed.
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
