#!/usr/bin/env nix-shell
#!nix-shell -i bash -p gh curl python3
# Deploy the *current commit* to isis, and prove it landed.
#
# Three failure modes this exists to rule out, all of which have bitten us:
#
#   1. Racing CI. `gh run list --limit 1` right after a push returns the *previous*
#      commit's run (GitHub hasn't created the new one yet). Waiting on that and
#      then restarting pulls an image built from the commit before — a deploy that
#      reports success and ships nothing. So we wait for the run whose headSha is
#      HEAD, and fail if it never appears.
#   2. Trusting the rollout. `kubectl rollout status` says a *pod came up*, not
#      which image it came up on. So afterwards we ask the running server what
#      commit it is (GET /version, baked in at image-build time) and require it to
#      equal HEAD. Comparing bundle hashes instead would be wrong twice over: a
#      backend-only commit legitimately leaves the bundle byte-identical, and an
#      unchanged hash can't distinguish "nothing to change" from "shipped nothing".
#   3. Deploying a dirty tree. What CI built is what's committed; a local edit that
#      isn't pushed is not in the image, however green everything looks.
set -euo pipefail

REMOTE="root@isis.xinutec.org"
URL="https://coach.xinutec.org"
SHA="$(git rev-parse HEAD)"

if [ -n "$(git status --porcelain)" ]; then
  echo "worktree is dirty — commit before deploying, or you'll ship the last commit" >&2
  exit 1
fi
if [ -n "$(git log origin/main..HEAD --oneline)" ]; then
  echo "HEAD is not pushed — CI has nothing to build" >&2
  exit 1
fi

echo "deploying ${SHA:0:8} (running now: $(curl -fsS "$URL/version" || echo unknown))"

echo "waiting for the CI run of ${SHA:0:8} ..."
for _ in $(seq 1 90); do
  read -r status conclusion <<<"$(gh run list --branch main --limit 20 \
    --json headSha,status,conclusion \
    -q "[.[] | select(.headSha == \"$SHA\")][0] | \"\(.status) \(.conclusion // \"-\")\"" 2>/dev/null || echo "none -")"
  case "$status" in
    completed)
      [ "$conclusion" = "success" ] || { echo "CI failed for ${SHA:0:8}: $conclusion" >&2; exit 1; }
      break
      ;;
    none | null) echo "  no run for this commit yet ..." ;;
    *) echo "  CI $status ..." ;;
  esac
  sleep 20
done
[ "${status:-}" = "completed" ] || { echo "timed out waiting for CI on ${SHA:0:8}" >&2; exit 1; }

echo "CI green — rolling out"
ssh "$REMOTE" "kubectl -n coach rollout restart deploy/coach-app && kubectl -n coach rollout status deploy/coach-app --timeout=180s"

# A pod being up is not the same as *this commit* being up. Ask it.
for _ in $(seq 1 20); do
  running="$(curl -fsS "$URL/version" || true)"
  if [ "$running" = "$SHA" ]; then
    echo "deployed: $URL is running ${SHA:0:8}"
    exit 0
  fi
  sleep 4
done
echo "rollout finished but $URL reports ${running:-unknown}, not ${SHA:0:8} — the image does not contain this commit" >&2
exit 1
