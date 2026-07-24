#!/usr/bin/env bash
# coach verify — rust backend (fmt + clippy + tests) + angular frontend (build +
# unit tests) + shared rules.
#
# The backend tests include tests/db.rs, which runs the real SQL against a real
# MariaDB — the gate that was missing when a query drifted from its `FromRow`
# struct, compiled, passed every pure test, and 500'd in the gym on 82 of 119
# exercises. So this script *provides* the database rather than telling you to run
# those "separately", which in practice meant never.
set -euo pipefail
cd "$(dirname "$0")/.."

# A MariaDB for tests/db.rs. Reuse one that's already up (the dev DB on 3308, or
# whatever COACH_TEST_DATABASE_URL names); otherwise start a throwaway and stop it
# on the way out. `bash`'s /dev/tcp avoids depending on nc, which the CI container
# doesn't have.
db_up() { (exec 3<>/dev/tcp/127.0.0.1/3308) 2>/dev/null; }
if [ -z "${COACH_TEST_DATABASE_URL:-}" ] && ! db_up; then
  echo "verify: starting a throwaway MariaDB for the DB tests ..."
  ./scripts/dev-db.sh >.dev/verify-mysql.log 2>&1 &
  db_pid=$!
  trap 'kill "$db_pid" 2>/dev/null || true' EXIT
  for _ in $(seq 1 60); do
    db_up && break
    sleep 0.5
  done
  db_up || {
    echo "verify: MariaDB did not come up — see .dev/verify-mysql.log" >&2
    exit 1
  }
fi
nix develop -c bash -c '
  set -euo pipefail
  # @angular/build:application tears down its Piscina worker pool at process
  # exit; on macOS / Node 24 / libuv 1.52 that teardown intermittently aborts
  # the process — a libuv kqueue assertion ("errno == EINTR", uv__io_poll →
  # Abort 6) or "EBADF: bad file descriptor, close" — AFTER "bundle generation
  # complete", i.e. once a complete, valid bundle is already on disk.
  # NG_BUILD_MAX_WORKERS=1 lowers the rate (fewer worker pipes to race) but does
  # NOT eliminate it, so the build goes through frontend/scripts/ng-build.sh,
  # which treats "bundle complete, then abort in teardown" as success (the
  # artifact is valid) instead of failing the whole gate. Harmless on Linux/CI,
  # which build cleanly. NOT the sandbox.
  export NG_BUILD_MAX_WORKERS=1
  cargo fmt --all --check
  # Clippy gets its own target dir: clippy-driver and rustc fingerprint the
  # workspace differently and evict each other in a shared dir, forcing a full
  # recompile. A dedicated dir keeps both caches warm.
  CARGO_TARGET_DIR="${CARGO_CLIPPY_TARGET_DIR:-$HOME/.cache/cargo/clippy-target}" \
    cargo clippy --all-targets -- -D warnings
  # The pacing core must compile #![no_std]. This is the purity guarantee made
  # legible: with std out of scope, a std::fs / SystemTime::now() / thread::spawn
  # / global mutable state in coach-pacing is not a lint to be waived — it fails
  # to compile. (A normal coach build already links the core no_std, so this can
  # only fail if that guarantee broke; the named step says *why* it matters.) The
  # `ts` feature, which pulls std for the ts-rs type-gen, is off here on purpose.
  cargo build -p coach-pacing
  cargo test
  # Generated-types drift (formerly the separate pre-push gate): regenerate the
  # ts-rs bindings and fail if the committed frontend generated output moved.
  scripts/check-types.sh
  # ui-check (L2 phone-width layout harness) runs after the build — it serves
  # the freshly-built dist via e2e/serve.mjs and asserts no overlap/overflow at
  # Pixel width. See @xinutec/ui-harness + dev-lint/docs/layout-quality-architecture.md.
  # Frontend deps must exist before lint/build. verify.sh has to run from a clean
  # checkout (a fresh clone, or the tree the fleetwatch collector runs in) — not
  # just a warm dev machine — so install them when absent or the lockfile moved.
  if [ ! -d frontend/node_modules ] || [ frontend/package-lock.json -nt frontend/node_modules ]; then
    ( cd frontend && npm ci )
  fi
  ( cd frontend && npm run lint && bash scripts/ng-build.sh && npm test && npm run ui-check )
'
dev_lint_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/dev-lint"
[ -d "$dev_lint_dir" ] || dev_lint_dir="$HOME/Code/dev-lint"
[ -d "$dev_lint_dir" ] || dev_lint_dir="$HOME/code/dev-lint"
nix run "$dev_lint_dir" -- . # dev-lint
