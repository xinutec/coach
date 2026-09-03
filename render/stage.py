"""Camera, lighting and render settings — the look every renderer must share.

An écorché and a skinned body only tell you something when you put them side by
side, and they only compare if the light is the same. Both are lit here.

Suns are aimed RELATIVE TO THE CAMERA so the surface facing us is always lit
obliquely, which is what reveals relief. Aiming them in world space lit the far
side on a back view and came out flat.
"""
import math

import mathutils

VIEWS = {"front": (0, -1, 0), "back": (0, 1, 0), "left": (-1, 0, 0), "right": (1, 0, 0)}


def visible_bounds(bpy):
    mins = mathutils.Vector((1e9,) * 3)
    maxs = mathutils.Vector((-1e9,) * 3)
    for o in bpy.data.objects:
        if o.type != "MESH" or o.hide_render:
            continue
        for c in o.bound_box:
            w = o.matrix_world @ mathutils.Vector(c)
            mins = mathutils.Vector(map(min, mins, w))
            maxs = mathutils.Vector(map(max, maxs, w))
    return mins, maxs


def setup(bpy, view):
    """Frame the visible figure orthographically from `view` and light it."""
    if view not in VIEWS:
        raise SystemExit(f"unknown view {view!r} — expected one of {sorted(VIEWS)}")
    mins, maxs = visible_bounds(bpy)
    center = (mins + maxs) / 2
    size = maxs - mins

    cam_data = bpy.data.cameras.new("cam")
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = max(size.x, size.z) * 1.15
    cam = bpy.data.objects.new("cam", cam_data)
    bpy.context.scene.collection.objects.link(cam)
    d = mathutils.Vector(VIEWS[view])
    cam.location = center + d * max(size) * 3
    cam.rotation_euler = (center - cam.location).normalized().to_track_quat("-Z", "Y").to_euler()
    bpy.context.scene.camera = cam

    # Low ambient so directional light, not a flat sky, defines the relief.
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (1, 1, 1, 1)
    world.node_tree.nodes["Background"].inputs[1].default_value = 0.15
    bpy.context.scene.world = world

    cam_dir = (center - cam.location).normalized()  # into the scene, away from camera
    right = cam_dir.cross(mathutils.Vector((0, 0, 1))).normalized()
    up = right.cross(cam_dir).normalized()

    def add_sun(name, energy, travel):
        light = bpy.data.lights.new(name, "SUN")
        light.energy = energy
        # Blender's sun defaults to a 0.526° disc, which casts a razor edge. An
        # arm held towards the camera then lays a hard-edged wedge across the
        # torso that reads as a rendering fault rather than as shadow. Widen the
        # source so the shadow has a penumbra.
        light.angle = math.radians(12.0)
        o = bpy.data.objects.new(name, light)
        bpy.context.scene.collection.objects.link(o)
        # 'travel' is the direction the light moves, so an upper-left source
        # travels down-right-forward.
        o.rotation_euler = travel.normalized().to_track_quat("-Z", "Y").to_euler()

    # A key of 4.0 on a 0.80-albedo surface clips to white: the figure lost all
    # its form and read as a plaster cast. Keep the midtones off the ceiling so
    # shading can describe the shape.
    add_sun("key", 2.1, cam_dir - up * 0.5 + right * 0.5)
    add_sun("fill", 0.7, cam_dir + up * 0.3 - right * 0.5)


def render_png(bpy, out_png, res=768):
    """Render to `out_png`. Cycles unless COACH_RENDER_ENGINE says otherwise.

    Set COACH_RENDER_ENGINE=EEVEE while iterating on a pose or a colour map,
    where the question is where things are rather than how they are lit. What
    ships is Cycles; an animation may not be able to afford it, which is a
    measurement to make rather than a preference to hold.
    """
    import os

    scene = bpy.context.scene
    if os.environ.get("COACH_RENDER_ENGINE", "").upper().startswith("EEVEE"):
        scene.render.engine = "BLENDER_EEVEE_NEXT"
        scene.eevee.taa_render_samples = 32
    else:
        scene.render.engine = "CYCLES"
        scene.cycles.samples = 64
        scene.cycles.use_denoising = True
        scene.cycles.device = "CPU"
    scene.render.resolution_x = res
    scene.render.resolution_y = res
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = out_png
    bpy.ops.render.render(write_still=True)
    print(f"WROTE {out_png}")


def unstyle(bpy):
    """Drop Z-Anatomy's compositor + Freestyle sepia 'sketch' post-process.

    The atlas bakes it over every render, and it overrode all material and
    lighting changes — identical output across edits was the tell.
    """
    scene = bpy.context.scene
    for vl in scene.view_layers:
        vl.material_override = None
        vl.use_freestyle = False
    scene.use_nodes = False
    scene.render.use_freestyle = False
