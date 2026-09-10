#!/usr/bin/env bash
# Build the backend from ONLY the files the Dockerfile copies into the image.
#
# The gate builds from the working tree, where every file exists. The image gets
# what its COPY lines name and nothing else, so a file the build needs but no
# COPY mentions passes the gate and fails in CI — which is exactly how .sqlx
# went in: the offline build was verified inside the repo, where the directory
# is obviously present, and the image had no idea it existed.
#
# So: assemble a tree from the Dockerfile's own COPY lines and compile there,
# with no database, the way the image does.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# The backend stage's COPY lines: everything up to the runtime stage, minus the
# ones pulling from an earlier stage.
copies=$(awk '
    /^FROM .* AS backend/ { inb = 1; next }
    /^FROM/ && inb        { exit }
    inb && /^COPY/ && !/--from=/ { print }
' Dockerfile)
[ -n "$copies" ] || { echo "no backend COPY lines found — did the Dockerfile change shape?" >&2; exit 1; }

while read -r _ rest; do
    # shellcheck disable=SC2086
    set -- $rest
    dest="${*: -1}"
    for src in "${@:1:$#-1}"; do
        target="$WORK/${dest%/}"
        mkdir -p "$target"
        # Docker's trailing slash means "the CONTENTS of", not the directory
        # itself — `COPY src/ src/` must not produce src/src.
        case "$src" in
            */) cp -R "$ROOT/${src%/}/." "$target/" ;;
             *) cp -R "$ROOT/$src" "$target/" ;;
        esac
    done
done <<< "$copies"

echo "image sees: $(cd "$WORK" && ls -A | tr '\n' ' ')"
cd "$WORK"
# Reuse the repo's target dir; this is about which FILES are present, not a
# cold build, and a cold one would cost the gate ten minutes for nothing.
env -u DATABASE_URL CARGO_TARGET_DIR="$ROOT/target" cargo check --quiet
echo "the backend compiles from the image's file set alone"
