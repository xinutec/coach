#!/usr/bin/env bash
# Resilient `ng build` for the verify path.
#
# @angular/build:application tears down its Piscina worker pool at process exit;
# on macOS / Node 24 / libuv 1.52 that teardown intermittently aborts the
# process — a libuv kqueue assertion ("errno == EINTR", uv__io_poll → Abort 6)
# or "EBADF: bad file descriptor, close" — AFTER "Application bundle generation
# complete", i.e. once a complete, valid bundle is already on disk.
# NG_BUILD_MAX_WORKERS=1 (set by the gate row) lowers the rate but does NOT
# eliminate it. Rather than make every commit flaky and re-run by hand, treat
# "bundle completed, then aborted in worker teardown" as the success it is (the
# artifact is valid), and retry once for a genuine mid-build failure. Harmless on
# Linux/CI, which build cleanly and exit 0 on the first pass.
#
# Args pass straight through to `ng build` (e.g. --configuration production).
set -euo pipefail

COMPLETE="Application bundle generation complete"
OUT="$(dirname "$0")/../dist/coach-web/browser"
log="$(mktemp)"
trap 'rm -f "$log"' EXIT

for attempt in 1 2; do
  # Run the build as an if-condition so a non-zero exit doesn't trip errexit —
  # we need to inspect the code and the output before deciding it's fatal.
  if pnpm exec ng build "$@" 2>&1 | tee "$log"; then
    exit 0
  fi
  rc=${PIPESTATUS[0]}
  # "Complete" is necessary but NOT sufficient: Angular flushes some of the
  # output AFTER that line, so the abort can land mid-write. Observed on
  # 2026-07-26 — the log said complete, the script called it success, and dist/
  # held the assets and the stylesheet but no index.html and no main-*.js, which
  # the static server then served as a 404. Check the entry points are actually
  # there before believing the message, and retry if they aren't.
  if grep -qF "$COMPLETE" "$log" && [ -f "$OUT/index.html" ] && compgen -G "$OUT/main-*.js" >/dev/null; then
    echo "" >&2
    echo "[ng-build] bundle completed and is on disk; the process aborted in Piscina worker teardown (rc=$rc) — treating as success." >&2
    exit 0
  fi
  echo "[ng-build] build did not complete (attempt ${attempt}/2, rc=$rc)." >&2
done

echo "[ng-build] build failed to complete after 2 attempts." >&2
exit 1
