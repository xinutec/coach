//! HTTP routing table.

pub mod api;
pub mod auth;
pub mod equipment;
pub mod exercises;
pub mod locations;
pub mod muscles;
pub mod pacing;
pub mod places;
pub mod settings;
pub mod telemetry;
pub mod workout;

use axum::Router;
use axum::http::{HeaderValue, Response, header};
use axum::routing::{delete, get, patch, post};

use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::services::fs::ServeFileSystemResponseBody;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

/// How long a static response may be reused. ⚠ **`index.html` must revalidate**
/// (`no-cache`: ask first, then a 304 via the ETag): without `Cache-Control` a WebView
/// caches heuristically and runs a build several deploys old against the new API, which
/// reads as "the change did not deploy". Everything else Angular emits has a content
/// hash in its name, so it is `immutable`.
fn cache_control_for(res: &Response<ServeFileSystemResponseBody>) -> Option<HeaderValue> {
    let is_html = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));
    Some(if is_html {
        HeaderValue::from_static("no-cache")
    } else {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    })
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
        // Exercise catalog
        .route("/exercises", get(exercises::list).post(exercises::create))
        .route(
            "/exercises/{id}",
            get(exercises::detail).patch(exercises::patch),
        )
        .route("/exercises/{id}/image", get(exercises::image))
        .route("/exercises/{id}/loop", get(exercises::demo_loop))
        // Reference catalogs
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
        // Micro-log
        .route("/sets", get(workout::list).post(workout::create))
        .route("/sets/{id}", delete(workout::delete))
        // Pacing settings + the live pacing verdict
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

    // The commit this binary was built from, baked in at image-build time (the
    // Dockerfile passes CI's GIT_SHA). Public and unauthenticated so a deploy can
    // *prove* the running pod contains the commit it just pushed, rather than
    // inferring it from "the rollout succeeded" — which only says a pod came up,
    // not which image it came up on. `dev` for a local build.
    let version = state.cfg.git_sha.clone();
    let mut app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/version", get(move || async move { version }))
        .route("/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/logout", post(auth::logout))
        .nest("/api", api);

    // DEV ONLY: mount /dev-login only when DEV_LOGIN_USER is set.
    if state.cfg.dev_login_user.is_some() {
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
            .layer(SetResponseHeaderLayer::overriding(
                header::CACHE_CONTROL,
                cache_control_for,
            ))
            .service(serve);
        app = app.fallback_service(serve);
    }

    app.with_state(state)
}
