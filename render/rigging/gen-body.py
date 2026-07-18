"""Generate a rigged male body with MB-Lab (headless).

MB-Lab (free Blender add-on) produces a rigged, weighted humanoid we use only as
a *deformation donor*: its skeleton + weights get transferred onto the Z-Anatomy
écorché (see transfer-rig.py), which itself has no rig. No GUI, no login, no cost.

    Blender -b --python gen-body.py -- <mblab.zip> <out.blend>

Get mblab.zip from github.com/animate1978/MB-Lab (tag 1_8_1). Runs in Blender
4.2 and 5.1. The zip path MUST be absolute — a relative path makes the installer
fail with FileNotFoundError ''. See docs/anatomy-renders.md.
"""
import bpy
import addon_utils
import os
import sys

argv = sys.argv[sys.argv.index("--") + 1:]
zip_path = os.path.abspath(argv[0])
out = os.path.abspath(argv[1])

bpy.ops.preferences.addon_install(filepath=zip_path, overwrite=True)
name = next(m.__name__ for m in addon_utils.modules() if "MB-Lab" in m.__name__)
bpy.ops.preferences.addon_enable(module=name)

scn = bpy.context.scene
scn.mblab_character_name = "m_ca01"          # male caucasian base
bpy.ops.mbast.init_character()
scn.mblab_body_tone = 0.9                     # muscle definition
scn.mblab_body_mass = 0.35
bpy.ops.mbast.finalize_character()

for me in [o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("MBlab")]:
    print(f"body mesh: {len(me.data.vertices)} verts, {len(me.vertex_groups)} vgroups")
bpy.ops.wm.save_as_mainfile(filepath=out)
print("WROTE", out)
