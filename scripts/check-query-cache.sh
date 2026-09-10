#!/usr/bin/env bash
# Fail if the committed .sqlx query cache no longer matches the schema.
#
# A CHANGED QUERY already fails loudly on its own: its text is the cache key, so
# an edited query simply has no cached entry and the offline build errors. This
# guards the case that stays quiet — a MIGRATION lands, the queries are
# untouched, and the cache still matches them while describing columns that have
# moved. The build stays green and the mismatch surfaces against a real row.
#
# Needs a database: DATABASE_URL, provided by the gate's ephemeral server.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${DATABASE_URL:?needs a database to check the cache against}"
sqlx migrate run
exec cargo sqlx prepare --check -- --lib
