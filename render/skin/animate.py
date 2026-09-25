"""Render one exercise as a seamless loop on the skinned body.

A rep is two poses and the way between them. The catalog still decides the
colour, exactly as for a still, and the loop starts and ends on the same pose so
it can sit in the app's 16:9 hero without a visible cut.

Every frame is checked for the body passing through itself, not just the poses
at each end: two legal poses can be joined by an illegal path, and an
interpolated frame is exactly as capable of putting a hand inside a thigh as an
authored one. The check costs more than the render does; --no-check skips it
while iterating on timing, never for anything that ships.

    blender -b <labelled.blend> --python animate.py -- \\
        <slug> <view> <out.mp4> <pose@frame,...> [--no-check]
    blender -b <labelled.blend> --python animate.py -- <slug> <out.mp4>

e.g.  squat_goblet left out/squat.mp4 stand@0,squat@12,stand@24
      glute_bridge out/bridge.mp4          (view and keys from loops.json)

The short form is how a shipped loop is RE-MADE; the long form is how a new one
is found. Whatever the long form is finally happy with belongs in loops.json, or
the loop cannot be reproduced.
"""
import bpy
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import za  # noqa: E402  the sys.path line above puts the siblings on the path
import stage  # noqa: E402  the sys.path line above puts the siblings on the path
import collide  # noqa: E402  the sys.path line above puts the siblings on the path
import plant  # noqa: E402  the sys.path line above puts the siblings on the path
import floor as floormod  # noqa: E402  the sys.path line above puts the siblings on the path

argv = sys.argv[sys.argv.index("--") + 1:]
positional = [a for a in argv if not a.startswith("--")]
do_check = "--no-check" not in argv

HERE = Path(__file__).resolve().parent
if len(positional) >= 4:
    slug, view, out_path, spec = positional[0], positional[1], positional[2], positional[3]
elif len(positional) == 2:
    # Re-making a recorded loop: the manifest is the only place its view and keys
    # exist, so a missing entry is an error rather than a default.
    slug, out_path = positional
    recorded = json.loads((HERE / "loops.json").read_text())
    if slug not in recorded:
        sys.exit(f"loops.json has no entry for {slug!r} — it is one of the loops "
                 "whose spec was never recorded. Render it with the long form "
                 "until it passes, then add what worked.")
    view, spec = recorded[slug]["view"], recorded[slug]["spec"]
    print(f"{slug}: {view}, {spec} (from loops.json)")
else:
    sys.exit("usage: <slug> <view> <out.mp4> <pose@frame,...>  |  <slug> <out.mp4>")
RENDER = HERE.parent
REPO = RENDER.parent
poses = json.loads((HERE / "poses.json").read_text())
catalog = json.loads((REPO / "data/catalog/exercises.json").read_text())
muscle_map = json.loads((RENDER / "muscle_map.json").read_text())

keys = []
for part in spec.split(","):
    name, _, frame = part.partition("@")
    if name not in poses:
        sys.exit(f"no pose {name!r} in poses.json")
    keys.append((int(frame), name))
keys.sort()
if len(keys) < 2:
    sys.exit("a loop needs at least two keys")
if poses[keys[0][1]] != poses[keys[-1][1]]:
    sys.exit(f"first and last key are {keys[0][1]!r} and {keys[-1][1]!r} — "
             "a loop has to end where it started or it cuts")
last = keys[-1][0]

ex = next((e for e in catalog if e.get("slug") == slug), None)
if ex is None:
    sys.exit(f"no exercise with slug {slug!r} in the catalog")
prim, sec = set(), set()
for m in ex.get("muscles", []):
    if m["role"] not in ("primary", "secondary"):
        continue
    if m["slug"] not in muscle_map:
        sys.exit(f"muscle_map.json has no entry for {m['slug']!r} (needed by {slug})")
    (prim if m["role"] == "primary" else sec).update(muscle_map[m["slug"]])

stage.unstyle(bpy)
body = next(o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("MBlab"))
arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
for o in bpy.data.objects:
    if o.type == "MESH":
        o.hide_render = o.hide_viewport = o is not body
body.hide_render = body.hide_viewport = False
FLOOR_Z = min((body.matrix_world @ v.co).z for v in body.data.vertices)

# Colour once: the muscles worked do not change during the rep.
gcolour = {}
for g in body.vertex_groups:
    if not g.name.startswith(za.GROUP_PREFIX):
        continue
    region = g.name[len(za.GROUP_PREFIX):]
    if region in prim:
        gcolour[g.index] = za.RGB_PRIM
    elif region in sec:
        gcolour[g.index] = za.RGB_SEC
mesh = body.data
for a in list(mesh.color_attributes):
    mesh.color_attributes.remove(a)
attr = mesh.color_attributes.new(name="muscle", type="FLOAT_COLOR", domain="POINT")
n_prim = 0
for v in mesh.vertices:
    rgb = za.RGB_BASE
    for g in v.groups:
        c = gcolour.get(g.group)
        if c is not None:
            rgb = c
            if c == za.RGB_PRIM:
                n_prim += 1
                break
    attr.data[v.index].color = (*rgb, 1.0)
if prim and n_prim == 0:
    sys.exit("primary muscles mapped but 0 body vertices matched")

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

# Keyframe the bones. Every bone any key touches is keyed at EVERY key, or a
# bone named only in the middle key would drift in from the rest pose instead of
# from where the previous key put it.
touched = sorted({b for _, name in keys for b in poses[name]})
missing = [b for b in touched if b not in arm.pose.bones]
if missing:
    sys.exit(f"poses name bones this rig does not have: {missing}")
bpy.context.view_layer.objects.active = arm
bpy.ops.object.mode_set(mode="POSE")
for frame, name in keys:
    for bone in touched:
        pb = arm.pose.bones[bone]
        pb.rotation_mode = "XYZ"
        xyz = poses[name].get(bone, (0, 0, 0))
        pb.rotation_euler = tuple(math.radians(a) for a in xyz)
        pb.keyframe_insert("rotation_euler", frame=frame)
bpy.ops.object.mode_set(mode="OBJECT")

scene = bpy.context.scene
scene.frame_start, scene.frame_end = keys[0][0], last - 1  # last == first: drop it
scene.render.fps = 12
# ⚠ EVERY frame. The blend this pipeline loads carries `frame_step = 2` from the
# asset it descends from, and the encoder then silently writes every OTHER frame:
# a 24-frame rep comes out as 12, at double speed with half the smoothness.
#
# Nothing upstream can see it. The collision and floor checks walk all 24 frames
# and pass honestly; the loss happens afterwards, in the render loop. The tell is
# Blender's own "Append frame 0 / 2 / 4", which reads as ordinary progress.
scene.frame_step = 1


# What the rep rests on. Every key must agree: a rep that changes its contacts
# partway is a different movement, and a frame interpolated between two contact
# sets would rest on neither.
floor_sets = {tuple(plant.contacts(poses, n)) for _, n in keys}
if len(floor_sets) > 1:
    sys.exit("the keys of this loop declare different floor contacts "
             f"({sorted(floor_sets)}) — a rep rests on one set throughout")
floor_bones = floor_sets.pop()

# ⚠ SOLVE AT THE KEYS, VERIFY EVERY FRAME.
#
# Levelling is a search, around 130 pose evaluations, so solving it per frame
# would cost more than the render does. Solving at the keys and letting the
# F-curve interpolate the correction is cheap, and it is also the more honest
# shape: the tip belongs to the pose, and one re-solved per frame would wander
# by a fraction of a degree each time and read as a wobble nobody authored.
#
# What every frame gets instead is a CHECK at one evaluation each, below. An
# interpolated frame is exactly as capable of floating or sinking as an authored
# one — the same argument that already makes every frame collision-checked.
if floor_bones:
    print(f"resting on {', '.join(floor_bones)}")
    bpy.context.view_layer.objects.active = arm
    root = arm.pose.bones["root"]
    root.rotation_mode = "XYZ"
    for frame, name in keys:
        scene.frame_set(frame)
        bpy.context.view_layer.update()
        tilt, _before, spread = plant.solve_tilt(bpy, body, arm, floor_bones)
        verdict = plant.fault(name, tilt, spread, 0.0, "")
        if verdict:
            sys.exit(verdict)
        root.keyframe_insert("rotation_euler", index=0, frame=frame)
        print(f"  key {frame} ({name}): levelled {tilt:+.2f} deg, "
              f"contacts within {spread * 1000:.1f}mm")


def lowest():
    ev = body.evaluated_get(bpy.context.evaluated_depsgraph_get())
    return min((ev.matrix_world @ v.co).z for v in ev.to_mesh().vertices)


# Stand the figure on the floor on every frame, not once: the hips drop through
# the rep, and a single offset would leave it sinking and then rising again.
# With declared contacts it is the lowest CONTACT that lands, so a hand hanging
# below a shoulder cannot lever the figure up off what it is resting on.
base_z = arm.location.z
for f in range(scene.frame_start, last + 1):
    scene.frame_set(f)
    arm.location.z = base_z
    bpy.context.view_layer.update()
    if floor_bones:
        mat, me = plant.evaluated(bpy, body)
        low = min(plant.contact_heights(body, mat, me, floor_bones))
    else:
        low = lowest()
    arm.location.z = base_z + (FLOOR_Z - low)
    arm.keyframe_insert("location", index=2, frame=f)
print(f"planted {last - scene.frame_start + 1} frames on the floor")

if do_check and floor_bones:
    # The keys were solved; these are the frames nobody authored. One evaluation
    # each, which is why this can run over every frame at all.
    worst_spread = worst_sink = 0.0
    worst_f = worst_who = None
    for f in range(scene.frame_start, scene.frame_end + 1):
        scene.frame_set(f)
        bpy.context.view_layer.update()
        _zs, spread = plant.measure(bpy, body, floor_bones)
        sink, who = plant.sunk(bpy, body, FLOOR_Z)
        if spread > worst_spread or sink > worst_sink:
            worst_f, worst_who = f, who
        worst_spread = max(worst_spread, spread)
        worst_sink = max(worst_sink, sink)
    if worst_spread > plant.TOLERANCE or worst_sink > 0:
        sys.exit(f"frame {worst_f} does not rest on the floor: its contacts are "
                 f"{worst_spread * 100:.1f}cm apart and {worst_who} is "
                 f"{worst_sink * 100:.1f}cm through it — the keys plant, the path "
                 "between them does not")
    print(f"checked {scene.frame_end - scene.frame_start + 1} frames on the floor: "
          f"contacts within {worst_spread * 1000:.1f}mm, nothing through it")

if do_check:
    # A frame between two keys is not a named pose, so it inherits the
    # contacts every key of this sequence vouched for.
    allow = collide.allowed_pairs(poses, [n for _, n in keys], arm)
    worst = 0
    for f in range(scene.frame_start, scene.frame_end + 1):
        scene.frame_set(f)
        bpy.context.view_layer.update()
        faults, _ = collide.find_faults(bpy, body, arm, allow=allow)
        if faults:
            sys.exit(f"frame {f} is physically impossible — the poses at each "
                     f"end are legal but the path between them is not:\n"
                     + collide.describe(faults))
        worst = max(worst, f)
    print(f"checked {scene.frame_end - scene.frame_start + 1} frames: "
          "nothing passes through anything")

# Frame on the union of every frame, so the figure does not drift as it moves.
import mathutils  # noqa: E402  a Blender script read top to bottom; the framing starts here

umin = mathutils.Vector((1e9,) * 3)
umax = mathutils.Vector((-1e9,) * 3)
for f in range(scene.frame_start, last + 1):
    scene.frame_set(f)
    bpy.context.view_layer.update()
    fmin, fmax = stage.visible_bounds(bpy)
    umin = mathutils.Vector(map(min, umin, fmin))
    umax = mathutils.Vector(map(max, umax, fmax))
print(f"framed on the whole rep: {(umax - umin).x:.2f} x {(umax - umin).z:.2f} m")
# After the bounds, never before: a 12m slab in visible_bounds would zoom the
# camera out until the figure was a speck.
if floor_bones:
    floormod.add(bpy, FLOOR_Z)
    print("added the ground — this rep rests on it")
stage.setup(bpy, view, bounds=(umin, umax))


def stills_dir():
    for i, a in enumerate(argv):
        if a == "--stills":
            return argv[i + 1]
    return None


scene.render.engine = "BLENDER_EEVEE_NEXT"
scene.eevee.taa_render_samples = 32
scene.render.resolution_x = scene.render.resolution_y = 768
scene.render.image_settings.file_format = "FFMPEG"
scene.render.ffmpeg.format = "MPEG4"
scene.render.ffmpeg.codec = "H264"
scene.render.ffmpeg.constant_rate_factor = "HIGH"
scene.render.filepath = out_path
bpy.ops.render.render(animation=True)
print(f"WROTE {out_path}")

# A loop can only be judged by looking at it, and a video is not something every
# reviewer can open in place. Emit the keys and the midpoints between them.
out_dir = stills_dir()
if out_dir:
    scene.render.image_settings.file_format = "PNG"
    marks = sorted({f for f, _ in keys} |
                   {(a + b) // 2 for (a, _), (b, _) in zip(keys, keys[1:])})
    written = 0
    for f in marks:
        if f > scene.frame_end:
            continue  # the closing key repeats the first and is not rendered
        scene.frame_set(f)
        scene.render.filepath = f"{out_dir}/frame_{f:03d}"
        bpy.ops.render.render(write_still=True)
        written += 1
    print(f"WROTE {written} stills to {out_dir}")
