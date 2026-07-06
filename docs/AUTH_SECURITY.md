# tabula: auth & security model

status: descriptive, documenting authentication, authorization, and the
defense layers as built. ported from hexmapper's proven auth stack
(`kernel/src/auth/*`, `kernel/src/authz.rs`, `kernel/src/ratelimit.rs`).

## identity: supabase is the IdP, the kernel only verifies

the kernel holds no credentials and no user table. users authenticate
with supabase (email/password or Discord OAuth via the web app); every
request to the kernel carries a supabase-issued JWT which the kernel verifies
locally (signature against supabase's JWKS, `aud = "authenticated"`,
expiry) with zero per-request calls to supabase.

- JWKS lifecycle (`auth/jwt.rs`): fetched at boot from
  `{SUPABASE_URL}/auth/v1/.well-known/jwks.json`, refreshed every 15 min so
  key rotation needs no restart. a failed refresh retains the previous key
  set, so a transient supabase outage never breaks verification of
  still-valid tokens.
- claims used: `sub` (the user uuid, the only identity the rest of the
  kernel ever sees), `exp`, `aud`, and `user_metadata` (display name
  resolution: full_name → global_name → name → user_name → email →
  "Adventurer").
- transport: HTTP requests use `Authorization: Bearer` (extracted by
  `AuthUser`); the WS upgrade uses `?token=` because browsers cannot set
  headers on WebSocket handshakes. trade-off acknowledged: query strings can
  land in logs. mitigations: the token is a short-lived access token (not the
  refresh token), TLS end-to-end, and our own access logs come from
  tower-http traces that don't record query strings. (post-MVP option if this
  ever tightens: one-time ticket endpoint → ticket in the query.)

tests exercise the real verification path: `session_e2e.rs` embeds a
test-only ES256 keypair + JWKS and mints supabase-shaped tokens, and the kernel
verifies them exactly as it verifies production tokens.

## authorization: two roles, session-scoped

the whole authz model is two SQL predicates (`authz.rs`):

- **member**: session owner or has a `session_members` row. required for:
  session detail, WS connect (checked *before* upgrade), and implicitly every
  command (commands only travel over an authorized WS).
- **GM**: the session owner, immutable for the session's lifetime (MVP
  simplification). determined at WS connect; rides the connection as
  `actor_is_gm` into every command, where it becomes `Actor::GmOverride` in
  the log's cause. GM interventions are permanently and visibly attributed.

join-by-URL is a prototype carryover, not the intended model. today,
knowing a session's UUIDv4 grants membership via idempotent
`POST /sessions/{id}/join`, inherited from the hexmapper prototype, where it
was a convenience (and where changing it is a prerequisite to ever reopening
signups). owner intent: tabula replaces this before any public exposure.
the replacement shape is dedicated invite tokens (random, stored
server-side, GM-created/revocable, optionally expiring or single-use)
redeemed at a `POST /invites/{token}/accept` that writes the
`session_members` row. session UUIDs then stop being credentials: `join`
goes away, and every session endpoint already gates on membership, so the
change is confined to the invite path (plus GM invite-management UI). until
then, treat session URLs as bearer secrets.

game-level authorization belongs to plugins: who may edit a sheet, spend
luck, or roll for an entity is a *rule*, decided in `decide` (see
`ownership_is_enforced` in `kernel/tests/shadowdark_plugin.rs`). the
frontend's `canEdit` is UX affordance only; the kernel-side plugin is the
enforcement point. the kernel supplies honest inputs (`actor`, `actor_is_gm`)
and never lets a client speak for another user. `actor` is always the
verified JWT `sub` of the connection, never client-supplied.

## the plugin containment stack

plugins are the least-trusted code in the system, eventually authored by
third parties or generated from prose (`PLUGIN_GENERATION.md`). containment
is layered; each layer assumes the ones above failed:

| layer | stops | mechanism |
|---|---|---|
| 1. no ambient capabilities | exfiltration, clocks, RNG, filesystem, network | components built from `wasm32-unknown-unknown`: zero WASI imports exist in the artifact (verifiable: `wasm-tools component wit dist/x.wasm`) |
| 2. memory isolation | corrupting the kernel | wasmtime sandbox; plugin memory is its own |
| 3. fuel metering | infinite loops / CPU DoS | 10⁹ fuel per call, refueled per call; exhaustion traps the call, actor survives |
| 4. output is untrusted | forged/malformed state | every delta re-parsed (uuid/key/JSON) then capability-validated: declared namespaces + granted `core.*` keys only, schema-checked values, batch consistency, all-or-none (`PLUGIN_BOUNDARY.md`) |
| 5. attribution | silent tampering | every record carries `cause` {command, plugin id+version, actor}; the log is append-only |

a malicious plugin's worst case is therefore: writing *well-formed, in-schema
values inside its own declared namespaces, attributed to itself, in response
to a member's command*. it cannot read other sessions (host imports are bound
to one session's world), cannot mint time or randomness, and cannot touch
`core.*` beyond explicit grants (`core.name` for System plugins).

## other perimeter controls

- rate limiting (`ratelimit.rs`, governor): per-user keyed quotas applied
  to HTTP requests, WS upgrades, and *each* WS command frame, so a connected
  client can't machine-gun commands past the same budget. keys are retained
  and periodically pruned by a background task.
- CORS: explicit allowlist from `CORS_ALLOWED_ORIGIN`; no wildcard.
- browser hardening (deploy Caddyfile): CSP with `default-src 'self'`,
  `connect-src` limited to self + supabase + `wss://{DOMAIN}`,
  `frame-ancestors 'none'`, nosniff, strict referrer. single origin: the SPA
  and `/api` share a domain, so no cross-site auth flows.
- error discipline (`error.rs`): rule errors are shown verbatim (they're
  the game talking); internal errors return generic messages with details
  only in server traces. rule errors go to the issuing client only, so other
  players can't observe your refused commands.
- SQL: sqlx bound parameters everywhere; no string-built queries.
- secrets: kernel env is `DATABASE_URL` + `SUPABASE_URL` only. there is
  no supabase service-role key in the kernel at all, so there is none to
  leak. the web app holds only the publishable anon key.

## known gaps (accepted for MVP, tracked)

- join-by-URL membership: the biggest one; replacement design above.
  not acceptable beyond private testing.
- WS token in query string (mitigated above; ticket endpoint if needed).
- no per-session command audit UI. the data is all in `log_records.cause`;
  surfacing it is frontend work.
- no plugin signature/provenance checking: `PLUGIN_DIR` contents are trusted
  as operator-installed. becomes real work only with third-party distribution
  (packs improve this: they're auditable data, not opaque binaries).
- rate limits are per-user, not per-session; a hostile *member* can still be
  noisy within quota. GM kick/ban tooling is post-MVP.
