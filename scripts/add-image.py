#!/usr/bin/env nix-shell
#!nix-shell -i python3 -p python3
"""Put a picture into the catalog.

    ./scripts/add-image.py curl_biceps_dumbbell_standing https://…/curl.png
    ./scripts/add-image.py squat_front_rack_double_kettlebell ~/Downloads/squat.jpg

Downloads (or copies) the image **verbatim** into `data/catalog/images/<slug>.<ext>`
and points the catalog entry at it. The bytes are not touched: an anatomy diagram
with a transparent background keeps its alpha here, because this directory is the
source, and a source that has already been flattened cannot be un-flattened.

The app can't render a transparent portrait diagram as-is — dark line-art vanishes
on a dark theme, and a 16:9 hero with `object-fit: cover` would crop a portrait
figure to a band across its stomach. That normalisation happens **once, in the
seeder** (`src/seed/mod.rs`), on the way into the database: a picture with real
transparency is composited onto white and padded to 16:9; an ordinary photo is
stored exactly as it is. Rendering is a rendering concern, and doing it there keeps
one implementation of it instead of one per import tool.
"""

import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CATALOG = ROOT / "data/catalog/exercises.json"
IMAGES = ROOT / "data/catalog/images"

CONTENT_TYPES = {
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".webp": "image/webp",
}


def fetch(src: str) -> bytes:
    if src.startswith(("http://", "https://")):
        req = urllib.request.Request(src, headers={"User-Agent": "coach-catalog"})
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.read()
    return Path(src).expanduser().read_bytes()


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(f"usage: {Path(sys.argv[0]).name} <exercise-slug> <url-or-file>")
    slug, src = sys.argv[1], sys.argv[2]

    catalog = json.loads(CATALOG.read_text())
    entry = next((e for e in catalog if e["slug"] == slug), None)
    if entry is None:
        sys.exit(f"add-image: no exercise {slug!r} in the catalog")

    # The extension comes from the source, since the bytes do too — writing PNG
    # bytes to a .jpg would make the content type a lie.
    ext = Path(src.split("?")[0]).suffix.lower()
    if ext not in CONTENT_TYPES:
        sys.exit(f"add-image: don't know what {ext or 'that'} is — expected {', '.join(CONTENT_TYPES)}")

    raw = fetch(src)
    dest = IMAGES / f"{slug}{ext}"
    dest.write_bytes(raw)

    # A slug can only have one picture; drop any earlier one in a different format.
    for other in IMAGES.glob(f"{slug}.*"):
        if other != dest:
            other.unlink()

    entry["image"] = {"file": dest.name, "type": CONTENT_TYPES[ext]}
    CATALOG.write_text(json.dumps(catalog, indent=2, ensure_ascii=False) + "\n")

    print(f"  {slug}: {dest.relative_to(ROOT)} ({len(raw) // 1024} KB, stored as-is)")
    print("  the seeder renders it on the next deploy.")


if __name__ == "__main__":
    main()
