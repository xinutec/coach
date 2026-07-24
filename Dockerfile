# Multi-stage build for coach: Angular frontend + Rust backend, served from one
# image (the backend serves the bundle + API). Mirrors the fleet's image
# convention (xinutec/<app>:latest).

# --- frontend: build the Angular bundle ---
FROM node:24-alpine AS frontend
WORKDIR /fe
COPY frontend/package.json frontend/package-lock.json ./
# git: the shared layout harness is a git dependency (github:xinutec/ui-harness),
# so npm ci clones it — node:alpine ships no git.
RUN apk add --no-cache git ca-certificates && npm ci
COPY frontend/ .
# Stamp the build version into the bundle (see scripts/stamp-version.mjs). The
# build context has no .git, so the commit comes from GIT_SHA (passed by CI —
# .github/workflows/docker.yml); it defaults to 'dev' for a plain local build.
ARG GIT_SHA=dev
RUN GIT_SHA="$GIT_SHA" node scripts/stamp-version.mjs
RUN npx ng build --configuration production

# --- backend: build the Rust binary (deps cached in their own layer) ---
FROM rust:1-bookworm AS backend
WORKDIR /app
# Both manifests: the root is a workspace, so cargo won't even *load* it without
# every member's Cargo.toml present — the priming build below fails on a missing
# coach-pacing/Cargo.toml long before it compiles anything.
COPY Cargo.toml Cargo.lock ./
COPY coach-pacing/Cargo.toml coach-pacing/
# Prime the dependency cache with stub crates (one per workspace member), then
# build for real.
RUN mkdir -p src coach-pacing/src \
    && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && echo '' > coach-pacing/src/lib.rs \
    && cargo build --release && rm -rf src coach-pacing/src
COPY src/ src/
COPY coach-pacing/src/ coach-pacing/src/
COPY migrations/ migrations/
RUN touch src/main.rs src/lib.rs coach-pacing/src/lib.rs && cargo build --release

# --- runtime ---
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# The app only reads (binary, bundle) and never writes to disk, so everything
# stays root-owned; the process just must not run as root. uid/gid 65532 is the
# conventional "nonroot" id, matched by k8s/03-app.yaml.
RUN groupadd --gid 65532 coach \
    && useradd --uid 65532 --gid coach --no-create-home --shell /usr/sbin/nologin coach
WORKDIR /app
COPY --from=backend /app/target/release/coach /usr/local/bin/coach
COPY --from=frontend /fe/dist/coach-web/browser ./public
# The training-library seed bundle (exercises/muscles/equipment JSON + images),
# loaded into the DB at boot by the seeder. Read-only.
COPY data/ ./data
# The commit this image was built from, served at /version so a deploy can prove
# the running pod is the one it just pushed. Declared again here because an ARG
# only lives in the stage that declares it. Runtime env, not a compile-time stamp:
# baking it into the binary would bust the Rust build cache on every commit.
ARG GIT_SHA=dev
ENV GIT_SHA=$GIT_SHA \
    STATIC_DIR=/app/public \
    CATALOG_DIR=/app/data/catalog \
    BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
USER coach
CMD ["coach"]
