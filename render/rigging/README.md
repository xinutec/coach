# Écorché rigging (WIP)

Experiment to pose the Z-Anatomy écorché by borrowing a rig, instead of building
one on the loose muscle shells (which failed — see M2 in
[../../docs/anatomy-renders.md](../../docs/anatomy-renders.md)). This is the
approach that got the écorché to pose at all.

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

## Status

- **Works:** torso and legs deform coherently; the écorché poses (was impossible
  before). Pose conventions are pinned (see the header of `transfer-rig.py`).
  Alignment ~4cm median muscle→skin distance.
- **Open — the arms.** The écorché's arms hang down but MB-Lab is A-posed, so arm
  muscles bind across a ~5cm gap and stretch when posed. Pose-matching the arms is
  the fix; baking arms-down as the armature rest (`pose.armature_apply` *after*
  attaching the écorché modifiers) regressed — it re-deformed the down arms and
  swallowed the leg pose. Needs a cleaner arm-match that leaves the rest pose
  alone. The distance gate is a safety net, not a full fix.
- **Not yet wired:** catalog highlighting on the posed figure, per-exercise pose
  files, camera-per-exercise.

See memory `project_coach_anatomy_posing` for the full method and gotchas.
