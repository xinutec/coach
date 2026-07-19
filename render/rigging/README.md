# Écorché rigging (abandoned — kept as a record)

**Outcome (2026-07-19): posing the écorché is off the table.** This borrowed-rig
approach *did* move the figure, but the écorché is dozens of separate muscle
shells, not one skinned mesh, so every bent joint tears and interpenetrates them
into non-human shapes — structural, not fixable by tuning. The écorché is used
only for muscle colouring in the neutral pose (M1/M3, shipped). These scripts are
kept as a record of what was tried and why it doesn't generalise. See M2 in
[../../docs/anatomy-renders.md](../../docs/anatomy-renders.md).

## Idea

The écorché has real per-muscle geometry but no skeleton. **MB-Lab** (free
Blender add-on) generates a rigged, weighted humanoid body. We use that body only
as a **deformation donor**: transfer its bone weights onto the écorché muscles by
nearest vertex, add an Armature modifier, and bone rotations then pose the
muscles. Then the catalog-driven highlighting (`../render.py`) colours the target
muscles.

## Run it (locally on the Mac)

nixpkgs Blender won't build on Apple Silicon; use the official blender.org arm64
build (`hdiutil attach` the .dmg, copy `Blender.app` out). Do NOT run on isis —
it's production and an uncapped render already wedged it once.

```sh
BL=./Blender.app/Contents/MacOS/Blender
# 1. rigged donor body (mblab.zip from github.com/animate1978/MB-Lab tag 1_8_1)
"$BL" -b --python gen-body.py -- /abs/path/mblab.zip body.blend
# 2. slim écorché (see ../prepare.py, from Z-Anatomy Startup.blend)
"$BL" -b <Startup.blend> --python ../prepare.py -- slim.blend
# 3. transfer rig + demo pose + render
"$BL" -b slim.blend --python transfer-rig.py -- body.blend out.png
```

Regenerate `slim.blend` and `body.blend` with the **same** Blender version — a
5.1-saved blend won't open in 4.2.

## What was tried (in `transfer-rig.py`)

- The mechanism works: 666/666 muscles take weights; torso and legs deform;
  alignment ~4cm median muscle→skin distance. Pose conventions are pinned in the
  script header.
- The arms — initially the écorché's hanging arms bound across a ~5cm gap to the
  A-posed donor and stretched. Fixed by posing the donor arms down, snapshotting
  that geometry for the KDTree, and `pose.armature_apply` to bake arms-down as the
  rest *before* the écorché binds (doing it after → double-deform, transfer5's bug).

## Why it was abandoned

Even with the arm fix, bent joints produce **non-human shapes**: the écorché is
separate muscle shells with no shared skin, so a joint bend tears and
interpenetrates them regardless of weight quality. Structural, not tunable.
Posing is off the table — the écorché is used only for neutral-pose muscle
colouring (M1/M3, shipped). See memory `project_coach_anatomy_posing`.
