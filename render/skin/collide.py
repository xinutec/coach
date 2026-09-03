"""Find places where the posed skin passes through itself.

Posing has no collision: bones rotate, skin follows, and a limb swung into the
torso simply occupies the same space. Some of that is wanted — a deep squat
presses hamstring against calf, and skin folds at a closed hip. What is not is a
hand entering the buttock and emerging at the crotch.

The difference is not whether surfaces intersect but WHICH parts do. Every
intersecting triangle is attributed to the bone that drives it, and the two
bones are measured apart along the skeleton. Neighbours sharing space is a
crease; a hand seven joints from the pelvis sharing space with it is not.
"""
from collections import defaultdict, deque

from mathutils.bvhtree import BVHTree

# How many joints apart two surfaces must be before shared space is a fault
# rather than a fold. Within a limb is 1-2 and thigh-to-thigh is 4, both of
# which touch legitimately; a hand reaches the pelvis in 7.
MIN_HOPS = 5
# Under this many triangles, treat it as a graze rather than something buried.
MIN_TRIANGLES = 40


def _hop_table(armature):
    adj = defaultdict(set)
    for b in armature.data.bones:
        if b.parent:
            adj[b.name].add(b.parent.name)
            adj[b.parent.name].add(b.name)
    table = {}
    for b in armature.data.bones:
        seen = {b.name: 0}
        q = deque([b.name])
        while q:
            cur = q.popleft()
            for nxt in adj[cur]:
                if nxt not in seen:
                    seen[nxt] = seen[cur] + 1
                    q.append(nxt)
        table[b.name] = seen
    return table


def find_faults(bpy, body, armature, min_hops=MIN_HOPS, min_triangles=MIN_TRIANGLES):
    """Return [(bone_a, bone_b, triangles, hops)], worst first, and the pair count."""
    hops = _hop_table(armature)
    bone_of_group = {g.index: g.name for g in body.vertex_groups if g.name in hops}

    def dominant(vert):
        best, best_w = None, 0.0
        for g in vert.groups:
            name = bone_of_group.get(g.group)
            if name is not None and g.weight > best_w:
                best, best_w = name, g.weight
        return best

    ev = body.evaluated_get(bpy.context.evaluated_depsgraph_get())
    mesh = ev.to_mesh()
    mesh.calc_loop_triangles()
    verts = [(ev.matrix_world @ v.co)[:] for v in mesh.vertices]
    tris = [t.vertices[:] for t in mesh.loop_triangles]
    # Weights live on the ORIGINAL vertices; the evaluated mesh shares their
    # indices because every modifier left in the stack is a deformer.
    owner = [dominant(body.data.vertices[t[0]]) for t in tris]
    bvh = BVHTree.FromPolygons(verts, tris, all_triangles=True)
    pairs = bvh.overlap(bvh)

    guilty = defaultdict(set)
    for i, j in pairs:
        a, b = owner[i], owner[j]
        if a is None or b is None or a == b:
            continue
        if hops[a].get(b, 99) < min_hops:
            continue  # neighbouring parts: a crease, not a collision
        guilty[tuple(sorted((a, b)))] |= {i, j}
    ev.to_mesh_clear()

    faults = [(a, b, len(t), hops[a][b]) for (a, b), t in guilty.items()
              if len(t) >= min_triangles]
    faults.sort(key=lambda f: -f[2])
    return faults, len(pairs)


def describe(faults):
    return "\n".join(f"    {a} through {b}: {n} triangles, {h} joints apart"
                     for a, b, n, h in faults)
