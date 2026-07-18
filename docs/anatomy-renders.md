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

## Aesthetic: skinned grey figure, muscles revealed in red

Match the reference stock art, not a medical atlas. Those references are a
neutral grey **skinned** male figure *with a face* — the target muscles rendered
red as if revealed through the skin, everything else left under plain grey skin
so the figure reads as a person.

Z-Anatomy carries this: the BodyParts3D base includes the integumentary system
(skin) and the head/face, not only muscles. The render composites two layers:

- **Skin mesh** — neutral grey, opaque, the default surface (with face). This is
  what makes it look like the references rather than an écorché.
- **Target muscles** — dark red (primary) / light red (secondary), made visible
  through the skin over the highlighted regions (skin cut away or turned
  semi-transparent there; muscle meshes composited on top). Non-target muscles
  stay hidden under grey skin.

This is more compositing than a bare-muscle render (two reconciled layers), and
the skin-vs-muscle reveal is a per-material decision the render script drives
from which muscles the catalog marks primary/secondary. Verified at M1 that the
skin mesh renders cleanly with the highlighted muscles showing through.

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
