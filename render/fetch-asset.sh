#!/usr/bin/env bash
# Fetch + unpack the Z-Anatomy atlas (the model is inside Startup.blend).
# Idempotent: skips the download when the blend is already present, so the CI
# cache makes repeat runs instant. The archive lives on the Z-Anatomy repo's
# `master` branch (NOT main — a raw/main URL 404s to an HTML page).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p asset
BLEND=asset/anatomy/Z-Anatomy/Startup.blend
URL=https://raw.githubusercontent.com/Z-Anatomy/Models-of-human-anatomy/master/Z-Anatomy.zip

if [ -f "$BLEND" ]; then
  echo "asset already present: $BLEND"
  exit 0
fi
echo "downloading Z-Anatomy.zip ..."
curl -fsSL -o asset/Z-Anatomy.zip "$URL"
# guard against the 404-HTML-page failure mode: expect a real zip (~87 MB)
size=$(wc -c < asset/Z-Anatomy.zip)
[ "$size" -gt 50000000 ] || { echo "download too small ($size bytes) — not the zip"; exit 1; }
unzip -o -q asset/Z-Anatomy.zip -d asset/anatomy
test -f "$BLEND"
echo "asset ready: $BLEND"
