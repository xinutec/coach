//! Runtime configuration, read from the environment at startup.
//!
//! Secrets (session secret, NC OAuth client) come from the environment so
//! they can be supplied as k8s secrets in deployment and a `.env`-style shell
//! locally. Nothing here is hard-coded.

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    /// MariaDB connection string, e.g. `mysql://coach:pw@host/coach`.
    pub database_url: String,
    /// HMAC key for signing session cookies.
    pub session_secret: String,
    /// Address to bind the HTTP server to.
    pub bind_addr: String,

    /// Base URL of the Nextcloud instance, no trailing slash.
    pub nc_base_url: String,
    /// OAuth2 client registered in NC admin (identity flow).
    pub nc_client_id: String,
    pub nc_client_secret: String,
    /// Must match the redirect URI registered for the OAuth2 client.
    pub nc_redirect_uri: String,

    /// Directory of the built Angular bundle to serve (with SPA fallback). When
    /// unset the server is API-only — e.g. in dev, where `ng serve` proxies.
    pub static_dir: Option<String>,

    /// Directory of the training-library seed bundle (exercises/muscles/
    /// equipment/images), loaded into the DB at boot. Default `data/catalog`
    /// (repo-relative locally; `/app/data/catalog` in the container).
    pub catalog_dir: String,

    /// DEV ONLY. When set, `/dev-login` mints a session for this user id
    /// without Nextcloud. Absent in production → the route 404s. Never set this
    /// in a deployed environment.
    pub dev_login_user: Option<String>,

    /// health-sync integration (optional): the in-cluster base URL of health's
    /// internal API (e.g. `http://health-auth.health.svc.cluster.local:3000`)
    /// and the shared `X-Service-Token`. Both must be set for location
    /// auto-detection; absent → the feature is simply off (manual selection).
    pub health_internal_url: Option<String>,
    pub health_service_token: Option<String>,
    /// The commit this image was built from (CI passes it to the Dockerfile, which
    /// puts it in the environment). Served at `/version` so a deploy can prove the
    /// running pod is the commit it pushed. `dev` for a local build. Read at
    /// runtime, not `env!`: a compile-time stamp would invalidate the Rust build
    /// cache on every commit and turn a 30-second CI into a full rebuild.
    pub git_sha: String,
}

impl Config {
    /// The health client config, present only when both the URL and token are set.
    pub fn health(&self) -> Option<(&str, &str)> {
        match (&self.health_internal_url, &self.health_service_token) {
            (Some(url), Some(token)) => Some((url.as_str(), token.as_str())),
            _ => None,
        }
    }
}

fn env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let nc_base_url = env("NC_BASE_URL")?.trim_end_matches('/').to_string();
        // Fail fast at boot rather than panicking inside the /login handler at
        // request time: identity::authorize_url parses this as a base URL.
        let parsed = url::Url::parse(&nc_base_url)
            .with_context(|| format!("NC_BASE_URL is not a valid URL: {nc_base_url:?}"))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
            anyhow::bail!("NC_BASE_URL must be an http(s) URL with a host: {nc_base_url:?}");
        }
        Ok(Self {
            database_url: env("DATABASE_URL")?,
            session_secret: env("SESSION_SECRET")?,
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8080"),
            nc_base_url,
            nc_client_id: env("NC_CLIENT_ID")?,
            nc_client_secret: env("NC_CLIENT_SECRET")?,
            nc_redirect_uri: env("NC_REDIRECT_URI")?,
            static_dir: std::env::var("STATIC_DIR").ok(),
            catalog_dir: env_or("CATALOG_DIR", "data/catalog"),
            dev_login_user: std::env::var("DEV_LOGIN_USER").ok(),
            health_internal_url: std::env::var("HEALTH_INTERNAL_URL")
                .ok()
                .map(|u| u.trim_end_matches('/').to_string()),
            health_service_token: std::env::var("HEALTH_SERVICE_TOKEN").ok(),
            git_sha: env_or("GIT_SHA", "dev"),
        })
    }
}
