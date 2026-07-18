"""Rig the Z-Anatomy écorché by transferring weights from the MB-Lab body.

The écorché has real per-muscle geometry but no skeleton. This borrows a working
rig: for each écorché muscle vertex, copy the bone weights of the nearest body
vertex, then add an Armature modifier so bone rotations deform the muscles. This
is the technique that got the écorché to POSE at all (automatic weighting on the
loose muscle shells had failed — see M2 in docs/anatomy-renders.md). Pippijn's
idea: a rigged body as a deformation donor.

    Blender -b <slim.blend> --python transfer-rig.py -- <body.blend> <out.png>

slim.blend = the écorché (render/prepare.py). body.blend = gen-body.py output.
Run BOTH with the same Blender version (a 5.1 blend won't open in 4.2). Runs
locally on the Mac (official blender.org arm64 build) — NOT on isis.

STATUS (2026-07-18): core works — torso and legs deform coherently and the pose
conventions below are correct. OPEN PROBLEM: the arms. The écorché's arms hang
down but MB-Lab is A-posed, so arm muscles bind across a ~5cm gap and stretch
when posed. Pose-matching the arms is the fix; baking arms-down as the armature
rest (`pose.armature_apply` after attaching the écorché modifiers) regressed —
it re-deformed the already-down arms and swallowed the leg pose. Needs a cleaner
arm-match that doesn't disturb the rest pose. The distance gate below is a
safety net (far/bad-correspondence verts get no weight, stay put) but does not
fully fix the arms.

Pose conventions (MB-Lab rig, figure faces -Y):
  thigh_* +X = hip flexion (leg forward)   calf_* +X = knee flexion (shin back)
  upperarm_L z=-48 / upperarm_R z=+48 = arms down (to match the écorché)
Alignment: scale body to écorché height + match CLEAN-muscle bbox centres
(exclude fascia/labels first, or outliers wreck it) -> ~4cm median muscle->skin.
"""
import bpy
import math
import re
import sys

import mathutils
from mathutils.kdtree import KDTree

body_blend, out_png = sys.argv[-2], sys.argv[-1]

with bpy.data.libraries.load(body_blend, link=False) as (src, dst):
    dst.objects = [n for n in src.objects if n.startswith("MBlab")]
for o in dst.objects:
    if o:
        bpy.context.scene.collection.objects.link(o)
body = next(o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("MBlab"))
barm = next(o for o in bpy.data.objects if o.type == "ARMATURE" and o.name.startswith("MBlab"))

skel = set()
for c in bpy.data.collections:
    if "keletal" in c.name:
        skel |= {o.name for o in c.objects}


def stem(n):
    return re.sub(r"\.(o?l|o?r|e?l|e?r|j|t|i|s|g)$", "", n).strip()


ENV = ("fascia", "aponeurosis", "retinaculum", "sheath", "membrane")
DISTAL = ("interosse", "lumbric", "hallucis", "of foot", "of hand", "of finger",
          "of thumb", "phalan", "opponens", "pollicis", "thenar", "palmar",
          "dorsal expansion", "of little finger")


def bad(n):
    low = n.lower()
    b = stem(n)
    if "tensor fasciae latae" in low:
        return False
    if n.endswith(".g") or b in ("Muscular system", "Skeletal system") or b.isupper():
        return True
    return any(k in low for k in ENV)


def distal(n):
    return any(k in n.lower() for k in DISTAL)


musc = [o for o in bpy.data.objects if o.type == "MESH" and o.name not in skel
        and not o.name.startswith("MBlab") and not bad(o.name)]
show = [o for o in musc if not distal(o.name)]


def bbox(objs):
    mn = mathutils.Vector((1e9,) * 3)
    mx = mathutils.Vector((-1e9,) * 3)
    for o in objs:
        for c in o.bound_box:
            w = o.matrix_world @ mathutils.Vector(c)
            mn = mathutils.Vector(map(min, mn, w))
            mx = mathutils.Vector(map(max, mx, w))
    return mn, mx


emn, emx = bbox(show)
bmn, bmx = bbox([body])
ec = (emn + emx) / 2
bc = (bmn + bmx) / 2
s = (emx.z - emn.z) / (bmx.z - bmn.z)
M = mathutils.Matrix.Translation(ec) @ mathutils.Matrix.Scale(s, 4) @ mathutils.Matrix.Translation(-bc)
body.matrix_world = M @ body.matrix_world
barm.matrix_world = M @ barm.matrix_world
bpy.context.view_layer.update()

# nearest-vertex weight transfer (bpy.ops.object.data_transfer produced 0 weights
# headless — do it by hand with a KDTree). Distance gate: skip verts with no good
# body correspondence so they stay put instead of exploding.
bmw = body.matrix_world
bverts = body.data.vertices
kd = KDTree(len(bverts))
for i, v in enumerate(bverts):
    kd.insert(bmw @ v.co, i)
kd.balance()
gname = {g.index: g.name for g in body.vertex_groups}
bweights = [[(gname[g.group], g.weight) for g in v.groups] for v in bverts]

for o in musc:
    if o.data.users > 1:
        o.data = o.data.copy()          # .l/.r share data; single-user before painting
for o in musc:
    mw = o.matrix_world
    grp = {}
    for vi, v in enumerate(o.data.vertices):
        _, idx, dist = kd.find(mw @ v.co)
        if dist is None or dist > 0.07:
            continue
        for gn, wt in bweights[idx]:
            g = grp.get(gn) or o.vertex_groups.get(gn) or o.vertex_groups.new(name=gn)
            grp[gn] = g
            g.add([vi], wt, "REPLACE")
    o.modifiers.new("Armature", "ARMATURE").object = barm
    cs = o.modifiers.new("Smooth", "CORRECTIVE_SMOOTH")
    cs.factor, cs.iterations, cs.use_only_smooth = 0.7, 10, True
print("transferred weights to", len(musc), "muscles")

# neutralise Z-Anatomy's compositor/Freestyle sketch filter (see render.py)
sc = bpy.context.scene
for vl in sc.view_layers:
    vl.material_override = None
    vl.use_freestyle = False
sc.use_nodes = False
sc.render.use_freestyle = False

# demo pose: seated (hip + knee flexion, both POSITIVE per the conventions above)
bpy.context.view_layer.objects.active = barm
bpy.ops.object.mode_set(mode="POSE")


def rot(b, x=0, y=0, z=0):
    pb = barm.pose.bones.get(b)
    if pb:
        pb.rotation_mode = "XYZ"
        pb.rotation_euler = (math.radians(x), math.radians(y), math.radians(z))


rot("thigh_L", x=85)
rot("thigh_R", x=85)
rot("calf_L", x=95)
rot("calf_R", x=95)
bpy.ops.object.mode_set(mode="OBJECT")

mg = bpy.data.materials.new("mg")
mg.use_nodes = True
mg.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.82, 0.64, 0.57, 1)
for o in musc:
    o.hide_render = distal(o.name)
    o.data.materials.clear()
    o.data.materials.append(mg)
    for sl in o.material_slots:
        sl.link = "DATA"
        sl.material = mg
for o in bpy.data.objects:
    if o.name in skel or o.name.startswith("MBlab"):
        o.hide_render = True

deps = bpy.context.evaluated_depsgraph_get()
mn = mathutils.Vector((1e9,) * 3)
mx = mathutils.Vector((-1e9,) * 3)
for o in show:
    ev = o.evaluated_get(deps)
    me = ev.to_mesh()
    for v in me.vertices:
        w = ev.matrix_world @ v.co
        mn = mathutils.Vector(map(min, mn, w))
        mx = mathutils.Vector(map(max, mx, w))
center = (mn + mx) / 2
size = mx - mn
cd = bpy.data.cameras.new("c")
cd.type = "ORTHO"
cd.ortho_scale = max(size.x, size.z) * 1.15
cam = bpy.data.objects.new("c", cd)
bpy.context.scene.collection.objects.link(cam)
cam.location = center + mathutils.Vector((1, 0, 0)) * max(size) * 3
cam.rotation_euler = (center - cam.location).normalized().to_track_quat("-Z", "Y").to_euler()
bpy.context.scene.camera = cam
world = bpy.data.worlds.new("w")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[1].default_value = 0.5
bpy.context.scene.world = world
for e, el, az in [(4, 50, 30), (1.3, 30, -60)]:
    d = bpy.data.lights.new("s", "SUN")
    d.energy = e
    su = bpy.data.objects.new("s", d)
    bpy.context.scene.collection.objects.link(su)
    su.rotation_euler = (math.radians(el), 0, math.radians(az))
sc.render.engine = "CYCLES"
sc.cycles.samples = 44
sc.render.resolution_x = 640
sc.render.resolution_y = 640
sc.render.filepath = out_png
bpy.ops.render.render(write_still=True)
print("WROTE", out_png)
