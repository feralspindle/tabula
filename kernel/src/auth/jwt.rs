use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, errors, DecodingKey, Validation};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SupabaseClaims {
    pub sub: Uuid,
    pub exp: usize,
    pub role: String,
    pub email: Option<String>,
    #[serde(default)]
    pub user_metadata: serde_json::Value,
}

impl SupabaseClaims {
    /// display name, mirroring the `fill_display_name` SQL trigger's coalesce order,
    /// resolved straight from the token (no DB round trip).
    pub fn display_name(&self) -> String {
        let m = &self.user_metadata;
        ["full_name", "global_name", "name", "user_name"]
            .iter()
            .find_map(|k| m.get(*k).and_then(|v| v.as_str()))
            .or(self.email.as_deref())
            .unwrap_or("Adventurer")
            .to_string()
    }
}

pub async fn fetch_jwks(supabase_url: &str) -> Result<JwkSet, reqwest::Error> {
    let url = format!(
        "{}/auth/v1/.well-known/jwks.json",
        supabase_url.trim_end_matches('/')
    );
    reqwest::get(&url).await?.json::<JwkSet>().await
}

/// periodically re-fetches the Supabase JWKS so signing-key rotations are picked up
/// without a restart. on a failed fetch the previous key set is retained, so a
/// transient Supabase outage never breaks verification.
pub fn spawn_jwks_refresh(supabase_url: String, state: crate::state::AppState) {
    const REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(REFRESH_INTERVAL);
        interval.tick().await; // consume the immediate first tick; keys are already fresh at startup
        loop {
            interval.tick().await;
            match fetch_jwks(&supabase_url).await {
                Ok(jwks) => state.set_jwks(jwks),
                Err(error) => {
                    tracing::warn!(%error, "JWKS refresh failed; retaining previous key set")
                }
            }
        }
    });
}

pub fn verify(token: &str, jwks: &JwkSet) -> Result<SupabaseClaims, errors::Error> {
    let header = decode_header(token)?;

    let kid = header
        .kid
        .as_deref()
        .ok_or(errors::ErrorKind::InvalidToken)?;
    let jwk = jwks.find(kid).ok_or(errors::ErrorKind::InvalidToken)?;
    let decoding_key = DecodingKey::from_jwk(jwk)?;

    let mut validation = Validation::new(header.alg);
    validation.set_audience(&["authenticated"]);

    let decoded = decode::<SupabaseClaims>(token, &decoding_key, &validation)?;

    Ok(decoded.claims)
}
