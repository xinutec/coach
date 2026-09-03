"""Render one exercise on the skinned body, coloured from the catalog.

The fork-C counterpart of ../render.py. Same catalog, same muscle_map.json, same
light — the only difference is what carries the colour: whole named meshes in
the écorché, per-vertex regions here (see label-body.py for where the regions
come from). An unmapped primary or secondary is a hard error in both, because a
picture that silently under-colours disagrees with the muscle model.

    blender -b <labelled.blend> --python render-skin.py -- <slug> <view> <out.png>
      view: front | back | left | right
"""
import bpy
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import za  # noqa: E402
import stage  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
slug, view, out_png = argv[0], argv[1], argv[2]

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
