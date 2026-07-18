"""Render one exercise's écorché illustration from the slim blend.

Colouring is driven by the catalog, not by hand: it reads the exercise's muscle
roles from data/catalog/exercises.json, maps each slug to Z-Anatomy meshes via
muscle_map.json, and paints primaries dark red, secondaries light red, the rest
neutral grey. A primary/secondary slug with no mapping is a hard error — the
picture must never disagree with the muscle model by silently under-colouring.

    blender -b <slim.blend> --python render.py -- <slug> <view> <out.png>
      view: front | back | left | right
"""
import bpy
import json
import math
import re
import sys
from pathlib import Path

import mathutils

argv = sys.argv[sys.argv.index("--") + 1:]
slug, view, out_png = argv[0], argv[1], argv[2]

REPO = Path(__file__).resolve().parent.parent
catalog = json.loads((REPO / "data/catalog/exercises.json").read_text())
muscle_map = json.loads((Path(__file__).resolve().parent / "muscle_map.json").read_text())

ex = next((e for e in catalog if e.get("slug") == slug), None)
if ex is None:
    sys.exit(f"no exercise with slug {slug!r} in the catalog")

# Resolve catalog muscle slugs -> sets of Z-Anatomy base mesh names, by role.
prim_bases, sec_bases = set(), set()
for m in ex.get("muscles", []):
    role, mslug = m["role"], m["slug"]
    if role not in ("primary", "secondary"):
        continue  # stabilizer / other -> left neutral
    if mslug not in muscle_map:
        sys.exit(f"muscle_map.json has no entry for {mslug!r} (needed by {slug})")
    (prim_bases if role == "primary" else sec_bases).update(muscle_map[mslug])

print(f"{slug}: primary bases={sorted(prim_bases)} secondary bases={sorted(sec_bases)}")


def base(name: str) -> str:
    # strip side/variant/label suffixes: .l .r .ol .or .el .er .j .t .i .s .g
    return re.sub(r"\.(o?l|o?r|e?l|e?r|j|t|i|s|g)$", "", name).strip()


def material(name, rgb):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (*rgb, 1)
    bsdf.inputs["Roughness"].default_value = 0.6
    return m


M_GREY = material("m_grey", (0.78, 0.75, 0.72))
M_PRIM = material("m_prim", (0.55, 0.02, 0.02))
M_SEC = material("m_sec", (0.90, 0.32, 0.28))

n_prim = n_sec = 0
for o in bpy.data.objects:
    if o.type != "MESH":
        continue
    b = base(o.name)
    o.data.materials.clear()
    if b in prim_bases:
        o.data.materials.append(M_PRIM)
        n_prim += 1
    elif b in sec_bases:
        o.data.materials.append(M_SEC)
        n_sec += 1
    else:
        o.data.materials.append(M_GREY)
print(f"coloured meshes: primary={n_prim} secondary={n_sec}")
if prim_bases and n_prim == 0:
    sys.exit(f"primary muscles mapped but 0 meshes matched — mesh names drifted?")

# Frame the whole figure with an orthographic camera from the requested side.
mins = mathutils.Vector((1e9,) * 3)
maxs = mathutils.Vector((-1e9,) * 3)
for o in bpy.data.objects:
    if o.type != "MESH":
        continue
    for c in o.bound_box:
        w = o.matrix_world @ mathutils.Vector(c)
        mins = mathutils.Vector(map(min, mins, w))
        maxs = mathutils.Vector(map(max, maxs, w))
center = (mins + maxs) / 2
size = maxs - mins

cam_data = bpy.data.cameras.new("cam")
cam_data.type = "ORTHO"
cam_data.ortho_scale = max(size.x, size.z) * 1.15
cam = bpy.data.objects.new("cam", cam_data)
bpy.context.scene.collection.objects.link(cam)
dirs = {"front": (0, -1, 0), "back": (0, 1, 0), "left": (-1, 0, 0), "right": (1, 0, 0)}
d = mathutils.Vector(dirs[view])
cam.location = center + d * max(size) * 3
cam.rotation_euler = (center - cam.location).normalized().to_track_quat("-Z", "Y").to_euler()
bpy.context.scene.camera = cam

world = bpy.data.worlds.new("w")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (1, 1, 1, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 1.0
bpy.context.scene.world = world
sun_data = bpy.data.lights.new("sun", "SUN")
sun_data.energy = 3
sun = bpy.data.objects.new("sun", sun_data)
bpy.context.scene.collection.objects.link(sun)
sun.rotation_euler = (math.radians(55), 0, math.radians(30 + (180 if view == "back" else 0)))

scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.samples = 64
scene.cycles.use_denoising = True
scene.cycles.device = "CPU"
scene.render.resolution_x = 768
scene.render.resolution_y = 768
scene.render.image_settings.file_format = "PNG"
scene.render.filepath = out_png
bpy.ops.render.render(write_still=True)
print(f"WROTE {out_png}")
