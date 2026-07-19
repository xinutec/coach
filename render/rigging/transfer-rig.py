"""Rig the Z-Anatomy écorché by transferring weights from an MB-Lab donor body.

ABANDONED (2026-07-19). This is the most complete attempt and it works
MECHANICALLY — 666/666 muscles take weights, the figure poses — but the écorché
is dozens of SEPARATE muscle shells, not one skinned mesh, so any bent joint
tears and interpenetrates the shells into non-human shapes. Structural, not a
tuning problem. Kept as a record; posing is off the table (the écorché is used
only for neutral-pose muscle colouring, M1/M3). See docs/anatomy-renders.md (M2)
and memory project_coach_anatomy_posing.

Method — bake arms-down as the armature REST BEFORE the écorché binds. transfer5
regressed by calling pose.armature_apply AFTER attaching the écorché Armature
modifiers -> double deform. Correct order:
  align -> pose body arms down -> snapshot deformed body (arms-down) for the KDTree
  -> armature_apply (bakes arms-down rest, zeroes pose) -> transfer weights from the
  snapshot -> add écorché modifiers (bind at identity) -> pose legs only.

  Blender -b <slim.blend> --python transfer-rig.py -- <body.blend> <out.png> [view]
view = x (side, default) | y (front). Run locally on the Mac (blender.org arm64
build), NOT isis. A 5.1-saved blend won't open in 4.2 — regenerate with one version.

Pose conventions (MB-Lab rig, figure faces -Y):
  thigh_* +X = hip flexion   calf_* +X = knee flexion
  upperarm_L z=-48 / upperarm_R z=+48 = arms down (to match the écorché)
"""
import bpy, sys, re, math, mathutils
from mathutils.kdtree import KDTree

body_blend = sys.argv[sys.argv.index("--") + 1]
out_png = sys.argv[sys.argv.index("--") + 2]
view = sys.argv[sys.argv.index("--") + 3] if len(sys.argv) - sys.argv.index("--") > 3 else "x"

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


def rot(b, x=0, y=0, z=0):
    pb = barm.pose.bones.get(b)
    if pb:
        pb.rotation_mode = "XYZ"
        pb.rotation_euler = (math.radians(x), math.radians(y), math.radians(z))


# 1) pose body arms down (+ slight forearm) to match the écorché's hanging arms
bpy.context.view_layer.objects.active = barm
bpy.ops.object.mode_set(mode="POSE")
rot("upperarm_L", z=-48)
rot("upperarm_R", z=48)
bpy.ops.object.mode_set(mode="OBJECT")
bpy.context.view_layer.update()

# 2) snapshot the DEFORMED body (arms down) in world space -> spatial reference,
#    paired with each vert's ORIGINAL bone weights (index-aligned to body.data).
#    Disable non-armature modifiers so the evaluated mesh keeps the original count.
for m in body.modifiers:
    if m.type != "ARMATURE":
        m.show_viewport = m.show_render = False
bpy.context.view_layer.update()
deps = bpy.context.evaluated_depsgraph_get()
bev = body.evaluated_get(deps)
bmesh = bev.to_mesh()
snap = [bev.matrix_world @ v.co for v in bmesh.vertices]
assert len(snap) == len(body.data.vertices), f"count mismatch {len(snap)} vs {len(body.data.vertices)}"
gname = {g.index: g.name for g in body.vertex_groups}
bweights = [[(gname[g.group], g.weight) for g in v.groups] for v in body.data.vertices]

# 3) bake arms-down as the armature REST (before any écorché modifier exists),
#    which also zeroes the pose. Legs' rest is untouched (still straight).
bpy.context.view_layer.objects.active = barm
bpy.ops.object.mode_set(mode="POSE")
bpy.ops.pose.armature_apply()
bpy.ops.object.mode_set(mode="OBJECT")

# 4) KDTree on the arms-down snapshot
kd = KDTree(len(snap))
for i, co in enumerate(snap):
    kd.insert(co, i)
kd.balance()

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
print("transferred weights to", len(musc), "muscles; arms baked into rest")

# 5) pose legs seated (arms stay at baked-down rest)
bpy.context.view_layer.objects.active = barm
bpy.ops.object.mode_set(mode="POSE")
rot("thigh_L", x=85)
rot("thigh_R", x=85)
rot("calf_L", x=95)
rot("calf_R", x=95)
bpy.ops.object.mode_set(mode="OBJECT")

# neutralise Z-Anatomy's compositor/Freestyle sketch filter
sc = bpy.context.scene
for vl in sc.view_layers:
    vl.material_override = None
    vl.use_freestyle = False
sc.use_nodes = False
sc.render.use_freestyle = False

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
cd.ortho_scale = max(size) * 1.15
cam = bpy.data.objects.new("c", cd)
bpy.context.scene.collection.objects.link(cam)
axis = mathutils.Vector((1, 0, 0)) if view == "x" else mathutils.Vector((0, -1, 0))
cam.location = center + axis * max(size) * 3
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
