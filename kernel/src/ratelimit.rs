//! per-user request rate limiting for the HTTP API.
//!
//! keyed on the authenticated user id rather than IP: the API always sits behind
//! Cloudflare + Caddy, so the peer IP is the proxy's (IP limiting would lump every
//! user together), and every API request is authenticated anyway. the check lives
//! in the `AuthUser` extractor, which runs for every API handler, so a single token
//! bucket per user bounds the abuse-prone write endpoints (dice rolls, hex upserts,
//! photo broadcasts, joins) without per-route wiring.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use governor::{clock::DefaultClock, state::keyed::DefaultKeyedStateStore, Quota, RateLimiter};
use uuid::Uuid;

use crate::state::AppState;

/// sustained refill rate, requests per second per user.
const REFILL_PER_SEC: u32 = 40;
/// burst allowance, absorbs the fan-out of GETs on page load and fast drag-paints
/// while still capping a script hammering thousands of requests per second.
const BURST: u32 = 160;
/// how often fully-replenished buckets are reclaimed from the keyed store.
const RETAIN_INTERVAL: Duration = Duration::from_secs(120);

pub type UserRateLimiter = RateLimiter<Uuid, DefaultKeyedStateStore<Uuid>, DefaultClock>;

pub fn build() -> Arc<UserRateLimiter> {
    let quota = Quota::per_second(NonZeroU32::new(REFILL_PER_SEC).expect("nonzero refill"))
        .allow_burst(NonZeroU32::new(BURST).expect("nonzero burst"));
    Arc::new(RateLimiter::keyed(quota))
}

/// periodically drops fully-replenished buckets so the keyed store doesn't grow
/// unbounded with the set of all users ever seen.
pub fn spawn_retain(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(RETAIN_INTERVAL);
        loop {
            interval.tick().await;
            state.rate_limiter().retain_recent();
        }
    });
}
