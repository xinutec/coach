#!/usr/bin/env nix-shell
#!nix-shell -i bash -p mariadb
# E1 — load the prod dump into the local dev DB and run the walk-forward
# back-test (src/bin/backtest.rs). Deterministic output → redirect to a file and
# diff across engine changes to see what a change did to real prescriptions.
#
# Prereqs:
#   1. dev DB running:   ./scripts/dev-db.sh      (another terminal)
#   2. a prod dump:      ./scripts/prod-dump.sh
#
#   ./scripts/backtest.sh                 # print the trace
#   ./scripts/backtest.sh > .dev/bt.txt   # baseline; diff a later run against it
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

echo "Running back-test ..." >&2
DATABASE_URL="$URL" nix develop "$ROOT" --command cargo run --quiet --bin backtest
