"""The ground, for poses that rest on it.

Most of the catalog is performed standing, and a standing figure needs no floor:
it is obviously upright and the plain background keeps the attention on the
muscles. A supine one is a different picture. Without a ground reference a person
lying down reads as a person floating, and "you are on your back" is exactly the
information a demo of a floor movement exists to convey.

⚠ A PLANE IS THE WRONG SHAPE. The views are orthographic and horizontal, so a
flat plane is seen exactly edge-on and draws as a one-pixel line — in the scene,
absent from the picture. That is worse than no floor at all, because the check
looks done. Hence a slab with thickness.

Only poses that declare `_floor` contacts get one (see plant.py), so no loop
that has already shipped can change.
"""

# Wide enough to leave the frame on every view, thick enough to read as a
# surface rather than a rule.
SIZE = 6.0
THICK = 0.12
RGB = (0.62, 0.60, 0.58)


def add(bpy, z):
    """A slab whose TOP face is at `z`, the height the contacts were planted to."""
    verts = [(-SIZE, -SIZE, 0), (SIZE, -SIZE, 0), (SIZE, SIZE, 0), (-SIZE, SIZE, 0),
             (-SIZE, -SIZE, -THICK), (SIZE, -SIZE, -THICK),
             (SIZE, SIZE, -THICK), (-SIZE, SIZE, -THICK)]
    faces = [(0, 1, 2, 3), (7, 6, 5, 4), (0, 4, 5, 1),
             (1, 5, 6, 2), (2, 6, 7, 3), (3, 7, 4, 0)]
    mesh = bpy.data.meshes.new("floor")
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    ob = bpy.data.objects.new("floor", mesh)
    ob.location.z = z
    bpy.context.scene.collection.objects.link(ob)
    mat = bpy.data.materials.new("m_floor")
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*RGB, 1.0)
    bsdf.inputs["Roughness"].default_value = 0.95
    mesh.materials.append(mat)
    return ob
