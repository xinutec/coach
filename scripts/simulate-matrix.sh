#!/usr/bin/env nix-shell
#!nix-shell -i bash -p mariadb
# E3, swept — run src/bin/simulate.rs across a matrix of athletes instead of one.
#
# A single simulated future answers "does the coach handle *this* athlete?".
# The interesting question is the one a single run can't ask: which athlete does
# it handle badly? So this loads the prod dump once and walks the same weeks for
# every temperament (how strong they are, and where that goes) crossed with every
# behaviour (whether they do what they're told), writing one trace per cell.
#
# The sim never writes sets back, so one dump load serves every cell and the
# cells are independent — same soil, different athlete.
#
# Prereqs:
#   1. dev DB running:   ./scripts/dev-db.sh      (another terminal)
#   2. a prod dump:      ./scripts/prod-dump.sh
#
#   ./scripts/simulate-matrix.sh                    # the default sweep
#   SIM_WEEKS=12 ./scripts/simulate-matrix.sh       # longer futures
#   OUT=.dev/round7 ./scripts/simulate-matrix.sh    # somewhere else
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DUMP="$ROOT/.dev/coach-prod.sql"
OUT="${OUT:-$ROOT/.dev/matrix}"
PORT=3308
URL="mysql://coach:coach@127.0.0.1:${PORT}/coach"

[ -f "$DUMP" ] || {
  echo "no dump at $DUMP — run ./scripts/prod-dump.sh first" >&2
  exit 1
}

# Each cell is "athlete:behaviour:recovery". The sweep is deliberately not the
# full cross product: temperament and behaviour are independent axes, so varying
# one at a time against a fixed other isolates which axis a finding belongs to,
# and the handful of crosses at the end are the combinations that plausibly
# interact (a novice who also skips; an injury you sleep badly through).
#
# A probe can name its own: once a finding is localised to two or three cells,
# re-running the other twelve to see them not move is minutes spent confirming
# what the last run already said.
#
#   CELLS="injured:compliant:untracked novice:compliant:untracked" \
#     OUT=.dev/probe ./scripts/simulate-matrix.sh
override="${CELLS:-}"
CELLS=(
  # the ability axis, with a compliant athlete
  improver:compliant:untracked
  plateauer:compliant:untracked
  badweek:compliant:untracked
  novice:compliant:untracked
  strong:compliant:untracked
  injured:compliant:untracked
  # the behaviour axis, with an improving athlete
  improver:skipper:untracked
  improver:partial:untracked
  improver:overachiever:untracked
  improver:improviser:untracked
  improver:layoff:untracked
  # readiness, and the crosses worth their run time
  improver:compliant:roughweek
  novice:skipper:untracked
  injured:compliant:roughweek
  strong:overachiever:untracked
)
if [ -n "$override" ]; then
  # Word-split on purpose: the override is a space-separated list of the same
  # "athlete:behaviour:recovery" triples.
  # shellcheck disable=SC2206
  CELLS=($override)
fi

mkdir -p "$OUT"

echo "Loading $DUMP into dev DB (127.0.0.1:${PORT}) ..." >&2
mariadb -h127.0.0.1 -P"$PORT" -ucoach -pcoach coach <"$DUMP"

for cell in "${CELLS[@]}"; do
  IFS=: read -r athlete behaviour recovery <<<"$cell"
  name="$athlete-$behaviour-$recovery"
  echo "  $name ..." >&2
  DATABASE_URL="$URL" \
  SIM_ATHLETE="$athlete" SIM_BEHAVIOUR="$behaviour" SIM_RECOVERY="$recovery" \
    nix develop "$ROOT" --command cargo run --quiet --bin simulate \
    >"$OUT/$name.txt" 2>"$OUT/$name.err" || {
      echo "    FAILED — see $OUT/$name.err" >&2
      continue
    }
done

echo >&2
echo "Traces in $OUT. Summary lines:" >&2
grep -h '^# summary' "$OUT"/*.txt | sed 's/^/  /' >&2
