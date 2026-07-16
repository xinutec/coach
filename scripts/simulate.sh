#!/usr/bin/env nix-shell
#!nix-shell -i bash -p mariadb
# E3 — load the prod dump into the local dev DB and simulate an athlete into
# the future (src/bin/simulate.rs): the coach prescribes, a deterministic
# athlete performs and logs, the walk continues on the grown history.
# Deterministic output → redirect to a file and diff across engine changes.
#
# Prereqs:
#   1. dev DB running:   ./scripts/dev-db.sh      (another terminal)
#   2. a prod dump:      ./scripts/prod-dump.sh
#
#   ./scripts/simulate.sh                              # 8 weeks, improver
#   SIM_ATHLETE=plateauer ./scripts/simulate.sh        # temperaments: improver | plateauer | badweek
#   SIM_WEEKS=12 SIM_LOCATION=Office ./scripts/simulate.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DUMP="$ROOT/.dev/coach-prod.sql"
PORT=3308
URL="mysql://coach:coach@127.0.0.1:${PORT}/coach"

[ -f "$DUMP" ] || {
  echo "no dump at $DUMP — run ./scripts/prod-dump.sh first" >&2
  exit 1
}

echo "Loading $DUMP into dev DB (127.0.0.1:${PORT}) ..." >&2
mariadb -h127.0.0.1 -P"$PORT" -ucoach -pcoach coach <"$DUMP"

echo "Running simulation ..." >&2
DATABASE_URL="$URL" nix develop "$ROOT" --command cargo run --quiet --bin simulate
