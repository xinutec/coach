//! The tests that run SQL against a **real MariaDB**, because nothing else did.
//!
//! Every other test in this suite is pure: the engine, the ability model, the load
//! maths. They are the reason the *thinking* is trustworthy, and they cannot catch
//! a single thing that goes wrong between the code and the database. So one
//! didn't: `EquipmentRow` grew a `loadable` field, one of the two SELECTs that
//! build it was updated and the other wasn't, and because a `FromRow` struct binds
//! its columns **by name at runtime**, it compiled, passed the whole suite, shipped
//! — and 500'd in production on every exercise that has any equipment, which is 82
//! of them. The bug was live in the gym.
//!
//! The fix at the time was to share the column list (`eq_cols!`). This is the
//! other half: a test that actually executes the queries. The rule it enforces is
//! blunt — *every read path runs against a migrated, seeded schema, and the whole
//! catalog goes through the one that broke.*
//!
//! Needs a database. `scripts/dev-db.sh` (127.0.0.1:3308) is the default; CI
//! supplies one via `COACH_TEST_DATABASE_URL`. It fails loudly when there isn't
//! one rather than skipping: a test that silently passes when it can't run is
//! worse than no test, because it reports the coverage it isn't providing.

use chrono::{Duration, Utc};
use sqlx::{AssertSqlSafe, MySqlPool};

use coach::exercise::repo as ex_repo;
use coach::location::types::{EquipmentOption, NewLocation};
use coach::pacing::service;
use coach::pacing::types::SuggestionKind;
use coach::settings::types::SettingsPatch;
use coach::workout::repo as workout_repo;
use coach::workout::types::NewSet;
use coach::{db, equipment, location, muscle, seed, settings};

const DEV_DB: &str = "mysql://coach:coach@127.0.0.1:3308/coach";

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
    for stmt in [
        format!("DROP DATABASE IF EXISTS `{db_name}`"),
        format!("CREATE DATABASE `{db_name}` CHARACTER SET utf8mb4"),
    ] {
        // A database name can't be a bind parameter, so this is the one query that
        // has to be built by hand. The assert above is the audit sqlx is asking for.
        sqlx::query(AssertSqlSafe(stmt.clone()))
            .execute(&admin)
            .await
            .unwrap_or_else(|e| panic!("{stmt}: {e}"));
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

// A database per test, not one shared pool. Sharing a `static` pool across tests
// looks like an easy win — seeding copies ~15 MB of image blobs — and is a trap:
// every `#[tokio::test]` builds its own runtime, a sqlx pool's keepalive tasks
// belong to the runtime that created it, and the first test to finish takes that
// runtime (and the pool's ability to hand out connections) down with it. The rest
// then fail on a pool timeout, which reads like a database problem and isn't.
//
// Tests run on parallel threads, so the seeds overlap: the cost is roughly one
// seed of wall-clock, not six.

/// **The regression test for the production 500.** Every exercise detail — the
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
    // The bug hit exactly the exercises that have equipment, so a run where none
    // do would pass while proving nothing.
    assert!(
        with_equipment >= 80,
        "only {with_equipment} exercises have equipment — the join isn't being exercised"
    );
}

/// Every other read path, executed once against the real schema. Cheap, and it
/// closes the same class of bug for the queries that didn't happen to break.
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
        !ex_repo::primary_muscles_by_exercise(pool)
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
                exercise_id: ex_id,
                reps: Some(8),
                load_kg: Some(10.0),
                hold_s: None,
                rpe: Some(8),
                note: None,
                logged_at: None,
            },
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
/// prescribable load. Before this, a pulley was a `machine`, `machine` wasn't a
/// free weight, and the coach could put no weight on the one machine whose entire
/// purpose is the weight on it — so its exercises were bodyweight reps forever.
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
                weights: (1..=18).map(|i| i as f64 * 5.0).collect(),
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
/// It didn't. The seed's hash gate watched `exercises.json` only, and its
/// reconcile wrote back four flags — so fixing a broken `demo_url` in the catalog
/// re-ran the seed and left the row exactly as broken, in a way that looked
/// entirely applied from the outside. This corrupts a row the way prod was
/// corrupted and asserts the next boot repairs it.
#[tokio::test]
async fn a_catalog_correction_reaches_an_already_seeded_row() {
    let pool = fresh("reconcile").await;

    let before = ex_repo::detail(&pool, first_id(&pool).await).await.unwrap();
    let before = before.expect("seeded exercise");
    let good_url = before.demo_url.clone().expect("catalog entry has a demo");

    // Break the row, exactly as prod's rows were broken: a stale value the catalog
    // has since corrected.
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

async fn first_id(pool: &MySqlPool) -> i64 {
    ex_repo::list(pool, false).await.unwrap()[0].id
}

/// A picture can arrive after the movement does — a movement is catalogued the
/// moment it's real, and someone photographs it later. The image seed used to be
/// gated on the exercise being *new*, so a picture added to an existing row had
/// nowhere to land: the seed skipped it, forever, however many images went into
/// the bundle.
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
    let aspect = decoded.width() as f64 / decoded.height() as f64;
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
