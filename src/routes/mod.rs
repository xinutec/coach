//! HTTP routing table.

pub mod api;
pub mod auth;
pub mod equipment;
pub mod exercises;
pub mod hurts;
pub mod locations;
pub mod muscles;
pub mod pacing;
pub mod places;
pub mod settings;
pub mod telemetry;
pub mod workout;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::routing::{delete, get, patch, post};

use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

/// A name carrying its content hash (`main-7GNM4EPX.js`, a font's `-ABCD1234`):
/// a new build is a new name, so it may be cached for ever.
fn is_content_hashed(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("");
    let Some((stem, _ext)) = name.rsplit_once('.') else {
        return false;
    };
    let Some((_, hash)) = stem.rsplit_once('-') else {
        return false;
    };
    hash.len() == 8
        && hash
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// How long a static response may be reused. A hashed file is `immutable`.
/// Everything else revalidates (`no-cache`: ask first, then a 304 via the ETag):
/// the page names the bundle, and an icon, the manifest and the service worker's
/// files keep their names across builds, so a year of `immutable` pins the old
/// one on every phone. Without `Cache-Control` a WebView caches heuristically and
/// runs a build several deploys old. A 404 is never `immutable`: the name would
/// not be asked for again.
async fn cache_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let hashed = is_content_hashed(req.uri().path());
    let mut res = next.run(req).await;
    let value = if hashed && res.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
}

/// Serve the app for a client-side route, and 404 anything naming a file (a dot in the
/// last segment). ⚠ A missing file must not get the page: a `200` of HTML for a font
/// renders broken icons with no error anywhere.
fn spa(index: &str, path: &str) -> axum::response::Response {
    use axum::response::IntoResponse as _;

    if path
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
    {
        return (axum::http::StatusCode::NOT_FOUND, "not found").into_response();
    }
    match std::fs::read_to_string(index) {
        Ok(page) => axum::response::Html(page).into_response(),
        Err(error) => {
            // STATIC_DIR set with no index is a misconfigured deployment, and
            // saying so beats serving an empty page that looks like the app.
            tracing::error!("the app's index could not be read: {error}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "no index").into_response()
        }
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/me", get(api::me))
        .route("/exercises", get(exercises::list).post(exercises::create))
        .route(
            "/exercises/{id}",
            get(exercises::detail).patch(exercises::patch),
        )
        .route("/exercises/{id}/image", get(exercises::image))
        .route("/exercises/{id}/loop", get(exercises::demo_loop))
        .route(
            "/exercises/{id}/hurts",
            post(hurts::report).delete(hurts::take_back),
        )
        .route("/equipment", get(equipment::list))
        .route("/muscles", get(muscles::list))
        // Training locations (equipment inventories you can be "at")
        .route("/locations", get(locations::list).post(locations::create))
        .route(
            "/locations/{id}",
            patch(locations::patch).delete(locations::delete),
        )
        // health-sync bridge: detected places (for linking) + current location
        .route("/places/detected", get(places::detected))
        .route("/location/current", get(places::current))
        .route("/sets", get(workout::list).post(workout::create))
        .route("/sets/{id}", delete(workout::delete))
        .route("/settings", get(settings::get).patch(settings::patch))
        .route("/pacing/now", get(pacing::now))
        // What the person did, folded into the same log as what the API saw.
        .route("/telemetry", post(telemetry::record))
        // One INFO line per API request (method, path, status, latency). Scoped to
        // /api so static-asset serving and the k8s /healthz probe don't spam it.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    // The commit this binary was built from (CI's GIT_SHA; `dev` locally). Public, so
    // a deploy can prove which image the pod runs: a finished rollout doesn't say.
    let version = state.cfg.git_sha.clone();
    let mut app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        // dev-lint: allow-unauthenticated-route the build sha, so a stale client can see it is stale
        .route("/version", get(move || async move { version }))
        .route("/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/logout", post(auth::logout))
        .nest("/api", api);

    // DEV ONLY: mount /dev-login only when DEV_LOGIN_USER is set.
    if state.cfg.dev_login_user.is_some() {
        // dev-lint: allow-unauthenticated-route dev only: mounted when DEV_LOGIN_USER is set, which no deployed manifest sets
        app = app.route("/dev-login", get(auth::dev_login));
    }

    // Serve the built Angular bundle (single origin), falling back to
    // index.html so client-side routes resolve. API-only when STATIC_DIR unset.
    if let Some(dir) = state.cfg.static_dir.clone() {
        let index = format!("{dir}/index.html");
        let serve = ServeDir::new(&dir).fallback(get(move |uri: axum::http::Uri| {
            let index = index.clone();
            async move { spa(&index, uri.path()) }
        }));
        // ⚠ The layer wraps only the STATIC service: an API response is neither
        // a document to revalidate nor an immutable asset, and giving JSON a
        // year-long `immutable` would be the same bug pointing the other way.
        let serve = ServiceBuilder::new()
            .layer(axum::middleware::from_fn(cache_headers))
            .service(serve);
        app = app.fallback_service(serve);
    }

    app.with_state(state)
}
