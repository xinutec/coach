{-
coach/gate.dhall — this repository's commit gate.

The database is acquired, used and released by `with-test-db`, which the rows
that need one invoke: one row, one command, no setup/teardown vocabulary. The
server is ephemeral, and `tests/db.rs` creates and drops its own `coach_test_*`
databases, so no development state leaks into the suite.

Shared vocabulary (`inDevShell`, `ngBuild`, `devLint`, …) lives in dev-lint's
schema as `G.` values. dev-lint is pinned to its committed HEAD, so a
neighbour's half-finished edit cannot fail this gate.

The generated `gate.json` is committed; `the table matches its Dhall` re-renders
and diffs it, so running the gate needs no `dhall`.
-}

let G = ../dev-lint/gate/schema.dhall

in  { name = "coach"
    , checks =
      [ G.Check::{
        , name = "formatting"
        , argv = G.inDevShell [ "cargo", "fmt", "--all", "--check" ]
        , timeout_s = 180
        }
      , G.Check::{
        , name = "clippy"
        , {-  `--workspace` so the pacing core is linted as a crate rather than
              skipped as a dependency: its totality rules (no unwrap / index /
              panic — see coach-pacing/src/lib.rs) are crate-level `deny`s, and
              clippy does not lint dependencies. Without this they would be
              decorative.
          -}
          argv =
            G.inDevShell
              [ "cargo"
              , "clippy"
              , "--workspace"
              , "--all-targets"
              , "--"
              , "-D"
              , "warnings"
              ]
        , {-  Clippy gets its own target directory: clippy-driver and rustc
              fingerprint the workspace differently and evict each other in a
              shared one, forcing a full recompile.
          -}
          env =
            G.clippyTarget
        , timeout_s = 1800
        }
      , G.cargoDoc
      , {-  The pacing core must compile #![no_std]. This is the purity guarantee
            made legible: with std out of scope, a std::fs / SystemTime::now() /
            thread::spawn / global mutable state in coach-pacing is not a lint to
            be waived — it fails to compile. A normal coach build already links
            the core no_std, so this can only fail if that guarantee broke; the
            named row says why it matters. The `ts` feature, which pulls std for
            the ts-rs type-gen, is off here on purpose.
        -}
        G.Check::{
        , name = "the pacing core still compiles no_std"
        , argv = G.inDevShell [ "cargo", "build", "-p", "coach-pacing" ]
        , timeout_s = 900
        }
      , {-  The whole suite, including tests/db.rs, which runs real SQL against a
            real MariaDB: a query that drifts from its `FromRow` struct compiles,
            passes every pure test, and 500s in production.

            `--grant-all` because tests/db.rs creates and drops its own
            `coach_test_<name>` database per test, which needs rights beyond
            `coach.*`. Port 3319: fleetwatch's ephemeral server takes 3317 and
            messages' 3318, and the fleet gate can run all three at once.
        -}
        G.Check::{
        , name = "tests (against a real MariaDB)"
        , argv =
            G.withTestDb
              "../"
              [ "--database"
              , "coach"
              , "--user"
              , "coach"
              , "--password"
              , "coach"
              , "--port"
              , "3319"
              , "--url-env"
              , "COACH_TEST_DATABASE_URL"
              , "--grant-all"
              , "--"
              , "cargo"
              , "test"
              ]
        , timeout_s = 1800
        }
      , {-  The gate builds from the working tree, where every file exists; the
            image gets what its COPY lines name and nothing else. A file the
            build needs but no COPY mentions passes here and fails in CI.

            So compile from a tree assembled out of the Dockerfile's own COPY
            lines, with no database, the way the image does.
        -}
        G.Check::{
        , name = "the backend builds from the image's file set"
        , argv = G.inDevShell [ "scripts/check-image-sources.sh" ]
        , timeout_s = 900
        }
      , {-  Query-cache drift. The .sqlx cache is what lets a checked query
            compile with no database, in CI and in the nix sandbox — but it is a
            snapshot, and a migration can age it while every query still matches
            its own cache key and builds green. This applies the migrations to an
            ephemeral server and asks sqlx whether the cache still describes it.

            Port 3320: the test row takes 3319, and the two can run at once.
        -}
        G.Check::{
        , name = "the query cache matches the schema"
        , argv =
            G.withTestDb
              "../"
              [ "--database"
              , "coach"
              , "--user"
              , "coach"
              , "--password"
              , "coach"
              , "--port"
              , "3320"
              , "--url-env"
              , "DATABASE_URL"
              , "--"
              , "scripts/check-query-cache.sh"
              ]
        , timeout_s = 900
        }
      , {-  Generated-types drift: regenerate the ts-rs bindings and fail if the
            committed frontend output moved. Catches a Rust API-type edit that
            was not regenerated and committed.

            Through `scripts/gen-types.sh`, so generating and checking share
            one statement of the output directory and the cargo invocation.
        -}
        G.Check::{
        , name = "generated types are current"
        , argv = G.inDevShell [ "scripts/gen-types.sh", "--check" ]
        , timeout_s = 900
        }
      , {-  `--frozen-lockfile` is pnpm ci: install exactly pnpm-lock.yaml, or
            fail. The gate has to run from a clean checkout — a fresh clone, or
            the tree the fleetwatch collector runs in — not just a warm dev
            machine.
        -}
        G.Check::{
        , name = "frontend deps match the lockfile"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "install", "--frozen-lockfile" ]
        , env = G.nonInteractive
        , timeout_s = 900
        }
      , G.Check::{
        , name = "frontend lint"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "lint" ]
        , env = G.nonInteractive
        , timeout_s = 900
        }
      , {-  The Playwright specs, type-checked. Nothing else reads them:
            Playwright transforms them with esbuild, which strips types rather
            than checking them, and `tsconfig.app.json` reaches only what
            `src/main.ts` imports. The layout harness is the only gate that
            sees what a phone suffers; it should not be the least-checked code
            here.
        -}
        G.Check::{
        , name = "frontend e2e specs type-check"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "typecheck:e2e" ]
        , env = G.nonInteractive
        , timeout_s = 900
        }
      , {-  `../../dev-lint`, not `../dev-lint`: cwd is `coach/frontend`.
        -}
        G.Check::{
        , name = "frontend build"
        , cwd = "frontend"
        , argv =
            G.ngBuild
              "../../"
              [ "dist/coach-web/browser" ]
              [ "pnpm", "exec", "ng", "build" ]
        , env = G.nonInteractive
        , timeout_s = 1800
        }
      , G.Check::{
        , name = "frontend unit tests"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "test" ]
        , env = G.nonInteractive # G.oneAngularWorker
        , timeout_s = 1800
        }
      , {-  The L2 phone-width layout harness: it serves the freshly-built dist
            and asserts no overlap or overflow at Pixel width.

            It reads the dist the build row above wrote, so it must run after
            it. Exactly one row writes `dist/`, which is what makes the mtime
            rule in `ng-build` mean something.
        -}
        G.Check::{
        , name = "frontend ui-check (phone-width layout harness)"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "ui-check" ]
        , {-  Playwright DELETES this at the start of every run, so the run made
              to investigate a failure is the run that erases it — and no option
              turns that off (`preserveOutput` is about PASSING tests). Declaring
              it here makes the gate copy it aside when this check fails.
          -}
          artifacts = [ "test-results" ]
        , env = G.nonInteractive
        , timeout_s = 1800
        }
      , {-  The Android app. Toolchain comes from recall's android dev shell,
            the same one android/deploy.sh uses; a missing shell FAILS this row
            rather than skipping it, because a gate that skips is a gate that
            lies. The build additionally needs ui-harness — the shared WebView
            shell — checked out beside the repo; app/build.gradle.kts says so in
            a sentence when it isn't.

            `assembleDebug` as well as the tests: MainActivity and the receivers
            carry no unit tests, so packaging the APK is what proves they still
            build.

            Not `-q`: at quiet level a failure reports "1 failed" and an HTML
            report path and never names the test, which is the one thing you want
            from a gate that has just gone red. The hook prints nothing unless
            the run fails, so the cost is noise on a hand-run.

            Two rows rather than gradle's one invocation, so a failing unit test
            and a failing package are named separately.
        -}
        G.Check::{
        , name = "android :app assembleDebug"
        , cwd = "android"
        , argv =
            [ "nix"
            , "develop"
            , "git+file:../../recall?ref=HEAD#android"
            , "--command"
            , "./gradlew"
            , "--console=plain"
            , ":app:assembleDebug"
            ]
        , timeout_s = 1800
        }
      , G.Check::{
        , name = "android :app unit tests"
        , cwd = "android"
        , argv =
            [ "nix"
            , "develop"
            , "git+file:../../recall?ref=HEAD#android"
            , "--command"
            , "./gradlew"
            , "--console=plain"
            , ":app:testDebugUnitTest"
            ]
        , timeout_s = 1800
        }
      , G.checkTable "../dev-lint"
      , G.devLint "../"
      ]
    }
