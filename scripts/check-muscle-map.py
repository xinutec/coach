"""Every name in muscle_map.json must be a mesh the atlas actually models.

    blender -b render/slim.blend --python scripts/check-muscle-map.py

A name that matches nothing colours nothing, silently. muscle_map.json's own
comment used to call that harmless; it is how `erector_spinae` came to point at
"Erector spinae" — which Z-Anatomy models only as a two-vertex ANNOTATION, so
eight exercises coloured nothing for a PRIMARY muscle and one of them shipped a
loop that way, live, for nine days.

The renderers cannot catch it. They fail when a slug matches ZERO meshes in
total, so a slug naming two primaries hides a broken name behind a working one —
which is exactly what cat_cow does. This check is per NAME, which is the level
the mistake happens at.

⚠ Runs against slim.blend, the stripped atlas the renderers use — not the full
one. A name that exists only in a mesh prepare.py discards is still broken here,
and should be: it cannot colour anything either.
"""
import bpy
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "render"))
import za  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
mmap = json.loads((REPO / "render/muscle_map.json").read_text())

real = {}
for o in bpy.data.objects:
    if o.type != "MESH" or za.is_label(o.name):
        continue
    base = za.base(o.name)
    real[base] = real.get(base, 0) + len(o.data.vertices)

bad = {}
for slug, names in mmap.items():
    if slug.startswith("_"):
        continue  # _comment is prose, not a mapping
    missing = [n for n in names if n not in real]
    if missing:
        bad[slug] = missing

for slug, missing in sorted(bad.items()):
    print(f"{slug}: names nothing in the atlas: {missing}")
    stem = slug.split("_")[0][:6].lower()
    near = sorted(b for b in real if stem in b.lower())[:8]
    if near:
        print(f"    did you mean: {near}")

print(f"checked {len([k for k in mmap if not k.startswith('_')])} slugs "
      f"against {len(real)} modelled meshes: "
      + (f"{len(bad)} BROKEN" if bad else "every name resolves"))
sys.exit(1 if bad else 0)
