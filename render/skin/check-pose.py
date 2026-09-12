"""Report poses the body cannot hold.

Two different impossibilities, checked together because a pose has to survive
both and finding out about them one at a time doubles the render round trips:

- the body passing through ITSELF — see collide.py for what counts;
- a floor pose not resting where it says it does — see plant.py. Only poses that
  declare `_floor` contacts are checked that way; every other pose is untouched.

    blender -b <labelled.blend> --python check-pose.py -- <pose> [pose ...]

Exits non-zero if any pose is impossible.
"""
import bpy
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import collide  # noqa: E402
import plant  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
poses = json.loads((Path(__file__).resolve().parent / "poses.json").read_text())
wanted = argv or [k for k in poses if not k.startswith("_")]

body = next(o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("MBlab"))
arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
for o in bpy.data.objects:
    if o.type == "MESH":
        o.hide_viewport = o is not body
body.hide_viewport = False

# A contact naming a bone the rig does not have would otherwise read as a contact
# that is simply never found, which looks like a pose that rests on nothing.
bad = plant.check_contacts(poses, body)
if bad:
    sys.exit("_floor names bones this rig does not have: "
             + "; ".join(f"{k}: {v}" for k, v in bad.items()))

# The floor is where the unposed body's feet are — the atlas figure's ground,
# not an arbitrary zero.
FLOOR_Z = min((body.matrix_world @ v.co).z for v in body.data.vertices)

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

    floor_bones = plant.contacts(poses, name)
    if floor_bones:
        arm.location.z = 0.0
        tilt, _before, spread = plant.solve_tilt(bpy, body, arm, floor_bones)
        plant.drop(bpy, body, arm, FLOOR_Z, floor_bones)
        sink, who = plant.sunk(bpy, body, FLOOR_Z)
        verdict = plant.fault(name, tilt, spread, sink, who)
        if verdict:
            bad = True
            print(f"UNPLANTED {verdict}")
        else:
            print(f"ok {name}: rests on {', '.join(floor_bones)} "
                  f"(levelled with {tilt:+.2f} deg, contacts within "
                  f"{spread * 1000:.1f}mm)")

    faults, pairs = collide.find_faults(
        bpy, body, arm, allow=collide.allowed_pairs(poses, [name], arm)
    )
    if faults:
        bad = True
        print(f"IMPOSSIBLE {name}: {len(faults)} region pair(s) share space")
        print(collide.describe(faults))
    else:
        print(f"ok {name}: nothing passes through anything "
              f"({pairs} self-intersecting triangle pairs, all between neighbours)")

sys.exit(1 if bad else 0)
