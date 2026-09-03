"""Render one exercise's écorché illustration from the slim blend.

Colouring is driven by the catalog, not by hand: it reads the exercise's muscle
roles from data/catalog/exercises.json, maps each slug to Z-Anatomy meshes via
muscle_map.json, and paints primaries dark red, secondaries light red, the rest
neutral grey. A primary/secondary slug with no mapping is a hard error — the
picture must never disagree with the muscle model by silently under-colouring.

The skeleton is hidden except for the head: with it fully gone the head is a
hollow shell of facial muscles showing the empty cranial cavity, so the skull is
kept (painted neutral flesh) to give the figure a proper solid head.

    blender -b <slim.blend> --python render.py -- <slug> <view> <out.png>
      view: front | back | left | right
"""
import bpy
import json
import sys
from pathlib import Path

import mathutils

sys.path.insert(0, str(Path(__file__).resolve().parent))
import za  # noqa: E402
import stage  # noqa: E402

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

# Z-Anatomy bakes a sepia "sketch" filter over every render; strip it.
stage.unstyle(bpy)


base = za.base


def material(name, rgb):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (*rgb, 1)
    bsdf.inputs["Roughness"].default_value = 0.6
    return m


M_BASE = material("m_base", za.RGB_BASE)  # muted flesh — non-target muscle
M_PRIM = material("m_prim", za.RGB_PRIM)
M_SEC = material("m_sec", za.RGB_SEC)


def paint(o, mat):
    # Force our material to win. Z-Anatomy's slots are object-linked, so clearing
    # mesh.materials alone leaves the original muscle material rendering. Replace
    # every slot (data + object link) and point every face at slot 0.
    o.data.materials.clear()
    o.data.materials.append(mat)
    for slot in o.material_slots:
        slot.link = "DATA"
        slot.material = mat
    for poly in o.data.polygons:
        poly.material_index = 0

# Which meshes are skeleton — hidden for the muscle-only écorché. The slim blend
# keeps the "Skeletal system" collection, so membership is still queryable.
skel_names = set()
for c in bpy.data.collections:
    if "keletal" in c.name:
        skel_names |= {o.name for o in c.objects}

is_label = za.is_label
is_envelope = za.is_envelope


# Z-Anatomy ships most layers hidden (it opens on the skeleton). The muscles were
# coloured but never showed because they stayed hide_render — the M1 bug. Reset
# visibility explicitly, then hide the skeleton so muscle is the subject. Pass 1
# also measures the muscle figure's vertical extent, used to locate the head.
n_prim = n_sec = n_muscle = 0
sizes = []  # (diagonal, name) — to spot any body-envelope mesh that would occlude
mus_min_z, mus_max_z = 1e9, -1e9
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
    mus_min_z = min(mus_min_z, min(v.z for v in bb))
    mus_max_z = max(mus_max_z, max(v.z for v in bb))
    diag = (max(v.z for v in bb) - min(v.z for v in bb)) + (max(v.x for v in bb) - min(v.x for v in bb))
    sizes.append((diag, o.name))
    b = base(o.name)
    if b in prim_bases:
        paint(o, M_PRIM)
        n_prim += 1
    elif b in sec_bases:
        paint(o, M_SEC)
        n_sec += 1
    else:
        paint(o, M_BASE)

# Give the figure a proper (solid) head. With the whole skeleton hidden the head
# is only a shell of thin facial muscles, so the camera looks straight into the
# empty cranial cavity — an ugly hollow head. Reveal just the head-region
# skeleton (cranium, mandible, teeth) as the solid volume that closes it off,
# painted the same neutral flesh as non-target muscle so it reads as a plain
# head, not a white skeleton. The rest of the skeleton stays hidden — this is
# still a muscle écorché. "Head region" = the top 16% of the figure's height,
# which cleanly separates the skull/jaw from the shoulders below.
neck_z = mus_max_z - 0.16 * (mus_max_z - mus_min_z)
n_head = 0
for o in bpy.data.objects:
    if o.type != "MESH" or o.name not in skel_names or is_label(o.name):
        continue
    bb = [o.matrix_world @ mathutils.Vector(c) for c in o.bound_box]
    center_z = sum(v.z for v in bb) / len(bb)
    if center_z <= neck_z:
        continue  # below the head — keep it hidden
    o.hide_render = False
    o.hide_viewport = False
    paint(o, M_BASE)
    n_head += 1

print(f"visible muscle meshes={n_muscle}  coloured primary={n_prim} secondary={n_sec}  head-fill bones={n_head}")
sizes.sort(reverse=True)
print("largest visible meshes:", [n for _, n in sizes[:6]])
if n_muscle == 0:
    sys.exit("no muscle meshes visible — did prepare.py keep the Muscular system?")
if prim_bases and n_prim == 0:
    sys.exit("primary muscles mapped but 0 meshes matched — mesh names drifted?")
if n_head == 0:
    # Loudly, like its neighbours. This was a warning once, and the headless
    # render it let through was committed to the catalog and served for weeks —
    # the muscles were right, so nothing downstream had a reason to look.
    sys.exit("no head-region skeleton found — the head would render hollow. "
             "Did prepare.py keep the Skeletal system?")

stage.setup(bpy, view)
stage.render_png(bpy, out_png)
