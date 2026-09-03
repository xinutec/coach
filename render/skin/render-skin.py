"""Render one exercise on the skinned body, coloured from the catalog.

The fork-C counterpart of ../render.py. Same catalog, same muscle_map.json, same
light — the only difference is what carries the colour: whole named meshes in
the écorché, per-vertex regions here (see label-body.py for where the regions
come from). An unmapped primary or secondary is a hard error in both, because a
picture that silently under-colours disagrees with the muscle model.

    blender -b <labelled.blend> --python render-skin.py -- <slug> <view> <out.png> [pose]
      view: front | back | left | right
      pose: a key in poses.json (default "stand")
"""
import bpy
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import za  # noqa: E402
import stage  # noqa: E402
import collide  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
slug, view, out_png = argv[0], argv[1], argv[2]
pose_name = argv[3] if len(argv) > 3 else "stand"

RENDER = Path(__file__).resolve().parent.parent
REPO = RENDER.parent
catalog = json.loads((REPO / "data/catalog/exercises.json").read_text())
muscle_map = json.loads((RENDER / "muscle_map.json").read_text())

ex = next((e for e in catalog if e.get("slug") == slug), None)
if ex is None:
    sys.exit(f"no exercise with slug {slug!r} in the catalog")

prim_bases, sec_bases = set(), set()
for m in ex.get("muscles", []):
    role, mslug = m["role"], m["slug"]
    if role not in ("primary", "secondary"):
        continue  # stabilizer / other -> left neutral
    if mslug not in muscle_map:
        sys.exit(f"muscle_map.json has no entry for {mslug!r} (needed by {slug})")
    (prim_bases if role == "primary" else sec_bases).update(muscle_map[mslug])
print(f"{slug}: primary bases={sorted(prim_bases)} secondary bases={sorted(sec_bases)}")

stage.unstyle(bpy)

poses = json.loads((Path(__file__).resolve().parent / "poses.json").read_text())
if pose_name not in poses:
    sys.exit(f"no pose {pose_name!r} in poses.json — have "
             f"{sorted(k for k in poses if not k.startswith('_'))}")

body = next((o for o in bpy.data.objects
             if o.type == "MESH" and o.name.startswith("MBlab")), None)
if body is None:
    sys.exit("no MB-Lab body in the blend — was it labelled by label-body.py?")

# The blend still holds the écorché the labels were measured against. Only the
# body is the subject here.
for o in bpy.data.objects:
    if o.type == "MESH":
        o.hide_render = o.hide_viewport = o is not body
body.hide_render = body.hide_viewport = False

arm = next((o for o in bpy.data.objects if o.type == "ARMATURE"), None)
if arm is None:
    sys.exit("no armature — the body cannot be posed")
# The floor is wherever the unposed body's feet are: label-body.py registered it
# onto the écorché, so that height is the atlas figure's ground, not an
# arbitrary zero.
FLOOR_Z = min((body.matrix_world @ v.co).z for v in body.data.vertices)
bpy.context.view_layer.objects.active = arm
bpy.ops.object.mode_set(mode="POSE")
for pb in arm.pose.bones:
    pb.matrix_basis.identity()
missing = [b for b in poses[pose_name] if b not in arm.pose.bones]
if missing:
    sys.exit(f"pose {pose_name!r} names bones this rig does not have: {missing}")
for bone, xyz in poses[pose_name].items():
    pb = arm.pose.bones[bone]
    pb.rotation_mode = "XYZ"
    pb.rotation_euler = tuple(math.radians(a) for a in xyz)
bpy.ops.object.mode_set(mode="OBJECT")
bpy.context.view_layer.update()


def lowest(ob):
    ev = ob.evaluated_get(bpy.context.evaluated_depsgraph_get())
    return min((ev.matrix_world @ v.co).z for v in ev.to_mesh().vertices)


# Stand the figure on the floor. Bending the hips and knees moves the feet but
# not the root, so a posed figure hangs in the air at whatever height its rest
# pose left it — visible immediately, and tedious to correct by hand-tuning a
# root offset per pose. Measure the lowest vertex and drop the rig onto the
# floor the rest pose defined.
floor = lowest(body)
arm.location.z += FLOOR_Z - floor
bpy.context.view_layer.update()
print(f"posed {pose_name!r}: {len(poses[pose_name])} bones, "
      f"dropped {(FLOOR_Z - floor) * 100:+.1f}cm onto the floor")

# Refuse a pose the body cannot hold. Posing has no collision, so a limb swung
# into the torso renders as a limb inside the torso — a picture that is wrong
# in a way no downstream step can notice, exactly like the headless écorché.
faults, _pairs = collide.find_faults(bpy, body, arm)
if faults:
    sys.exit(f"pose {pose_name!r} is physically impossible — "
             f"{len(faults)} region pair(s) share space:\n"
             + collide.describe(faults))

# Region name -> colour, resolved once. A vertex in no region (BONE, or too far
# from any anatomy to be named) keeps the neutral flesh.
gcolour = {}
for g in body.vertex_groups:
    if not g.name.startswith(za.GROUP_PREFIX):
        continue  # an MB-Lab bone weight, not a muscle region
    region = g.name[len(za.GROUP_PREFIX):]
    if region in prim_bases:
        gcolour[g.index] = za.RGB_PRIM
    elif region in sec_bases:
        gcolour[g.index] = za.RGB_SEC

mesh = body.data
for a in list(mesh.color_attributes):
    mesh.color_attributes.remove(a)
attr = mesh.color_attributes.new(name="muscle", type="FLOAT_COLOR", domain="POINT")

n_prim = n_sec = 0
for v in mesh.vertices:
    rgb = za.RGB_BASE
    for g in v.groups:
        c = gcolour.get(g.group)
        if c is None:
            continue
        rgb = c
        if c == za.RGB_PRIM:
            n_prim += 1
            break  # primary wins over secondary where regions meet
        n_sec += 1
    attr.data[v.index].color = (*rgb, 1.0)

print(f"coloured {n_prim} primary and {n_sec} secondary vertices "
      f"of {len(mesh.vertices)}")
if prim_bases and n_prim == 0:
    sys.exit("primary muscles mapped but 0 body vertices matched — "
             "did label-body.py name its groups with Z-Anatomy base names?")

# MB-Lab ships a textured skin shader; replace it outright, as the écorché's
# object-linked slots taught us, or the vertex colours never reach the render.
mat = bpy.data.materials.new("m_skin")
mat.use_nodes = True
bsdf = mat.node_tree.nodes["Principled BSDF"]
bsdf.inputs["Roughness"].default_value = 0.6
vcol = mat.node_tree.nodes.new("ShaderNodeVertexColor")
vcol.layer_name = "muscle"
mat.node_tree.links.new(vcol.outputs["Color"], bsdf.inputs["Base Color"])

mesh.materials.clear()
mesh.materials.append(mat)
for slot in body.material_slots:
    slot.link = "DATA"
    slot.material = mat
for poly in mesh.polygons:
    poly.material_index = 0

stage.setup(bpy, view)
stage.render_png(bpy, out_png)
