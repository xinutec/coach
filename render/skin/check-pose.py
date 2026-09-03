"""Report poses in which the body passes through itself.

    blender -b <labelled.blend> --python check-pose.py -- <pose> [pose ...]

Exits non-zero if any pose is impossible. See collide.py for what counts.
"""
import bpy
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import collide  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
poses = json.loads((Path(__file__).resolve().parent / "poses.json").read_text())
wanted = argv or [k for k in poses if not k.startswith("_")]

body = next(o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("MBlab"))
arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
for o in bpy.data.objects:
    if o.type == "MESH":
        o.hide_viewport = o is not body
body.hide_viewport = False

bad = False
for name in wanted:
    if name not in poses:
        sys.exit(f"no pose {name!r} in poses.json")
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="POSE")
    for pb in arm.pose.bones:
        pb.matrix_basis.identity()
    for bone, xyz in poses[name].items():
        pb = arm.pose.bones[bone]
        pb.rotation_mode = "XYZ"
        pb.rotation_euler = tuple(math.radians(a) for a in xyz)
    bpy.ops.object.mode_set(mode="OBJECT")
    bpy.context.view_layer.update()

    faults, pairs = collide.find_faults(bpy, body, arm)
    if faults:
        bad = True
        print(f"IMPOSSIBLE {name}: {len(faults)} region pair(s) share space")
        print(collide.describe(faults))
    else:
        print(f"ok {name}: nothing passes through anything "
              f"({pairs} self-intersecting triangle pairs, all between neighbours)")

sys.exit(1 if bad else 0)
