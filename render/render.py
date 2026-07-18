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


M_BASE = material("m_base", (0.80, 0.62, 0.55))  # muted flesh — non-target muscle
M_PRIM = material("m_prim", (0.55, 0.02, 0.02))
M_SEC = material("m_sec", (0.90, 0.32, 0.28))

# Which meshes are skeleton — hidden for the muscle-only écorché. The slim blend
# keeps the "Skeletal system" collection, so membership is still queryable.
skel_names = set()
for c in bpy.data.collections:
    if "keletal" in c.name:
        skel_names |= {o.name for o in c.objects}

def is_label(name: str) -> bool:
    # Z-Anatomy carries text/guide meshes in the anatomy collections (e.g. the
    # "Muscular system" title card, `.g` guide markers). They are not anatomy.
    b = base(name)
    return (
        name.endswith(".g")
        or b in ("Muscular system", "Skeletal system")
        or b.isupper()
    )


# Connective-tissue sheets that wrap the muscles as a smooth outer envelope
# (fascia lata, investing abdominal fascia, aponeuroses, retinacula). They
# occlude the muscles beneath — the "featureless body silhouette, no visible
# muscle" symptom — so they are hidden for the écorché.
_ENVELOPE = ("fascia", "aponeurosis", "retinaculum", "sheath", "membrane")


def is_envelope(name: str) -> bool:
    return any(k in name.lower() for k in _ENVELOPE)


# Z-Anatomy ships most layers hidden (it opens on the skeleton). The muscles were
# coloured but never showed because they stayed hide_render — the M1 bug. Reset
# visibility explicitly, then hide the skeleton so muscle is the subject.
n_prim = n_sec = n_muscle = 0
sizes = []  # (diagonal, name) — to spot any body-envelope mesh that would occlude
for o in bpy.data.objects:
    if o.type != "MESH":
        continue
    if o.name in skel_names or is_label(o.name) or is_envelope(o.name):
        o.hide_render = True
        continue
    o.hide_render = False
    o.hide_viewport = False
    n_muscle += 1
    bb = [o.matrix_world @ mathutils.Vector(c) for c in o.bound_box]
    diag = (max(v.z for v in bb) - min(v.z for v in bb)) + (max(v.x for v in bb) - min(v.x for v in bb))
    sizes.append((diag, o.name))
    b = base(o.name)
    o.data.materials.clear()
    if b in prim_bases:
        o.data.materials.append(M_PRIM)
        n_prim += 1
    elif b in sec_bases:
        o.data.materials.append(M_SEC)
        n_sec += 1
    else:
        o.data.materials.append(M_BASE)
print(f"visible muscle meshes={n_muscle}  coloured primary={n_prim} secondary={n_sec}")
sizes.sort(reverse=True)
print("largest visible meshes:", [n for _, n in sizes[:6]])
if n_muscle == 0:
    sys.exit("no muscle meshes visible — did prepare.py keep the Muscular system?")
if prim_bases and n_prim == 0:
    sys.exit("primary muscles mapped but 0 meshes matched — mesh names drifted?")

# Frame the whole figure with an orthographic camera from the requested side.
mins = mathutils.Vector((1e9,) * 3)
maxs = mathutils.Vector((-1e9,) * 3)
for o in bpy.data.objects:
    if o.type != "MESH" or o.hide_render:
        continue  # frame the visible muscle figure, not the hidden skeleton
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

# Low ambient so directional light — not flat sky — defines the muscle relief.
# The previous flat look came from a strong even world washing out all shading.
world = bpy.data.worlds.new("w")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (1, 1, 1, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 0.25
bpy.context.scene.world = world

# Suns aimed RELATIVE TO THE CAMERA so the surface we see is always lit at an
# oblique angle (which is what reveals muscle relief), whatever the view. Sun
# energy is distance-independent, so this is predictable. The old bug was
# direction: view-flipped angles lit the far side on a back view -> flat.
cam_dir = (center - cam.location).normalized()  # into the scene, away from camera
right = cam_dir.cross(mathutils.Vector((0, 0, 1))).normalized()
up = right.cross(cam_dir).normalized()


def add_sun(name, energy, travel):
    d = bpy.data.lights.new(name, "SUN")
    d.energy = energy
    o = bpy.data.objects.new(name, d)
    bpy.context.scene.collection.objects.link(o)
    o.rotation_euler = travel.normalized().to_track_quat("-Z", "Y").to_euler()
    return o


# Key from upper-left of camera, fill (softer) from lower-right — 'travel' is the
# direction the light moves, so an upper-left source travels down-right-forward.
add_sun("key", 4.0, cam_dir - up * 0.5 + right * 0.5)
add_sun("fill", 1.3, cam_dir + up * 0.3 - right * 0.5)

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
