"""Z-Anatomy naming and palette, shared by every renderer.

The atlas carries text/guide meshes and connective sheets alongside the anatomy,
and splits each muscle across side/variant suffixes. Every script that reads the
atlas needs the same three answers, and the écorché and the skinned body must
use the same reds — two copies of either would let one renderer drift into
disagreeing with the other about what a picture means.
"""
import re

# Side/variant/label suffixes: .l .r .ol .or .el .er .j .t .i .s .g
_SUFFIX = re.compile(r"\.(o?l|o?r|e?l|e?r|j|t|i|s|g)$")

# Connective-tissue sheets (fascia lata, investing abdominal fascia, aponeuroses,
# retinacula) wrap the muscles as a smooth outer envelope. They occlude the
# muscles beneath in a render, and they are the nearest surface to most of the
# skin, so they mislead a nearest-vertex lookup just as badly as they occlude.
_ENVELOPE = ("fascia", "aponeurosis", "retinaculum", "sheath", "membrane")


def base(name: str) -> str:
    """The muscle's name with its side/variant suffix stripped."""
    return _SUFFIX.sub("", name).strip()


# Annotation suffixes. Real anatomy carries .l, .r or no suffix at all; .j, .i
# and .g are the atlas's own callout geometry — 1,051 meshes, most of them
# zero-thickness planes, many sitting well off the figure's axis. They are
# invisible in a lit render, which is why they went unnoticed, but they are
# ordinary surfaces to a nearest-surface query and they wreck a bounding box.
_ANNOTATION = (".j", ".i", ".g")


def is_label(name: str) -> bool:
    """True for the atlas's own text and callout meshes, which are not anatomy."""
    b = base(name)
    return (
        name.endswith(_ANNOTATION)
        or b in ("Muscular system", "Skeletal system")
        or b.isupper()
    )


def is_envelope(name: str) -> bool:
    low = name.lower()
    if "tensor fasciae latae" in low:  # a real muscle, not a fascia sheet
        return False
    return any(k in low for k in _ENVELOPE)


# Muted flesh for non-target tissue, dark red primary, light red secondary.
RGB_BASE = (0.80, 0.62, 0.55)
RGB_PRIM = (0.62, 0.03, 0.03)
RGB_SEC = (0.90, 0.34, 0.30)


# Vertex groups written onto the skinned body carry this prefix. The body
# already has one vertex group per bone driving its armature, so an unprefixed
# muscle name could collide with a bone name and silently corrupt the rig.
GROUP_PREFIX = "mus:"
