#!/usr/bin/env bash
# Dump the prod coach DB → .dev/coach-prod.sql (gitignored) for local
# back-testing (scripts/backtest.sh). Read-only on prod: mariadb-dump with a
# consistent InnoDB snapshot, no locks. The auth `sessions` table (live login
# tokens) is deliberately excluded — the back-test never needs it and those
# tokens shouldn't sit in a local file.
#
# Requires: ssh root@isis.xinutec.org + kubectl access to the coach namespace.
#
#   ./scripts/prod-dump.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/.dev/coach-prod.sql"
mkdir -p "$ROOT/.dev"

echo "Dumping prod coach DB → $OUT (read-only) ..."
ssh root@isis.xinutec.org \
  "kubectl -n coach exec deploy/coach-db -- sh -c 'mariadb-dump -uroot -p\"\$MARIADB_ROOT_PASSWORD\" --single-transaction --skip-lock-tables --ignore-table=coach.sessions coach'" \
  >"$OUT"

echo "Wrote $(wc -l <"$OUT" | tr -d ' ') lines to $OUT"
