"""Label an MB-Lab body's vertices with the muscle lying under each one.

Fork C in docs/anatomy-renders.md needs a single skinned body whose muscle
regions can be recoloured per exercise. Painting those regions by hand would
hand the colouring back to an artist's opinion, which is the one thing this
pipeline exists to avoid — the écorché's whole claim is that the catalog decides
what is red. So the regions are DERIVED instead: the two figures are scaled onto
each other in the same pose, and every body vertex takes the name of the nearest
Z-Anatomy muscle vertex. muscle_map.json then resolves a catalog slug onto those
same names, exactly as it does for the écorché meshes.

Skeleton meshes go into the lookup too, under BONE. Without them a vertex over
the shin or the top of the skull — where there is no muscle at all — would claim
the nearest muscle several centimetres away and paint a shin red.

Output: one vertex group per Z-Anatomy base name (plus BONE), saved to a blend.

    blender -b <slim.blend> --python label-body.py -- <body.blend> <out.blend>

Run on the Mac (blender.org arm64 build), not isis. The alignment and the
arms-down pose come from ../rigging/transfer-rig.py, which established both
against this same asset pair; the transfer runs the other way round here.
"""
import bpy
import math
import sys
from pathlib import Path

import mathutils
from mathutils.bvhtree import BVHTree

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import za  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
body_blend, out_blend = argv[0], argv[1]

BONE = "BONE"
# A body vertex further than this from any anatomy has nothing under it to name.
# 7cm, as in transfer-rig.py: wide enough to cross the skin/muscle gap, narrow
# enough that a hand does not reach the thigh beside it.
MAX_DIST = 0.07
# How far under the skin to look along the normal. Deep enough to cross fat and
# the fascia we exclude, shallow enough that a ray leaving a limb does not land
# on the other one.
RAY_DEPTH = 0.12
# Subdivision level applied to the body before labelling. 2 turns 18k vertices
# into ~288k, which is ~1,600 on the anterior thigh instead of 176.
SUBDIVIDE = 2

with bpy.data.libraries.load(body_blend, link=False) as (src, dst):
    dst.objects = [n for n in src.objects if n.startswith("MBlab")]
for o in dst.objects:
    if o:
        bpy.context.scene.collection.objects.link(o)
body = next(o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("MBlab"))
barm = next(o for o in bpy.data.objects if o.type == "ARMATURE" and o.name.startswith("MBlab"))

# Subdivide the body BEFORE labelling. A muscle region is painted per vertex,
# and the anterior mid-thigh of the base mesh holds 176 vertices in total — so
# all four quadriceps came to 217 of 17,996 and the red described a band rather
# than the muscle. The labels were never wrong; the canvas was too coarse.
# Shape keys go first: a modifier cannot be applied to a mesh that has them, and
# they are MB-Lab's facial expressions, which this pipeline does not use.
bpy.context.view_layer.objects.active = body
if body.data.shape_keys:
    body.shape_key_clear()
sub = next((m for m in body.modifiers if m.type == "SUBSURF"), None)
if sub is None:
    sys.exit("no subdivision modifier on the body — nothing to raise the "
             "resolution of, and per-vertex labels would stay coarse")
sub.levels = SUBDIVIDE
before = len(body.data.vertices)
bpy.ops.object.modifier_apply(modifier=sub.name)
print(f"subdivided the body {before} -> {len(body.data.vertices)} vertices")

skel_names = set()
for c in bpy.data.collections:
    if "keletal" in c.name:
        skel_names |= {o.name for o in c.objects}

anatomy = [
    o for o in bpy.data.objects
    if o.type == "MESH" and not o.name.startswith("MBlab")
    and not za.is_label(o.name) and not za.is_envelope(o.name)
]
muscles = [o for o in anatomy if o.name not in skel_names]
bones = [o for o in anatomy if o.name in skel_names]
print(f"lookup source: {len(muscles)} muscle meshes, {len(bones)} skeleton meshes")
if not muscles:
    sys.exit("no muscle meshes — did prepare.py keep the Muscular system?")
if not bones:
    sys.exit("no skeleton meshes — bare regions would take a distant muscle's name")


def rot(b, x=0, y=0, z=0):
    pb = barm.pose.bones.get(b)
    if pb:
        pb.rotation_mode = "XYZ"
        pb.rotation_euler = (math.radians(x), math.radians(y), math.radians(z))


# The MB-Lab body is generated in a T-pose; the écorché stands with its arms
# hanging. Match it before measuring distances, or every arm vertex lands on the
# ribcage it happens to be nearest to in the T.
bpy.context.view_layer.objects.active = barm
bpy.ops.object.mode_set(mode="POSE")
rot("upperarm_L", z=-48)
rot("upperarm_R", z=48)
bpy.ops.object.mode_set(mode="OBJECT")
bpy.context.view_layer.update()

# Measure against the DEFORMED body, but write groups onto the original vertices
# — the indices are the same, and the arms-down shape is what actually overlaps
# the écorché. Non-armature modifiers off so the evaluated mesh keeps its count.
was_on = {m.name: (m.show_viewport, m.show_render) for m in body.modifiers}
for m in body.modifiers:
    if m.type != "ARMATURE":
        m.show_viewport = m.show_render = False
bpy.context.view_layer.update()
def deformed():
    bpy.context.view_layer.update()
    bev = body.evaluated_get(bpy.context.evaluated_depsgraph_get())
    return [bev.matrix_world @ v.co for v in bev.to_mesh().vertices]


# Register the body onto the écorché from REAL VERTICES of the ARMS-DOWN shape,
# not from bound_box on the T-pose. bound_box is the pre-modifier box of a
# figure whose arms are still out, so it measured geometry that is not the
# geometry queried below: it left the body 9% too short and ~5cm out in depth,
# which sank the skin inside the écorché. Every lookup then answered with deep
# tissue — neck skin resolving to sternothyroid, thigh skin to the iliotibial
# tract — while looking like a clean segmentation.
posed = deformed()
if len(posed) != len(body.data.vertices):
    sys.exit(f"deformed vertex count {len(posed)} != {len(body.data.vertices)}")


def extent(pts):
    mn = mathutils.Vector((min(p[i] for p in pts) for i in range(3)))
    mx = mathutils.Vector((max(p[i] for p in pts) for i in range(3)))
    return mn, mx


everts = [o.matrix_world @ v.co for o in muscles for v in o.data.vertices]
emn, emx = extent(everts)
bmn, bmx = extent(posed)
scale = (emx.z - emn.z) / (bmx.z - bmn.z)
M = (mathutils.Matrix.Translation((emn + emx) / 2)
     @ mathutils.Matrix.Scale(scale, 4)
     @ mathutils.Matrix.Translation(-(bmn + bmx) / 2))
# MB-Lab PARENTS the body mesh to its armature. Transforming both compounds the
# scale — the body came out at 0.9117 of the height it had just been scaled to,
# and the guard below is what caught it. Move the parent and let the child ride.
# (../rigging/transfer-rig.py moves both, and predates this.)
for target in ([barm] if body.parent is barm else [body, barm]):
    target.matrix_world = M @ target.matrix_world
posed = deformed()
amn, amx = extent(posed)
print(f"registered body onto écorché: scale={scale:.4f}")
print(f"  écorché muscles x[{emn.x:.3f},{emx.x:.3f}] "
      f"y[{emn.y:.3f},{emx.y:.3f}] z[{emn.z:.3f},{emx.z:.3f}]")
print(f"  body registered x[{amn.x:.3f},{amx.x:.3f}] "
      f"y[{amn.y:.3f},{amx.y:.3f}] z[{amn.z:.3f},{amx.z:.3f}]")
# A skin that does not enclose the muscles it is being labelled from cannot be
# labelled correctly, and the failure is silent — so refuse it here instead.
if abs((amx.z - amn.z) - (emx.z - emn.z)) > 0.01:
    sys.exit(f"registration did not take: body height {amx.z - amn.z:.3f} "
             f"vs écorché {emx.z - emn.z:.3f}")
depth_off = abs((amn.y + amx.y) / 2 - (emn.y + emx.y) / 2)
if depth_off > 0.02:
    sys.exit(f"registration did not take: {depth_off * 100:.1f}cm out in depth, "
             "so the front of the body is not over the front of the écorché")

# Leave the rest pose alone and drop the arms-down pose now it has been
# measured. Baking it as the new rest leaves the rig inconsistent — bones
# arms-down, mesh still in the T it was authored in — and the body then renders
# in a T-pose whatever its bones say. Labels are per-vertex-index, so they do
# not care which pose the body was measured in; the rig has to stay poseable.
bpy.context.view_layer.objects.active = barm
bpy.ops.object.mode_set(mode="POSE")
for pb in barm.pose.bones:
    pb.matrix_basis.identity()
bpy.ops.object.mode_set(mode="OBJECT")
bpy.context.view_layer.update()

# Nearest SURFACE, not nearest vertex. The atlas models muscles at very
# different resolutions, so a vertex lookup lets a densely-modelled muscle win
# over the coarsely-modelled one that is actually closer — the first run gave
# the ascending trapezius 4,607 skin vertices and latissimus dorsi 156. One BVH
# over every anatomy triangle, with each triangle remembering its muscle.
sources = [(o, za.base(o.name)) for o in muscles] + [(o, BONE) for o in bones]
# TRIANGULATE OURSELVES. FromPolygons(all_triangles=False) triangulates ngons
# internally and find_nearest then returns an index into ITS face list, not into
# the list we passed — so every label past the first quad is shifted. The result
# still renders as tidy contiguous regions, which is why it looked right: the
# body came back with no pectoralis major and no biceps at all, and a deep neck
# strap muscle holding 2,798 vertices. Feeding pure triangles makes the index
# ours again.
verts = []
tris = []
labels = []          # index-aligned to tris
for o, name in sources:
    mw = o.matrix_world
    off = len(verts)
    verts.extend((mw @ v.co)[:] for v in o.data.vertices)
    o.data.calc_loop_triangles()
    for t in o.data.loop_triangles:
        tris.append([i + off for i in t.vertices])
        labels.append(name)
print(f"building BVH over {len(tris)} anatomy triangles ({len(verts)} vertices) ...")
bvh = BVHTree.FromPolygons(verts, tris, all_triangles=True)
del verts, tris

# What lies UNDER this patch of skin, not what lies nearest to it. A nearest
# -surface query happily answers with a muscle beside the point rather than
# beneath it — over the anterior thigh it kept picking the iliotibial tract and
# the femur, and all four quadriceps together held 192 of 17,996 vertices. A ray
# straight down the skin normal asks the question anatomy actually poses, and
# nearest-surface stays as the fallback for where the ray leaves the body.
bev = body.evaluated_get(bpy.context.evaluated_depsgraph_get())
bmesh_eval = bev.to_mesh()
nm = bev.matrix_world.to_3x3().inverted_safe().transposed()
normals = [(nm @ v.normal).normalized() for v in bmesh_eval.vertices]

groups = {}
unlabelled = 0
tally = {}
by_ray = 0
for vi, co in enumerate(posed):
    idx = None
    hit = bvh.ray_cast(co - normals[vi] * 0.001, -normals[vi], RAY_DEPTH)
    if hit[2] is not None:
        idx = hit[2]
        by_ray += 1
    else:
        _, _, idx, dist = bvh.find_nearest(co, MAX_DIST)
    if idx is None:
        unlabelled += 1
        continue
    name = labels[idx]
    g = groups.get(name) or body.vertex_groups.new(name=za.GROUP_PREFIX + name)
    groups[name] = g
    g.add([vi], 1.0, "REPLACE")
    tally[name] = tally.get(name, 0) + 1

muscle_verts = sum(n for k, n in tally.items() if k != BONE)
print(f"  {by_ray} labelled by ray, {len(posed) - unlabelled - by_ray} by nearest surface")
print(f"labelled {len(posed) - unlabelled}/{len(posed)} body vertices "
      f"across {len(tally)} regions ({muscle_verts} muscle, "
      f"{tally.get(BONE, 0)} bone, {unlabelled} unlabelled)")
print("largest regions:", sorted(tally.items(), key=lambda kv: -kv[1])[:8])
if muscle_verts == 0:
    sys.exit("every body vertex resolved to bone — is the alignment inverted?")

# Put the deformation stack back before saving. It is switched off above only
# so the evaluated mesh keeps its vertex count while distances are measured —
# leaving it off ships a raw 18k cage with no subdivision and no corrective
# smoothing, which is what "the flesh does not move like flesh" looks like.
for m in body.modifiers:
    if m.name in was_on:
        m.show_viewport, m.show_render = was_on[m.name]
# Dual-quaternion skinning. Linear blend collapses a bent hip or knee inward and
# twists a forearm into a candy wrapper; both are joints every exercise uses.
for m in body.modifiers:
    if m.type == "ARMATURE":
        m.use_deform_preserve_volume = True
print("deformation stack restored:",
      [(m.type, m.show_render) for m in body.modifiers])

bpy.ops.wm.save_as_mainfile(filepath=out_blend, compress=True)
print(f"WROTE {out_blend}")
