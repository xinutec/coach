# coach

Personal exercise/training tracker with an **adaptive pacing coach**. A sibling
of `life`: Rust (axum) backend + Angular 22 frontend + its own MariaDB, served
from one image and deployed to k3s on isis. Public at `coach.xinutec.org`, gated
by Nextcloud OAuth login.

There's no stored plan or program. On every request the pacing engine recomputes
what to do from first principles: your logged set history (rolling muscle-group
volume + recovery), your settings, your biometric readiness, and the kit at your
current location — rings on a 2 m bar, adjustable weights, a mat. It picks the
biggest recovered deficit, chooses an exercise you can actually do there, and
progresses it off your last performance — then nudges you to spread your sets
through the day instead of cramming them at night. Reminders fire from the
Android app's on-device geofence (only when you're home).

## Layout

- `src/` — Rust backend (see module docs). `pacing/engine.rs` is the pure,
  unit-tested core; `pacing/service.rs` assembles its input + applies your tz.
- `docs/trainer.md` — the trainer model: design principles, known gaps, and the
  staged roadmap toward a full deterministic trainer (ability model, ordered
  daily plan, calibration tasks).
- `migrations/` — sqlx migrations, run at boot. Append-only.
- `frontend/` — Angular app (Today burn-down, log, history, balance, exercise
  library, locations, settings). A movement's picture and demo are one tap from
  the plan card: the demo plays in the sheet (muted, chrome-stripped, from the
  timestamp the catalog link points at) rather than throwing you out to YouTube
  mid-set, and fills the screen if you turn the phone sideways.
- `android/` — WebView wrapper + native geofence/notification layer (WIP).

## Develop

```sh
nix develop                 # cargo + node toolchain
./scripts/dev-db.sh         # local MariaDB on :3308 (db/user: coach/coach)
cp .env.example .env        # fill in; DEV_LOGIN_USER bypasses Nextcloud locally
cargo run                   # API on :8080 (STATIC_DIR unset = API only)
# frontend: cd frontend && npm install && npm start   # ng serve :4200, proxies /api

# to serve the built SPA from the backend (single origin):
#   (cd frontend && NG_BUILD_MAX_WORKERS=1 npm run build)   # the =1 avoids a macOS
#   STATIC_DIR=frontend/dist/coach-web/browser cargo run    # build-teardown abort
```

`gen-types.sh` regenerates the frontend TS types from the Rust API types;
`check-types.sh` is the drift gate. `verify.sh` is the gate to run before
pushing: backend fmt + clippy, frontend lint/build/unit tests, the type-drift
check, the Playwright layout checks, and dev-lint. It diffs the worktree against
the git *index*, so `git add -A` first or the drift gate reads a stale tree.

## Deploy

```sh
./scripts/deploy.sh          # commit + push first; it refuses a dirty tree
```

CI (`.github/workflows/build.yml`, on push to `main`) builds+pushes
`xinutec/coach:latest`, tagging the image with the commit it was built from.
`deploy.sh` waits for the CI run **whose head SHA is this commit** (not merely
the latest run — that returns the *previous* commit's, and restarting on it
ships the code before yours while reporting success), rolls out, then asks the
running server `GET /version` and requires it to equal HEAD. A rollout that
succeeds proves a pod came up, not which image it came up on; `/version` is what
proves the deploy.

The k8s manifests live in the home monorepo (`xinutec/pippijn`
`code/kubes/coach/k8s/`). First time only, from that checkout, on isis as root:

```sh
# NC OAuth2 client "coach" (dash admin), redirect
#   https://coach.xinutec.org/auth/callback
NC_CLIENT_ID=... NC_CLIENT_SECRET=... ./k8s/secret.sh
./k8s/sync.sh
```

DNS: `code/dns` CNAME `coach → isis.xinutec.org` (`tofu apply` from isis).
