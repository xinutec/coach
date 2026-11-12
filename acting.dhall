{-
coach/acting.dhall — every operational script in this repository (`*.sh`,
`*.py`), classified, with a ceiling per class; a test needs no row when its path
says it is one. What the classes mean and when a ceiling may rise:
dev-lint/tables/acting_schema.dhall. The gate runs `dev-lint-fleet --acting .`:
a script with no row, a row for a file that is gone, or a class over its
ceiling fails the commit that causes it.
-}
let A = ../dev-lint/tables/acting_schema.dhall

let subject = A.subject

let Class = A.Class

in  { ceilings =
        -- acts: each row names what it becomes (automation-model items 13-15)
      { acts = 2
      , bootstrap = 0
      , by_design = 0
      , observes = 0
      , observer_checks = 0
      }
    , subjects =
      [ subject
              "android/deploy.sh"
              Class.Acts
              "builds the APK and installs it on a phone over adb: the phone-shells plan's target (automation-model item 14)"
          , subject "render/fetch-asset.sh" Class.OutOfScope ""
          , subject "render/floor.py" Class.OutOfScope ""
          , subject "render/pose.py" Class.OutOfScope ""
          , subject "render/prepare.py" Class.OutOfScope ""
          , subject "render/render.py" Class.OutOfScope ""
          , subject "render/rigging/gen-body.py" Class.OutOfScope ""
          , subject "render/rigging/transfer-rig.py" Class.OutOfScope ""
          , subject "render/skin/animate.py" Class.OutOfScope ""
          , subject "render/skin/check-pose.py" Class.OutOfScope ""
          , subject "render/skin/collide.py" Class.OutOfScope ""
          , subject "render/skin/debug-regions.py" Class.OutOfScope ""
          , subject "render/skin/label-body.py" Class.OutOfScope ""
          , subject "render/skin/plant.py" Class.OutOfScope ""
          , subject "render/skin/render-skin.py" Class.OutOfScope ""
          , subject "render/skin/try-pose.py" Class.OutOfScope ""
          , subject "render/stage.py" Class.OutOfScope ""
          , subject "render/za.py" Class.OutOfScope ""
          , subject "scripts/add-image.py" Class.OutOfScope ""
          , subject "scripts/backtest.sh" Class.OutOfScope ""
          , subject "scripts/check-image-sources.sh" Class.OutOfScope ""
          , subject "scripts/check-muscle-map.py" Class.OutOfScope ""
          , subject "scripts/check-query-cache.sh" Class.OutOfScope ""
          , subject "scripts/coachctl.py" Class.OutOfScope ""
          , subject
              "scripts/deploy.sh"
              Class.Acts
              "rolls coach on isis by hand: a deploy-plan row's target (item 15)"
          , subject "scripts/dev-db.sh" Class.OutOfScope ""
          , subject "scripts/gen-types.sh" Class.OutOfScope ""
          , subject
              "scripts/prod-dump.sh"
              Class.OutOfScope
              "dumps production's database into a local file; reads only"
          , subject "scripts/setup-hooks.sh" Class.OutOfScope ""
          , subject "scripts/sim-accuracy.py" Class.OutOfScope ""
          , subject "scripts/sim-neglect.py" Class.OutOfScope ""
          , subject "scripts/simulate-matrix.sh" Class.OutOfScope ""
          , subject "scripts/simulate.sh" Class.OutOfScope ""
      ]
    }
