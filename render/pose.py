"""Bind the Z-Anatomy muscles to the Z-Biomechanics armature and apply a pose.

The atlas muscles have no skeleton; the Z-Biomechanics file has a 237-bone
humanoid armature (mixamo-style: Hips, RightUpLeg, Knee.r, Tibia.r, Spine...)
aligned to the same BodyParts3D coordinate space. This appends that armature into
the slim muscle blend, skins the muscles to it with automatic weights (falling
back to rigid nearest-bone parenting for meshes bone-heat can't solve), applies a
named pose, and saves a posed blend for render.py.

    blender -b <slim.blend> --python pose.py -- <biomech.blend> <out.blend> <pose>
"""
import bpy
import json
import math
import sys
from pathlib import Path

import mathutils

argv = sys.argv[sys.argv.index("--") + 1:]
biomech_blend, out_blend, pose_name = argv[0], argv[1], argv[2]

HERE = Path(__file__).resolve().parent


def muscle_meshes():
    skel = set()
    for c in bpy.data.collections:
        if "keletal" in c.name:
            skel |= {o.name for o in c.objects}
    return [o for o in bpy.data.objects if o.type == "MESH" and o.name not in skel]


# --- append the armature from the biomechanics file ---
with bpy.data.libraries.load(biomech_blend, link=False) as (src, dst):
    dst.objects = [n for n in src.objects if n == "Armature"]
arm = next(o for o in dst.objects if o and o.type == "ARMATURE")
bpy.context.scene.collection.objects.link(arm)
print(f"appended armature {arm.name!r} with {len(arm.data.bones)} bones")

# The biomech armature is a constraint/mocap-driven rig (Set T-Pose, Mocap, Copy
# Rotation...) with drivers that break on append and drag the skinned figure out
# of frame. Strip it to a clean FK skeleton at its rest positions — which already
# match the muscle geometry — so plain bone rotations are the only thing moving it.
n_con = 0
for pb in arm.pose.bones:
    for con in list(pb.constraints):
        pb.constraints.remove(con)
        n_con += 1
    pb.location = (0, 0, 0)
    pb.rotation_euler = (0, 0, 0)
    pb.rotation_quaternion = (1, 0, 0, 0)
    pb.scale = (1, 1, 1)
if arm.animation_data:
    arm.animation_data_clear()
if arm.data.animation_data:
    arm.data.animation_data_clear()
print(f"stripped {n_con} pose-bone constraints + drivers -> clean FK rig")

meshes = muscle_meshes()
print(f"binding {len(meshes)} muscle meshes")

# Bake object transforms into geometry before binding. ARMATURE_AUTO binds
# relative to each object's transform; when the muscles and the armature don't
# share the same transform, the rest pose already deforms and the figure flies
# out of frame (confirmed: a rest bind rendered blank). The .l/.r muscles share
# mesh data (mirror duplicates), which blocks the transform_apply operator, so do
# it directly in Python: single-user the data, transform verts by the world
# matrix, reset the object to identity. Then apply the armature's own transform.
from mathutils import Matrix

for o in meshes:
    if o.parent is not None:
        mw = o.matrix_world.copy()
        o.parent = None
        o.matrix_world = mw
    if o.data.users > 1:
        o.data = o.data.copy()
    o.data.transform(o.matrix_world)
    o.matrix_world = Matrix.Identity(4)

bpy.ops.object.select_all(action="DESELECT")
arm.select_set(True)
bpy.context.view_layer.objects.active = arm
with bpy.context.temp_override(selected_editable_objects=[arm], selected_objects=[arm],
                               active_object=arm, object=arm):
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
print("baked object transforms (muscles + armature -> identity) before bind")

# --- bind: automatic weights, per-mesh so one failure can't sink the batch ---
# CRITICAL: never bone-parent a failed mesh — that snaps it to the bone origin
# and flings it out of frame. A mesh that can't be auto-weighted just keeps an
# (empty) Armature modifier: it stays exactly in place and doesn't deform.
bound_auto = bound_static = 0


def ensure_armature_mod(o):
    if not any(m.type == "ARMATURE" for m in o.modifiers):
        m = o.modifiers.new("Armature", "ARMATURE")
        m.object = arm


for o in meshes:
    ok = False
    try:
        with bpy.context.temp_override(active_object=arm, object=arm,
                                       selected_editable_objects=[o, arm], selected_objects=[o, arm]):
            bpy.ops.object.parent_set(type="ARMATURE_AUTO")
        ok = len(o.vertex_groups) > 0
    except RuntimeError:
        ok = False
    if ok:
        bound_auto += 1
    else:
        ensure_armature_mod(o)  # stays in place, undeformed — no displacement
        bound_static += 1
print(f"bound: auto-weights={bound_auto} static(undeformed, in place)={bound_static}")

# --- apply the pose (bone euler rotations, degrees) ---
poses = json.loads((HERE / "poses.json").read_text())
if pose_name not in poses:
    sys.exit(f"no pose {pose_name!r} in poses.json (have: {list(poses)})")
pose = poses[pose_name]
bpy.context.view_layer.objects.active = arm
bpy.ops.object.mode_set(mode="POSE")
applied = 0
for bone_name, xyz in pose.get("bones", {}).items():
    pb = arm.pose.bones.get(bone_name)
    if pb is None:
        print(f"  WARN pose bone {bone_name!r} not found")
        continue
    pb.rotation_mode = "XYZ"
    pb.rotation_euler = tuple(math.radians(a) for a in xyz)
    applied += 1
bpy.ops.object.mode_set(mode="OBJECT")
print(f"applied pose {pose_name!r}: {applied} bone rotations")

bpy.ops.wm.save_as_mainfile(filepath=out_blend, compress=True)
print(f"WROTE posed blend: {out_blend}")
