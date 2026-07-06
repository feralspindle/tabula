use jsonwebtoken::jwk::JwkSet;
use sqlx::PgPool;
use std::sync::{Arc, RwLock};

use crate::ratelimit::{self, UserRateLimiter};
use crate::runtime::PluginRuntime;
use crate::session::SessionRegistry;

#[derive(Clone)]
pub struct AppState(Arc<AppStateInner>);

pub struct AppStateInner {
    pub pool: PgPool,
    /// swappable so a background task can pick up Supabase key rotations without a
    /// restart (see `auth::jwt::spawn_jwks_refresh`).
    pub jwks: RwLock<Arc<JwkSet>>,
    pub rate_limiter: Arc<UserRateLimiter>,
    pub cors_allowed_origins: Vec<String>,
    /// compiled plugins, loaded once at boot; read-only thereafter.
    pub runtime: Arc<PluginRuntime>,
    /// live session actors.
    pub sessions: SessionRegistry,
    pub snapshot_every: u64,
}

impl AppState {
    pub fn new(
        pool: PgPool,
        jwks: JwkSet,
        cors_allowed_origins: Vec<String>,
        runtime: PluginRuntime,
        snapshot_every: u64,
    ) -> Self {
        Self(Arc::new(AppStateInner {
            pool,
            jwks: RwLock::new(Arc::new(jwks)),
            rate_limiter: ratelimit::build(),
            cors_allowed_origins,
            runtime: Arc::new(runtime),
            sessions: SessionRegistry::default(),
            snapshot_every: snapshot_every.max(1),
        }))
    }

    pub fn pool(&self) -> &PgPool {
        &self.0.pool
    }

    pub fn rate_limiter(&self) -> &UserRateLimiter {
        &self.0.rate_limiter
    }

    /// current JWKS snapshot. returns an `Arc` (not a borrow) so a concurrent
    /// rotation swap never invalidates an in-flight verification.
    pub fn jwks(&self) -> Arc<JwkSet> {
        self.0.jwks.read().expect("jwks lock poisoned").clone()
    }

    /// replaces the JWKS after a successful refresh.
    pub fn set_jwks(&self, jwks: JwkSet) {
        *self.0.jwks.write().expect("jwks lock poisoned") = Arc::new(jwks);
    }

    pub fn allows_origin(&self, origin: &str) -> bool {
        self.0
            .cors_allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
    }

    pub fn runtime(&self) -> &PluginRuntime {
        &self.0.runtime
    }

    pub fn sessions(&self) -> &SessionRegistry {
        &self.0.sessions
    }

    pub fn snapshot_every(&self) -> u64 {
        self.0.snapshot_every
    }
}
