//! HTTP routing table.

pub mod api;
pub mod auth;
pub mod equipment;
pub mod exercises;
pub mod import;
pub mod locations;
pub mod muscles;
pub mod pacing;
pub mod places;
pub mod settings;
pub mod workout;

use axum::Router;
use axum::routing::{delete, get, patch, post};

use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

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
        // One-time migration import (history)
        .route("/import/nocodb", post(import::nocodb))
        // Micro-log
        .route("/sets", get(workout::list).post(workout::create))
        .route("/sets/{id}", delete(workout::delete))
        // Pacing settings + the live pacing verdict
        .route("/settings", get(settings::get).patch(settings::patch))
        .route("/pacing/now", get(pacing::now))
        // One INFO line per API request (method, path, status, latency). Scoped to
        // /api so static-asset serving and the k8s /healthz probe don't spam it.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    let mut app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
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
        let serve = ServeDir::new(&dir).fallback(ServeFile::new(format!("{dir}/index.html")));
        app = app.fallback_service(serve);
    }

    app.with_state(state)
}
