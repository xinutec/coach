//! The tests that run SQL against a **real MariaDB**. A `FromRow` struct binds its
//! columns by name at runtime, so a SELECT that drifts from it compiles, passes every
//! pure test, and 500s in production. So every read path runs against a migrated,
//! seeded schema, and the whole catalog goes through the joins most likely to drift.
//!
//! Needs a database: `scripts/dev-db.sh` (127.0.0.1:3308) by default,
//! `COACH_TEST_DATABASE_URL` in CI. It fails loudly without one rather than skipping,
//! since a skipped test reports coverage it isn't providing.

use chrono::{Duration, Utc};
use sqlx::{AssertSqlSafe, MySqlPool};

use coach::exercise::repo as ex_repo;
use coach::exercise::types::Metric;
use coach::location::types::{EquipmentOption, NewLocation};
use coach::pacing::service;
use coach::pacing::types::SuggestionKind;
use coach::settings::types::SettingsPatch;
use coach::workout::repo as workout_repo;
use coach::workout::types::NewSet;
use coach::{db, equipment, location, muscle, seed, settings};

const DEV_DB: &str = "mysql://coach:coach@127.0.0.1:3308/coach";
/// How many times to try creating the scratch database, and how long to wait
/// between attempts — see the note in [`fresh`]. Generous enough to ride out an
/// InnoDB teardown on a loaded machine, short enough that a genuine failure still
/// reports in well under a second.
const CREATE_ATTEMPTS: u32 = 5;
const CREATE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(150);

fn catalog_dir() -> String {
    format!("{}/data/catalog", env!("CARGO_MANIFEST_DIR"))
}

/// The server this test suite is allowed to create scratch databases on.
fn base_url() -> String {
    std::env::var("COACH_TEST_DATABASE_URL").unwrap_or_else(|_| DEV_DB.to_string())
}

/// A migrated, catalog-seeded scratch database of its own.
///
/// Dropped and recreated by name rather than randomly named, so a failed run
/// leaves exactly one database behind to look inside, and the next run starts
/// clean regardless.
async fn fresh(name: &str) -> MySqlPool {
    let base = base_url();
    let admin = MySqlPool::connect(&base).await.unwrap_or_else(|e| {
        panic!(
            "these tests need a MariaDB. Run ./scripts/dev-db.sh, or point \
             COACH_TEST_DATABASE_URL at one.\n  tried: {base}\n  {e}"
        )
    });

    let db_name = format!("coach_test_{name}");
    // The name is ours, not user input — but interpolating into DDL is still the
    // one place SQL can't be parameterised, so keep it to what we generate.
    assert!(
        db_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "scratch db name must be a bare identifier: {db_name}"
    );
    // A database name can't be a bind parameter, so these are the one query that has
    // to be built by hand. The assert above is the audit sqlx is asking for.
    let run = async |stmt: String| {
        sqlx::query(AssertSqlSafe(stmt.clone()))
            .execute(&admin)
            .await
            .map_err(|e| format!("{stmt}: {e}"))
    };
    run(format!("DROP DATABASE IF EXISTS `{db_name}`"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));

    // Only the CREATE is retried. `DROP DATABASE` returns before InnoDB has removed the
    // directory, so under load the CREATE can meet the old directory and fail with
    // 1007; the suite passes on a rerun, the signature of a race, not a leftover. Not
    // `IF NOT EXISTS`, which would adopt a half-dropped database.
    let create = format!("CREATE DATABASE `{db_name}` CHARACTER SET utf8mb4");
    for attempt in 1..=CREATE_ATTEMPTS {
        match run(create.clone()).await {
            Ok(_) => break,
            Err(e) => {
                assert!(
                    attempt < CREATE_ATTEMPTS,
                    "{e} (after {CREATE_ATTEMPTS} attempts)"
                );
                tokio::time::sleep(CREATE_RETRY_DELAY).await;
            }
        }
    }
    admin.close().await;

    let url = match base.rsplit_once('/') {
        Some((prefix, _)) => format!("{prefix}/{db_name}"),
        None => panic!("COACH_TEST_DATABASE_URL has no database component: {base}"),
    };
    let pool = db::connect(&url)
        .await
        .expect("connecting to the scratch db");
    db::migrate(&pool).await.expect("migrating");
    seed::run(&pool, &catalog_dir()).await.expect("seeding");
    pool
}

// A database per test, not one shared `static` pool: each `#[tokio::test]` has its own
// runtime, a pool's keepalive tasks belong to the runtime that made it, and the first
// test to finish would take the pool down with it. The seeds run in parallel, so this
// costs about one seed of wall-clock.

/// Every exercise detail — the
/// query that joins `exercise_equipment` to `equipment` and builds an
/// `EquipmentRow` — for the whole catalog, active and retired. A column list that
/// drifts from the struct fails here instead of in the gym.
#[tokio::test]
async fn every_exercise_detail_loads() {
    let pool = &fresh("detail").await;
    let all = ex_repo::list(pool, true).await.expect("listing exercises");
    assert!(
        all.len() >= 119,
        "catalog looks unseeded: {} rows",
        all.len()
    );

    let mut with_equipment = 0;
    for ex in &all {
        let detail = ex_repo::detail(pool, ex.id)
            .await
            .unwrap_or_else(|e| panic!("detail({}) — {} — failed: {e}", ex.id, ex.slug))
            .unwrap_or_else(|| panic!("detail({}) — {} — vanished", ex.id, ex.slug));
        if !detail.equipment.is_empty() {
            with_equipment += 1;
        }
    }
    // The equipment join is the one that drifts, so a run where no exercise has
    // equipment would pass while proving nothing.
    assert!(
        with_equipment >= 80,
        "only {with_equipment} exercises have equipment — the join isn't being exercised"
    );
}

/// The three list-shaped queries must describe the same exercise. A checked query takes
/// only a string literal, so the column list is written three times; the shared
/// `ExerciseListRow` catches a column added to one copy, but not an expression changed
/// under the same alias and type. So this compares the values, via Debug strings, which
/// also covers any field added later.
#[tokio::test]
async fn every_read_path_agrees() {
    let pool = &fresh("agree").await;
    let all = ex_repo::list(pool, true).await.expect("listing exercises");
    assert!(
        all.len() >= 119,
        "catalog looks unseeded: {} rows",
        all.len()
    );
    let active = ex_repo::list(pool, false).await.expect("listing active");

    let mut compared_active = 0;
    for ex in &all {
        let one = ex_repo::get(pool, ex.id)
            .await
            .unwrap_or_else(|e| panic!("get({}) — {} — failed: {e}", ex.id, ex.slug))
            .unwrap_or_else(|| panic!("get({}) — {} — vanished", ex.id, ex.slug));
        assert_eq!(
            format!("{ex:?}"),
            format!("{one:?}"),
            "list(all) and get() disagree about {}",
            ex.slug
        );
        if let Some(from_active) = active.iter().find(|a| a.id == ex.id) {
            assert_eq!(
                format!("{ex:?}"),
                format!("{from_active:?}"),
                "list(all) and list(active) disagree about {}",
                ex.slug
            );
            compared_active += 1;
        }
    }
    // A run where the active list came back empty would pass while comparing
    // nothing, which is the shape of guard this test exists to distrust.
    assert!(
        compared_active >= 100,
        "only {compared_active} exercises were on the active path"
    );
}

/// Every other read path, executed once against the real schema. Cheap, and it
/// closes the same class of drift for every query.
#[tokio::test]
async fn every_read_path_runs() {
    let pool = &fresh("read").await;
    let u = "test-read";

    assert!(!equipment::repo::list(pool).await.unwrap().is_empty());
    assert!(!muscle::repo::list(pool).await.unwrap().is_empty());
    assert!(!muscle::repo::groups(pool).await.unwrap().is_empty());
    assert!(!ex_repo::list(pool, false).await.unwrap().is_empty());
    assert!(
        !ex_repo::equipment_by_exercise(pool)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !ex_repo::muscle_groups_by_exercise(pool)
            .await
            .unwrap()
            .is_empty()
    );

    // Per-user paths: empty is the correct answer for a fresh user, so these
    // assert they *run*, not what they return.
    settings::repo::get(pool, u).await.unwrap();
    workout_repo::list_recent(pool, u, 10).await.unwrap();
    workout_repo::list_since(pool, u, Utc::now().naive_utc() - Duration::weeks(4))
        .await
        .unwrap();
    location::repo::list(pool, u).await.unwrap();
    location::repo::by_health_place(pool, u, 1).await.unwrap();

    // Every seeded exercise carries an image blob; fetching one exercises the
    // blob path (content type + etag) that the sheet's picture depends on.
    let first = ex_repo::list(pool, false).await.unwrap()[0].id;
    let img = coach::exercise::image::get(pool, first).await.unwrap();
    assert!(img.is_some(), "exercise {first} seeded without an image");
}

/// The verdict, computed the way production computes it: a real user, a real
/// location with real kit, real logged sets — through `service::now`, which is
/// the one call that touches nearly every SELECT in the codebase at once.
#[tokio::test]
async fn a_verdict_is_computed_from_a_real_location_and_real_history() {
    let pool = &fresh("verdict").await;
    let u = "test-verdict";

    settings::repo::upsert(
        pool,
        u,
        &SettingsPatch {
            timezone: Some("Europe/London".into()),
            window_start_hour: None,
            window_end_hour: None,
            min_rest_min: None,
            mode: None,
            days_per_week: None,
            emphasis: None,
        },
    )
    .await
    .unwrap();

    // A gym: fixed dumbbells and a pull-up bar. Weights registered, so the coach
    // has an honest load to prescribe.
    let loc = location::repo::create(
        pool,
        u,
        &NewLocation {
            name: "Test gym".into(),
            is_default: true,
            equipment: vec!["dumbbell".into(), "pull_up_bar".into(), "bench".into()],
            equipment_options: vec![EquipmentOption {
                slug: "dumbbell".into(),
                weights: vec![6.0, 8.0, 10.0, 12.0, 16.0, 20.0],
                ..Default::default()
            }],
            plates: vec![],
            health_place_id: None,
        },
    )
    .await
    .unwrap();

    let verdict = service::now(pool, u, Some(loc.id), None, Default::default())
        .await
        .unwrap();
    assert!(
        !verdict.plan.is_empty(),
        "a stocked location with no history should still yield a plan (all calibration): {}",
        verdict.reason
    );
    // No history at all → the engine cannot know what he lifts, so every training
    // item must be a measurement. This is the safety rule (G3) observed end to end
    // through the database, not just in the pure engine's unit tests.
    let work: Vec<_> = verdict
        .plan
        .iter()
        .filter(|s| s.kind != SuggestionKind::Warmup)
        .collect();
    assert!(!work.is_empty(), "plan is warm-up only");
    assert!(
        work.iter().all(|s| s.kind == SuggestionKind::Assess),
        "an athlete with no logged history was prescribed work instead of measured: {:?}",
        work.iter()
            .filter(|s| s.kind != SuggestionKind::Assess)
            .map(|s| &s.exercise_name)
            .collect::<Vec<_>>()
    );

    // Now log sets and confirm the history actually reaches the verdict.
    let ex_id = work[0].exercise_id;
    for _ in 0..3 {
        workout_repo::create(
            pool,
            u,
            &NewSet {
                exercise_id: ex_id.get(),
                reps: Some(8),
                load_kg: Some(10.0),
                hold_s: None,
                distance_m: None,
                rpe: Some(8),
                note: None,
                logged_at: None,
                confirm_load: None,
            }
            .validate(Metric::WeightedReps)
            .unwrap(),
        )
        .await
        .unwrap();
    }
    let after = service::now(pool, u, Some(loc.id), None, Default::default())
        .await
        .unwrap();
    assert!(
        after.day_done_sets >= 3,
        "logged sets didn't reach the verdict: {} done",
        after.day_done_sets
    );
    assert_eq!(
        workout_repo::list_recent(pool, u, 10).await.unwrap().len(),
        3
    );
}

/// The cable stack, end to end: kit whose load lives in the catalog's `weighted`
/// flag, through the seeder, the location's registered weights, and out as a
/// prescribable load. A pulley is a `machine`, not a free weight, and its whole
/// purpose is the weight on it.
#[tokio::test]
async fn a_cable_stack_carries_a_load() {
    let pool = &fresh("cable").await;
    let u = "test-cable";

    let loc = location::repo::create(
        pool,
        u,
        &NewLocation {
            name: "Cable gym".into(),
            is_default: true,
            equipment: vec!["cable_machine".into()],
            equipment_options: vec![EquipmentOption {
                slug: "cable_machine".into(),
                // A stack: pin positions, 5 kg apart.
                weights: (1..=18).map(|i| f64::from(i) * 5.0).collect(),
                ..Default::default()
            }],
            plates: vec![],
            health_place_id: None,
        },
    )
    .await
    .unwrap();

    let ctx = service::context(pool, u, Some(loc.id)).await.unwrap();
    let cable: Vec<_> = ctx
        .exercises
        .iter()
        .filter(|e| ctx.exercise_loads.contains_key(&e.id))
        .collect();
    assert!(
        !cable.is_empty(),
        "no cable movement is loadable at a gym with a registered stack"
    );
    for e in &cable {
        let loads = &ctx.exercise_loads[&e.id];
        assert!(
            loads.contains(&40.0),
            "{} can't be loaded to a weight on the stack: {loads:?}",
            e.name
        );
    }
    // And it reaches the athlete: the plan prescribes (well — measures) them.
    let verdict = service::now(pool, u, Some(loc.id), None, Default::default())
        .await
        .unwrap();
    assert!(!verdict.plan.is_empty(), "no plan at the cable gym");
}

/// A correction in the catalog must actually reach an already-seeded row.
///
/// A reconcile that skips a column the catalog owns re-runs the seed and leaves
/// the row as it was, in a way that looks entirely applied from the outside. This
/// corrupts a row and asserts the next boot repairs it.
#[tokio::test]
async fn a_catalog_correction_reaches_an_already_seeded_row() {
    let pool = fresh("reconcile").await;

    let before = ex_repo::detail(&pool, first_id(&pool).await).await.unwrap();
    let before = before.expect("seeded exercise");
    let good_url = before.demo_url.clone().expect("catalog entry has a demo");

    // Break the row: a stale value the catalog has since corrected.
    sqlx::query("UPDATE exercises SET demo_url = ?, cue = ? WHERE id = ?")
        .bind("https://youtube.be/wrong")
        .bind("stale cue")
        .bind(before.id)
        .execute(&pool)
        .await
        .unwrap();
    // ...and force the gate to re-evaluate, as a catalog edit would.
    sqlx::query("UPDATE catalog_state SET catalog_hash = 'stale' WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();

    seed::run(&pool, &catalog_dir()).await.expect("re-seeding");

    let after = ex_repo::detail(&pool, before.id)
        .await
        .unwrap()
        .expect("exercise survived the reseed");
    assert_eq!(
        after.demo_url.as_deref(),
        Some(good_url.as_str()),
        "the catalog's demo link did not reach the existing row — the reconcile is \
         skipping a column the catalog owns"
    );
    assert_eq!(
        after.cue, before.cue,
        "the catalog's cue did not reach the row"
    );
}

/// An unchanged catalog must not re-seed — the gate is what keeps normal boots
/// fast, and a gate that never fires is a gate that isn't there.
#[tokio::test]
async fn an_unchanged_catalog_short_circuits_the_seed() {
    let pool = fresh("gate").await;
    let id = first_id(&pool).await;

    sqlx::query("UPDATE exercises SET cue = 'untouched-by-a-noop-seed' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    seed::run(&pool, &catalog_dir()).await.unwrap();

    let after = ex_repo::detail(&pool, id).await.unwrap().unwrap();
    assert_eq!(
        after.cue.as_deref(),
        Some("untouched-by-a-noop-seed"),
        "the seed ran even though the catalog is unchanged"
    );
}

/// A rendered still is a CC BY-SA derivative, so it carries its credit — from the
/// catalog, like every other scalar the catalog owns, and so a corrected credit
/// reaches a row that is already seeded. A photograph with none carries none.
#[tokio::test]
async fn a_rendered_still_carries_its_credit_and_a_photograph_none() {
    let pool = fresh("credit").await;
    let all = ex_repo::list(&pool, false).await.unwrap();
    let id_of = |slug: &str| all.iter().find(|e| e.slug == slug).expect(slug).id;
    let render = id_of("heel_toe_rocks");

    // A stale credit, and a gate that has to look again, as a catalog edit forces.
    sqlx::query("UPDATE exercises SET image_credit = NULL, image_credit_url = NULL WHERE id = ?")
        .bind(render)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog_state SET catalog_hash = 'stale' WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    seed::run(&pool, &catalog_dir()).await.expect("re-seeding");

    let credit = ex_repo::detail(&pool, render)
        .await
        .unwrap()
        .unwrap()
        .image_credit
        .expect("the render's credit did not reach the row");
    assert!(credit.text.contains("Z-Anatomy"), "{}", credit.text);
    assert!(credit.text.contains("CC BY-SA"), "{}", credit.text);
    assert_eq!(credit.url.as_deref(), Some("https://github.com/Z-Anatomy"));

    let photo = ex_repo::detail(&pool, id_of("rdl")).await.unwrap().unwrap();
    assert!(photo.image_credit.is_none());
}

async fn first_id(pool: &MySqlPool) -> i64 {
    ex_repo::list(pool, false).await.unwrap()[0].id
}

/// The shape rules hold even when nobody asks the parser: a path that writes this table
/// directly (an importer, a migration) has no parser. So the same rules are CHECK
/// constraints (migration 0026), and this test writes raw SQL to prove the database
/// refuses on its own. The metric-dependent half needs `exercises.metric`, which a
/// CHECK cannot read, so it remains the parser's.
#[tokio::test]
async fn the_database_refuses_a_set_no_parser_looked_at() {
    let pool = &fresh("shape").await;
    let ex = first_id(pool).await;
    let raw = |cols: &str, vals: &str| {
        let sql = format!(
            "INSERT INTO workout_sets (user_id, exercise_id, logged_at, {cols}) \
             VALUES ('test-shape', {ex}, NOW(), {vals})"
        );
        async move { sqlx::query(AssertSqlSafe(sql)).execute(pool).await }
    };

    // Numbers that describe nothing a human did. The 3 530-second carry is a
    // fat-fingered append the ability model would read as a demonstrated max
    // (R3-1).
    for (what, cols, vals) in [
        ("no reps at all", "reps", "0"),
        ("more reps than a set has", "reps", "101"),
        ("the fat-fingered carry", "hold_s", "3530"),
        ("a walk with the shopping", "distance_m", "501"),
        ("a weight nobody lifts", "reps, load_kg", "5, 301"),
        ("a weightless weight", "reps, load_kg", "5, 0"),
        ("an effort off the scale", "reps, rpe", "5, 11"),
        // Neither of these is a set: one records that something happened and
        // nothing about what, the other doesn't say which measurement it means.
        ("no measurement", "load_kg", "20"),
        ("two measurements", "reps, hold_s", "5, 30"),
    ] {
        assert!(
            raw(cols, vals).await.is_err(),
            "the schema accepted {what} ({cols} = {vals})"
        );
    }

    // And every honest shape still goes in — including the one that looks like a
    // mistake and isn't: a weighted movement logged with no weight is an
    // empty-bar technique set the athlete chose not to weigh, and refusing it
    // would lose a real set.
    for (what, cols, vals) in [
        ("bodyweight reps", "reps", "12"),
        ("an empty-bar set", "reps", "8"),
        ("weighted reps", "reps, load_kg", "5, 60"),
        ("a hold", "hold_s", "45"),
        ("a carry in metres", "distance_m, load_kg", "20, 24"),
    ] {
        raw(cols, vals)
            .await
            .unwrap_or_else(|e| panic!("the schema refused {what} ({cols} = {vals}): {e}"));
    }
}

/// A picture can arrive after the movement does — a movement is catalogued the
/// moment it's real, and someone photographs it later. The image seed must not
/// be gated on the exercise being *new*.
#[tokio::test]
async fn a_picture_added_later_reaches_an_existing_movement() {
    let pool = fresh("image").await;
    let id = first_id(&pool).await;

    // The movement has been in the catalog for months, and the picture is only now
    // taken: delete the blob and re-seed, which is that situation exactly.
    sqlx::query("DELETE FROM exercise_images WHERE exercise_id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog_state SET catalog_hash = 'stale' WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        coach::exercise::image::get(&pool, id)
            .await
            .unwrap()
            .is_none(),
        "the picture should be gone before the re-seed"
    );

    seed::run(&pool, &catalog_dir()).await.expect("re-seeding");

    assert!(
        coach::exercise::image::get(&pool, id)
            .await
            .unwrap()
            .is_some(),
        "the catalog's picture never reached the movement — the image seed is still \
         gated on the row being new"
    );
}

/// The catalog bundle is the **source** and keeps its alpha; what the app is served
/// is what the app can display. A transparent portrait diagram (dark line-art,
/// 241×338) would otherwise fail twice: invisible on a dark theme, and cropped by
/// the 16:9 hero to a band across the figure's stomach — losing the very muscle the
/// picture exists to show.
#[tokio::test]
async fn a_transparent_diagram_is_rendered_but_a_photograph_is_left_alone() {
    let pool = fresh("render").await;

    async fn by_slug(pool: &MySqlPool, slug: &str) -> coach::exercise::image::ImageBlob {
        let ex = ex_repo::list(pool, true)
            .await
            .unwrap()
            .into_iter()
            .find(|e| e.slug == slug)
            .unwrap_or_else(|| panic!("no exercise {slug}"));
        coach::exercise::image::get(pool, ex.id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{slug} has no image"))
    }

    // The diagram: served opaque, at the hero's shape.
    let img = by_slug(&pool, "curl_biceps_dumbbell_standing").await;
    let decoded = image::load_from_memory(&img.bytes).expect("decoding the served diagram");
    let aspect = f64::from(decoded.width()) / f64::from(decoded.height());
    assert!(
        (aspect - 16.0 / 9.0).abs() < 0.01,
        "the diagram is served at {}×{} — the 16:9 hero will crop it",
        decoded.width(),
        decoded.height()
    );
    assert!(
        !image::GenericImageView::pixels(&decoded).any(|(_, _, p)| p.0[3] < 255),
        "the served diagram is still transparent — it will vanish on a dark theme"
    );
    // ...while the source keeps the alpha it came with. That's the whole point.
    let src = std::fs::read(format!(
        "{}/images/curl_biceps_dumbbell_standing.png",
        catalog_dir()
    ))
    .expect("the source image");
    let src = image::load_from_memory(&src).expect("decoding the source");
    assert!(
        image::GenericImageView::pixels(&src).any(|(_, _, p)| p.0[3] < 255),
        "the source image lost its transparency — the bundle is the source, and a \
         flattened source cannot be un-flattened"
    );

    // A photograph that is already the right shape and opaque: stored byte-for-byte,
    // so a re-seed doesn't rewrite the whole bundle. (Not every "photo" qualifies —
    // rdl.png is a palette PNG *with* transparency, and is rendered like a diagram.
    // The rule is about the pixels, not about the filename.)
    let photo = by_slug(&pool, "ab_rollout_barbell").await;
    let raw = std::fs::read(format!("{}/images/ab_rollout_barbell.jpg", catalog_dir()))
        .expect("the source photo");
    assert_eq!(
        photo.bytes, raw,
        "an ordinary photograph was re-encoded — it needed nothing done to it"
    );
}

/// The correction loop through the database: a wrong number becomes the estimate, the
/// card names *that* set, and removing it re-derives the estimate. The only destructive
/// path here, and deleting the wrong row would be silent.
#[tokio::test]
async fn a_wrong_set_can_be_found_from_the_card_and_removed() {
    let pool = &fresh("correct_estimate").await;
    let u = "test-correct";

    settings::repo::upsert(
        pool,
        u,
        &SettingsPatch {
            timezone: Some("Europe/London".into()),
            window_start_hour: None,
            window_end_hour: None,
            min_rest_min: None,
            mode: None,
            days_per_week: None,
            emphasis: None,
        },
    )
    .await
    .unwrap();
    let loc = location::repo::create(
        pool,
        u,
        &NewLocation {
            name: "Test gym".into(),
            is_default: true,
            equipment: vec!["dumbbell".into(), "pull_up_bar".into(), "bench".into()],
            equipment_options: vec![EquipmentOption {
                slug: "dumbbell".into(),
                weights: vec![6.0, 8.0, 10.0, 12.0, 16.0, 20.0],
                ..Default::default()
            }],
            plates: vec![],
            health_place_id: None,
        },
    )
    .await
    .unwrap();

    // Find a weighted movement the engine will actually plan here.
    let first = service::now(pool, u, Some(loc.id), None, Default::default())
        .await
        .unwrap();
    let ex_id = first
        .plan
        .iter()
        .find(|s| s.kind != SuggestionKind::Warmup && s.ask.load_kg().is_some())
        .map(|s| s.exercise_id)
        .expect("expected a loadable movement in the plan");

    // Logged on past days on purpose: `evaluate` excludes the *current session's*
    // own sets (they are progress against the plan, not grounds to replan), and
    // the set that poisons an estimate is an old one anyway — which is the whole
    // reason the card has to be able to reach back to it.
    let log = |load: f64, reps: i32, days_ago: i64| {
        let ns = NewSet {
            exercise_id: ex_id.get(),
            reps: Some(reps),
            load_kg: Some(load),
            hold_s: None,
            distance_m: None,
            rpe: None,
            note: None,
            logged_at: Some(Utc::now().naive_utc() - Duration::days(days_ago)),
            confirm_load: Some(true), // the athlete confirmed it; that's the bug
        };
        let valid = ns.validate(Metric::WeightedReps).unwrap();
        async move { workout_repo::create(pool, u, &valid).await.unwrap() }
    };

    // Honest work across separate days, plus one fat-fingered set weeks back.
    // Far enough back that the rolling 7-day volume window no longer counts them
    // (so the group still needs work and the movement stays in the plan), but
    // well inside the 6-week confidence window, so the estimate is trusted.
    log(10.0, 8, 21).await;
    log(10.0, 8, 18).await;
    log(10.0, 8, 14).await;
    let bogus = log(100.0, 8, 16).await;

    // The card must name the offending set — by its row id, not a guess.
    let poisoned = service::now(pool, u, Some(loc.id), None, Default::default())
        .await
        .unwrap();
    let src = poisoned
        .plan
        .iter()
        .find(|s| s.exercise_id == ex_id && s.kind != SuggestionKind::Warmup)
        .and_then(|s| s.explanation.as_ref())
        .and_then(|e| e.estimate_from)
        .expect("the estimate must name the set it came from");
    assert_eq!(
        src.set_id.get(),
        bogus.id,
        "the card pointed at the wrong set — removing it would delete honest history"
    );
    assert_eq!(src.load_kg, Some(100.0));

    // Removing it is what the button does; the next verdict re-derives.
    assert!(
        workout_repo::soft_delete(pool, u, src.set_id.get())
            .await
            .unwrap(),
        "the set the card offered to remove could not be removed"
    );
    let fixed = service::now(pool, u, Some(loc.id), None, Default::default())
        .await
        .unwrap();
    let after = fixed
        .plan
        .iter()
        .find(|s| s.exercise_id == ex_id && s.kind != SuggestionKind::Warmup)
        .and_then(|s| s.explanation.as_ref())
        .and_then(|e| e.estimate_from);
    if let Some(a) = after {
        assert_ne!(
            a.set_id.get(),
            bogus.id,
            "the removed set still defines the estimate"
        );
        assert_eq!(
            a.load_kg,
            Some(10.0),
            "the estimate should fall back to honest work"
        );
    }
    // And the honest sets survived — a correction must not take history with it.
    assert_eq!(
        workout_repo::list_recent(pool, u, 10).await.unwrap().len(),
        3,
        "removing one set must leave the other three"
    );
}
