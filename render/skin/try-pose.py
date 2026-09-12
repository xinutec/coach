"""Try candidate poses: measure what they rest on, and look at them on a floor.

The authoring loop for a floor pose, in one place, because doing it by hand cost
a wasted render sweep three times over.

    blender -b <labelled.blend> --python try-pose.py -- <candidates.json> \\
        [--render <dir>] [--view left]

`candidates.json` is `{name: {bone: [x,y,z], ..., "_floor": [bone, ...]}}` — the
same shape as an entry in poses.json plus the contacts it rests on. Nothing is
written to poses.json; this is the step BEFORE that.

Two rules it exists to enforce, both learned the expensive way:

⚠ **MEASURE THE RIG, DO NOT REASON ABOUT IT.** The euler conventions here are
borrowed and not guessable: three sign guesses in a row were wrong, each costing
a six-render sweep, and eight measured evaluations then settled every one. Run a
grid through here and read the numbers; it does not render unless asked.

⚠ **AND THEN LOOK, AGAINST A FLOOR.** Every geometric check can pass while the
pose is not the movement. A cat and a cow that both planted perfectly rendered
almost identically — nothing measurable says a demo fails to demonstrate. The
floor slab matters too: these views are orthographic and horizontal, so a plane
draws edge-on as a one-pixel line and a figure hovering 15cm up looks planted.
"""
import bpy
import json
import math
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import plant  # noqa: E402
import stage  # noqa: E402
import collide  # noqa: E402
import floor as floormod  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
positional = [a for a in argv if not a.startswith("--")]
if not positional:
    sys.exit("usage: <candidates.json> [--render <dir>] [--view left]")
candidates = json.loads(Path(positional[0]).read_text())


def opt(name, default=None):
    return argv[argv.index(name) + 1] if name in argv else default


out_dir = opt("--render")
view = opt("--view", "left")

body = next(o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("MBlab"))
arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
for o in bpy.data.objects:
    if o.type == "MESH":
        o.hide_render = o.hide_viewport = o is not body
body.hide_render = body.hide_viewport = False
FLOOR_Z = min((body.matrix_world @ v.co).z for v in body.data.vertices)

if out_dir:
    stage.unstyle(bpy)

bad = 0
for name, spec in candidates.items():
    contacts = tuple(spec.get("_floor", ()))
    arm.location.z = 0.0
    arm.rotation_euler = (0, 0, 0)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="POSE")
    for pb in arm.pose.bones:
        pb.matrix_basis.identity()
    for bone, xyz in spec.items():
        if bone.startswith("_"):
            continue
        if bone not in arm.pose.bones:
            sys.exit(f"{name}: this rig has no bone {bone!r}")
        pb = arm.pose.bones[bone]
        pb.rotation_mode = "XYZ"
        pb.rotation_euler = tuple(math.radians(a) for a in xyz)
    bpy.ops.object.mode_set(mode="OBJECT")
    bpy.context.view_layer.update()

    if contacts:
        tilt, before, spread = plant.solve_tilt(bpy, body, arm, contacts)
        plant.drop(bpy, body, arm, FLOOR_Z, contacts)
        sink, who = plant.sunk(bpy, body, FLOOR_Z)
        verdict = plant.fault(name, tilt, spread, sink, who)
    else:
        tilt = before = spread = sink = 0.0
        who, verdict = "", None
    faults, _pairs = collide.find_faults(bpy, body, arm, allow=set())
    line = (f"[{name}] tilt {tilt:+6.2f}  spread {before * 100:6.1f}->"
            f"{spread * 100:5.2f}cm  through {sink * 100:5.2f}cm({who})  "
            f"collide {len(faults)}")
    if verdict:
        bad += 1
        print(line + f"\n    REFUSED {verdict}")
    elif faults:
        bad += 1
        print(line + "\n    REFUSED passes through itself:\n"
              + collide.describe(faults))
    else:
        print(line + "  OK")

    if out_dir:
        # Bounds from the BODY, before the slab: a 12m floor in visible_bounds
        # zooms the camera out until the figure is a speck.
        bounds = stage.visible_bounds(bpy)
        fl = floormod.add(bpy, FLOOR_Z) if contacts else None
        stage.setup(bpy, view, bounds=bounds)
        stage.render_png(bpy, f"{out_dir}/{name}.png")
        if fl:
            bpy.data.objects.remove(fl, do_unlink=True)

sys.stdout.flush()
sys.exit(1 if bad else 0)
