{-
coach/gate.dhall — this repository's commit gate.

Was `scripts/verify.sh`, and this is the conversion that decided the schema:
coach is the repository with a resource in its gate, and the question was
whether a table has to learn about lifetimes. It does not. The database is
acquired, used and released by `with-test-db`, which the `tests` row invokes —
one row, one command, no setup/teardown vocabulary in the table. That is the same
call the reconciler next door made about its drill, where seed→up→wait→verify→
teardown stayed one coarse effect rather than becoming four facts that reopen
each other.

Three things did not survive the move, each deliberately.

**The DB probe and its trap are gone.** The script poked 127.0.0.1:3308 with
bash's /dev/tcp, started `scripts/dev-db.sh` in the background if nothing
answered, polled sixty times, and killed it from a `trap … EXIT`. All of that is
`with-test-db` now, shared with fleetwatch and messages rather than written a
third time.

**And the database is ephemeral rather than the long-lived dev one.** The script
reused `.dev/` on :3308 when it was up, so the suite ran against whatever state a
development session had left there. `tests/db.rs` creates and drops its own
`coach_test_*` databases, so a fresh server costs it nothing and removes the
question entirely.

**The conditional pnpm install is gone**, for the reason gamepads' was: its own
comment justified it on correctness — "a node_modules left behind by npm still
has a working .bin, so verify would pass against packages the lockfile no longer
describes" — and running it unconditionally serves that better. Measured on
gamepads before cutting: an up-to-date `--frozen-lockfile` install is 455 ms.

**The `&&` chains are gone.** `pnpm run lint && ng-build && pnpm test &&
pnpm run ui-check` reported one name when four things could be wrong.

**The vocabulary moved into the schema.** `inDevShell`, the clippy target
directory, the Angular worker cap, and the `ng-build` / `dev-lint` /
`check-table` rows were defined here and in ten other tables identically — the
duplication the shared tools were built to remove, recreated one level up. They
are `G.` values now. Two consequences the rendered JSON shows: every dev-shell
row gains `--no-warn-dirty`, because a gate that prints "Git tree is dirty" on
every row of every run has trained everyone to ignore a warning; and dev-lint is
pinned to its committed HEAD rather than run out of its worktree, which is what
stops a neighbour's half-finished edit failing this gate for a reason no commit
anywhere explains.

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
            real MariaDB — the gate that was missing when a query drifted from
            its `FromRow` struct, compiled, passed every pure test, and 500'd in
            the gym on 82 of 119 exercises.

            `--grant-all` because tests/db.rs creates and drops its own
            `coach_test_<name>` database per test, which needs rights beyond
            `coach.*`. Port 3319: fleetwatch's ephemeral server takes 3317 and
            messages' 3318, and the fleet gate can run all three at once.
        -}
        G.Check::{
        , name = "tests (against a real MariaDB)"
        , argv =
              G.inDevShell
                [ "nix", "run", "../dev-lint#with-test-db", "--" ]
            # [ "--database"
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
      , {-  Generated-types drift: regenerate the ts-rs bindings and fail if the
            committed frontend output moved. Catches a Rust API-type edit that
            was not regenerated and committed.
        -}
        G.Check::{
        , name = "generated types are current"
        , argv = G.inDevShell [ "scripts/check-types.sh" ]
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
        , timeout_s = 900
        }
      , G.Check::{
        , name = "frontend lint"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "lint" ]
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
        , timeout_s = 1800
        }
      , G.Check::{
        , name = "frontend unit tests"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "test" ]
        , env = G.oneAngularWorker
        , timeout_s = 1800
        }
      , {-  The L2 phone-width layout harness: it serves the freshly-built dist
            and asserts no overlap or overflow at Pixel width.

            Placement is no longer only presentation. `pnpm run ui-check` used to
            begin with a build of its own, so the gate built the frontend twice —
            once in the row above and once here — and this row's ordering did not
            matter. The build is now the row above and nothing else, so this one
            genuinely reads what that one wrote. That is how the rest of the
            fleet does it (memview, recall), and it is what makes the mtime rule
            in `ng-build` mean something: exactly one thing in the gate writes
            `dist/`, and it has to prove it did.
        -}
        G.Check::{
        , name = "frontend ui-check (phone-width layout harness)"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "ui-check" ]
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
            , "../../recall#android"
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
            , "../../recall#android"
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
