"""Stand a posed figure on the floor, including poses that rest on two contacts.

The plain case is one contact: a standing figure meets the floor at its feet, so
dropping the rig until its lowest vertex touches is both necessary and
sufficient. Bending the hips moves the feet and not the root, so without the drop
a posed figure hangs in the air — see `drop`.

A floor pose is not that case. A glute bridge rests on the shoulders AND the
feet while everything between them rises, and no single-point drop can hold two
contacts: it lands whichever is lower and leaves the other in the air. Worse, the
error is invisible to every number you would naturally check — hip height,
shoulder height, foot height are all satisfied by a body that has simply rotated
as a whole, which is what a failed attempt at this actually produced (a straight
diagonal plank, feet highest, with every probe reading correct).

What makes it tractable is that the pose SAYS what it is resting on. Nothing else
knows: `spine03, foot_L, foot_R` is a bridge and `foot_L, foot_R` is standing, and
the geometry alone cannot tell you which contact is meant to be load-bearing and
which is incidentally low. With the contacts named, one angle is left free — how
far the figure tips along its own length — and that is solved here by search
rather than by closed-form levelling. A closed form has to decide WHICH axis
"along the body" is, and that axis is z when standing and y when lying down;
treating it as fixed rotates a squat by 27.9 degrees, silently, because a squat
looks plausible at any angle.

⚠ A pose that declares no contacts takes exactly the old path. That is the whole
safety argument for this file: `stand`, `squat` and `hinge_bottom` cannot be
touched by code they never reach.
"""
import math

# A declared contact this far off the floor (metres) is a pose that does not rest
# where it says it does. 8mm is under the skin's own detail — the sole of a foot
# is not flat — and far below the tens of centimetres a wrong pose is out by.
TOLERANCE = 0.008
# Ignore vertices only lightly driven by the contact bone: at low weights a
# vertex belongs as much to the neighbouring bone, and the lowest such vertex can
# sit well outside the part actually touching the floor.
WEIGHT_MIN = 0.5
# How far the solver may tip the figure from the angle the pose authored, and how
# finely it looks. +/- 45 degrees is enough to take a supine body to any
# plausible lean without ever being able to stand it up.
SEARCH_DEG = 45.0
SEARCH_STEPS = 90
# ⚠ How much tipping is a CORRECTION rather than a cover-up.
#
# The search will happily level any two contacts by rotating the whole figure,
# and a body rotated as a rigid whole satisfies "both contacts on the floor" just
# as well as one actually resting on them: a bridge with a wrong torso levels at
# +15.9 degrees into a straight ramp with the head 9cm underground. A pose that is right needs well under a
# degree, so anything past this is reported as a fault in the POSE.
MAX_TILT_DEG = 5.0


def contacts(poses, name):
    """The bones a pose declares as resting on the floor. Empty for most poses."""
    if name.startswith("_"):
        return ()
    return tuple(poses.get("_floor", {}).get(name, ()))


def check_contacts(poses, body):
    """Every declared contact names a real deform group. Call once, early: a typo
    here would otherwise read as a contact that is simply never found."""
    bad = {}
    for name, bones in poses.get("_floor", {}).items():
        # `_comment` and friends live in this table too. Without this guard the
        # comment STRING is iterated as a list of bone names, one character at a
        # time, and the error names 400 missing bones called "W", "h", "a"...
        if name.startswith("_"):
            continue
        missing = [b for b in bones if b not in body.vertex_groups]
        if missing:
            bad[name] = missing
    return bad


def evaluated(bpy, body):
    """The posed mesh and its world matrix.

    Returned rather than flattened to world coordinates because the solver wants
    the height of a few thousand contact vertices, not of all 276,437: building
    the full list is the single most expensive thing in a solve and almost none
    of it is read.
    """
    ev = body.evaluated_get(bpy.context.evaluated_depsgraph_get())
    me = ev.to_mesh()
    if len(me.vertices) != len(body.data.vertices):
        # Vertex groups are read off the original mesh and indexed into the
        # evaluated one, so a modifier that changes the count would silently
        # measure the wrong vertices rather than fail.
        raise SystemExit(
            f"the evaluated body has {len(me.vertices)} vertices and the original "
            f"{len(body.data.vertices)} — a modifier is changing the topology, so "
            "vertex groups can no longer locate a contact"
        )
    return ev.matrix_world, me


# Which vertices belong to which contact bone, cached across a run.
#
# ⚠ This cache is what makes the solver usable at all. Scanning all 276,437
# vertices and their group memberships in Python costs about a second; the
# search evaluates the pose ~110 times per solve, so measuring from scratch each
# time turned a solve into minutes of work for an answer that never changes.
# Weights do not move when a bone rotates — only positions do.
_VERTS = {}


def _contact_verts(body, bone):
    key = (body.name, bone)
    if key not in _VERTS:
        gi = body.vertex_groups[bone].index
        idx = [v.index for v in body.data.vertices
               if any(g.group == gi and g.weight >= WEIGHT_MIN for g in v.groups)]
        if not idx:
            raise SystemExit(
                f"no vertex is driven at least {WEIGHT_MIN} by {bone!r}, so it "
                "cannot be used as a floor contact"
            )
        _VERTS[key] = idx
    return _VERTS[key]


def contact_heights(body, mat, me, bones):
    """The lowest skin height under each contact bone, in the order given."""
    verts = me.vertices
    return [min((mat @ verts[i].co).z for i in _contact_verts(body, bone))
            for bone in bones]


def measure(bpy, body, bones):
    """Contact heights, and how far apart the highest and lowest are."""
    mat, me = evaluated(bpy, body)
    zs = contact_heights(body, mat, me, bones)
    return zs, max(zs) - min(zs)


def solve_tilt(bpy, body, arm, bones, root="root"):
    """Tip the figure along its own length until every contact rests level.

    Returns (degrees applied, spread before, spread after). The search is a
    coarse sweep then a refinement rather than a formula: the relationship
    between the angle and the spread runs through a skinned, corrective-smoothed
    mesh, so it is not the clean trigonometry the closed form assumed.
    """
    if root not in arm.pose.bones:
        raise SystemExit(f"the rig has no {root!r} bone to tip")
    pb = arm.pose.bones[root]
    pb.rotation_mode = "XYZ"
    base = pb.rotation_euler.x

    def spread_at(delta):
        pb.rotation_euler.x = base + math.radians(delta)
        bpy.context.view_layer.update()
        return measure(bpy, body, bones)[1]

    before = spread_at(0.0)
    best_d, best_s = 0.0, before
    step = SEARCH_DEG * 2 / SEARCH_STEPS
    for i in range(SEARCH_STEPS + 1):
        d = -SEARCH_DEG + i * step
        s = spread_at(d)
        if s < best_s:
            best_d, best_s = d, s
    # Refine inside the winning bracket. Two passes at a tenth of the step each
    # take the resolution below the tolerance without another full sweep.
    for _ in range(2):
        step /= 10
        for i in range(-10, 11):
            d = best_d + i * step
            s = spread_at(d)
            if s < best_s:
                best_d, best_s = d, s
    pb.rotation_euler.x = base + math.radians(best_d)
    bpy.context.view_layer.update()
    return best_d, before, best_s


def fault(name, tilt, spread, sink, who):
    """The one-line reason a floor pose is not resting where it says, or None.

    Collected here rather than at each call site so the renderer, the animator
    and the checker cannot disagree about what counts as planted.
    """
    if abs(tilt) > MAX_TILT_DEG:
        return (f"{name}: levelling its contacts needs {tilt:+.1f} deg of tip, over "
                f"the {MAX_TILT_DEG:.0f} deg limit — the pose is rotating as a whole "
                "rather than resting on the floor")
    if spread > TOLERANCE:
        return (f"{name}: its contacts are {spread * 100:.1f}cm apart in height and "
                "cannot be levelled by tipping — they are not all reachable at once")
    if sink > 0:
        return (f"{name}: {who} passes {sink * 100:.1f}cm through the floor while the "
                "declared contacts rest on it")
    return None


def drop(bpy, body, arm, floor_z, bones=()):
    """Lower the rig onto the floor, and say by how much.

    With no declared contacts this is the historical behaviour exactly: the
    lowest vertex anywhere on the body meets the floor. With contacts, it is the
    lowest DECLARED contact that meets it — so a fingertip hanging below a
    shoulder cannot push the whole figure up off what it is resting on.
    """
    mat, me = evaluated(bpy, body)
    if bones:
        z = min(contact_heights(body, mat, me, bones))
    else:
        z = min((mat @ v.co).z for v in me.vertices)
    arm.location.z += floor_z - z
    bpy.context.view_layer.update()
    return floor_z - z


_OWNER = {}


def _owner(body):
    """Each vertex's dominant deform bone, so a fault can be NAMED.

    "Something is 9cm through the floor" sends you looking at the whole figure;
    "head is 9cm through the floor" is the answer. Built once — weights do not
    move when a bone rotates.
    """
    if body.name not in _OWNER:
        deform = {g.index: g.name for g in body.vertex_groups
                  if not g.name.startswith("mus:")}
        own = {}
        for v in body.data.vertices:
            best = None
            for g in v.groups:
                name = deform.get(g.group)
                if name is not None and (best is None or g.weight > best[1]):
                    best = (name, g.weight)
            if best:
                own[v.index] = best[0]
        _OWNER[body.name] = own
    return _OWNER[body.name]


def sunk(bpy, body, floor_z, margin=0.01):
    """How far the lowest vertex sits below the floor, and what it belongs to.

    Only meaningful once contacts are planted, and it is the check that catches
    the failure a contact check cannot: a figure resting exactly on everything it
    declared, with a head or an elbow passing through the ground behind it. The
    margin absorbs the millimetre the skin's own detail puts under a sole.
    """
    mat, me = evaluated(bpy, body)
    own = _owner(body)
    low, who = None, None
    for v in me.vertices:
        z = (mat @ v.co).z
        if low is None or z < low:
            low, who = z, own.get(v.index, "?")
    return max(0.0, floor_z - low - margin), who
