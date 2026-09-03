"""Colour the labelled body by region so the transfer can be looked at.

Every region gets its own hue from a hash of its name, BONE goes blue and an
unlabelled vertex stays grey. The tally label-body.py prints says how many
vertices a region took; only a picture says whether they are the right ones.

    blender -b <labelled.blend> --python debug-regions.py -- <view> <out.png>
"""
import bpy
import colorsys
import hashlib
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import za  # noqa: E402
import stage  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
view, out_png = argv[0], argv[1]

stage.unstyle(bpy)

body = next(o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("MBlab"))
for o in bpy.data.objects:
    if o.type == "MESH":
        o.hide_render = o.hide_viewport = o is not body
body.hide_render = body.hide_viewport = False


def hue(name):
    if name == "BONE":
        return (0.15, 0.25, 0.85)
    h = int(hashlib.sha256(name.encode()).hexdigest()[:8], 16) / 0xFFFFFFFF
    return colorsys.hsv_to_rgb(h, 0.85, 0.95)


# Only the groups label-body.py wrote; the rest are MB-Lab's bone weights,
# and colouring by those draws a picture of the skeleton's influence map.
gname = {g.index: g.name[len(za.GROUP_PREFIX):]
         for g in body.vertex_groups if g.name.startswith(za.GROUP_PREFIX)}
mesh = body.data
for a in list(mesh.color_attributes):
    mesh.color_attributes.remove(a)
attr = mesh.color_attributes.new(name="muscle", type="FLOAT_COLOR", domain="POINT")
n_named = 0
for v in mesh.vertices:
    named = [gname[g.group] for g in v.groups if g.group in gname]
    if named:
        n_named += 1
    rgb = hue(named[0]) if named else (0.55, 0.55, 0.55)
    attr.data[v.index].color = (*rgb, 1.0)
print(f"DIAG regions={len(gname)} of {len(body.vertex_groups)} groups; "
      f"coloured {n_named}/{len(mesh.vertices)} vertices")

# EMISSION, not the lit shader the real renders use. Under stage.py's suns a
# saturated colour washes out to white and the segmentation is unreadable — the
# first debug pass looked like a blank body while 12,144 vertices were in fact
# labelled. An unlit surface shows the label colour verbatim.
mat = bpy.data.materials.new("m_debug")
mat.use_nodes = True
nt = mat.node_tree
out = nt.nodes["Material Output"]
nt.nodes.remove(nt.nodes["Principled BSDF"])
emit = nt.nodes.new("ShaderNodeEmission")
vcol = nt.nodes.new("ShaderNodeVertexColor")
vcol.layer_name = "muscle"
nt.links.new(vcol.outputs["Color"], emit.inputs["Color"])
nt.links.new(emit.outputs["Emission"], out.inputs["Surface"])
mesh.materials.clear()
mesh.materials.append(mat)
for slot in body.material_slots:
    slot.link = "DATA"
    slot.material = mat
for poly in mesh.polygons:
    poly.material_index = 0

stage.setup(bpy, view)
stage.render_png(bpy, out_png)
