"""Strip the Z-Anatomy atlas to a slim écorché blend.

The full Startup.blend is 7,184 objects (muscles, bones, organs, vessels,
nerves, thousands of text labels and leader curves) — rendering it whole
exhausts memory. This keeps only the muscular system and skeleton meshes, drops
everything else, purges the orphaned data, and saves a small working blend that
render.py points at. Never render Startup.blend directly.

    blender -b <Startup.blend> --python prepare.py -- <out.blend>
"""
import bpy
import sys

out = sys.argv[sys.argv.index("--") + 1:][0]

# Collections whose meshes make the écorché figure. Muscular system is the
# subject; the skeleton gives it a body to hang on. Everything else goes.
KEEP = ("Muscular system", "Skeletal system")


def kept(collection_name: str) -> bool:
    return any(k.lower() in collection_name.lower() for k in KEEP)


keep_objs = set()
for c in bpy.data.collections:
    if kept(c.name):
        for o in c.objects:
            if o.type == "MESH":
                keep_objs.add(o.name)
print(f"keeping {len(keep_objs)} meshes from {KEEP}")

# Remove every object we are not keeping (labels, curves, organs, cameras...).
for o in list(bpy.data.objects):
    if o.name not in keep_objs:
        bpy.data.objects.remove(o, do_unlink=True)

# Purge now-orphaned datablocks so the file actually shrinks. orphans_purge is
# unreliable headless, so sweep users==0 datablocks directly, repeatedly (removing
# a mesh can orphan its materials, and so on).
def sweep():
    removed = 0
    for coll in (
        bpy.data.meshes, bpy.data.curves, bpy.data.materials,
        bpy.data.images, bpy.data.armatures, bpy.data.cameras, bpy.data.lights,
    ):
        for d in list(coll):
            if d.users == 0:
                coll.remove(d)
                removed += 1
    return removed


while sweep():
    pass

bpy.ops.wm.save_as_mainfile(filepath=out, compress=True)
print(f"WROTE slim blend: {out}  objects={len(bpy.data.objects)}")
