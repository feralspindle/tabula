use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::jwt;
use crate::error::AppError;
use crate::state::AppState;

/// header-supplied forensic values land in the durable event log, so cap them.
const FIELD_MAX_LEN: usize = 80;

/// trims, drops empties, and caps a request header value.
fn header_capped(parts: &Parts, name: &str) -> Option<String> {
    parts
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(FIELD_MAX_LEN).collect())
}

pub struct AuthUser {
    pub user_id: Uuid,
    /// resolved from the JWT (mirrors `fill_display_name`), carried alongside user_id
    /// so events are human-readable without a join.
    pub display_name: String,
    /// semantic frontend action (`X-Intent`), e.g. `paint_terrain` vs `reveal_hex` -
    /// the highest-signal forensic field.
    pub intent: Option<String>,
    /// per-action correlation id (`X-Request-Id`), minted by the frontend so a client
    /// log line, the server span, and the resulting event all share one id. always
    /// present (generated server-side if the caller didn't send one).
    pub request_id: String,
    /// which browser tab/device issued it (`X-Client-Id`).
    pub client_id: Option<String>,
    /// frontend build that issued it (`X-App-Version`), catches "stale client".
    pub app_version: Option<String>,
    /// W3C trace id (from `traceparent`), for log↔trace correlation once tracing is on.
    pub trace_id: Option<String>,
    /// `METHOD /path` of the command, so the event records which endpoint produced it.
    pub route: String,
}

impl AuthUser {
    /// the forensic event-metadata envelope. `user_id`, `request_id`, and `route` are
    /// always present; the rest are included when the caller supplied them.
    pub fn metadata(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("user_id".into(), json!(self.user_id));
        m.insert("display_name".into(), json!(self.display_name));
        m.insert("request_id".into(), json!(self.request_id));
        m.insert("route".into(), json!(self.route));
        if let Some(v) = &self.intent {
            m.insert("intent".into(), json!(v));
        }
        if let Some(v) = &self.client_id {
            m.insert("client_id".into(), json!(v));
        }
        if let Some(v) = &self.app_version {
            m.insert("app_version".into(), json!(v));
        }
        if let Some(v) = &self.trace_id {
            m.insert("trace_id".into(), json!(v));
        }
        Value::Object(m)
    }

    /// `metadata()` merged with extra fields (e.g. server-resolved display_name, or a
    /// handler-supplied `actor_role`).
    pub fn metadata_with(&self, extra: Value) -> Value {
        let mut base = self.metadata();
        if let (Value::Object(b), Value::Object(e)) = (&mut base, extra) {
            for (k, v) in e {
                b.insert(k, v);
            }
        }
        base
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::Unauthorized)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AppError::Unauthorized)?;

        let claims = jwt::verify(token, &state.jwks()).map_err(|e| {
            tracing::warn!("jwt verification failed: {e}");
            AppError::Unauthorized
        })?;

        // per-user token bucket (keyed on the verified subject). bail before any
        // header parsing or DB work so a flood costs only a signature check.
        if state.rate_limiter().check_key(&claims.sub).is_err() {
            metrics::counter!("http_rate_limited_total").increment(1);
            return Err(AppError::TooManyRequests);
        }

        // traceparent format: 00-<32hex trace-id>-<16hex span-id>-<flags>
        let trace_id = header_capped(parts, "traceparent").and_then(|tp| {
            tp.split('-')
                .nth(1)
                .filter(|s| s.len() == 32)
                .map(str::to_string)
        });

        Ok(AuthUser {
            user_id: claims.sub,
            display_name: claims.display_name(),
            intent: header_capped(parts, "x-intent"),
            request_id: header_capped(parts, "x-request-id")
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            client_id: header_capped(parts, "x-client-id"),
            app_version: header_capped(parts, "x-app-version"),
            trace_id,
            route: format!("{} {}", parts.method.as_str(), parts.uri.path()),
        })
    }
}
