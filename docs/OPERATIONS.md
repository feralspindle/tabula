# tabula: build, deploy & operations

status: descriptive for what exists (`.github/workflows/ci.yml`,
`deploy/`, `kernel/src/{config,observability,main}.rs`); the "not yet ported"
list at the end is explicit. deployment shape is inherited from hexmapper's
proven single-box setup.

## build pipeline

three artifacts, three toolchains:

1. kernel: `cargo build --release -p tabula-kernel` (native workspace:
   `crates/tabula-core`, `kernel`, `vendor/ttrpg-dice-engine`).
2. plugins: `plugins/build.sh`. for each `plugins/*/` (each its own
   excluded cargo workspace) build `--target wasm32-unknown-unknown
   --release`, then `wasm-tools component new` → `plugins/dist/<name>.wasm`,
   then `wasm-tools validate`. the two-step build is a security property, not
   a convenience (see `AUTH_SECURITY.md`, zero WASI imports).
3. web: `cd web && npm run build` → static `dist/` (vite; content-hashed
   assets).

CI (`.github/workflows/ci.yml`, push to main + PRs):
- *native job*: `cargo fmt --check`; `cargo clippy --workspace --exclude
  ttrpg-dice-engine -D warnings` (the vendored crate is intentionally
  unlinted); `plugins/build.sh` runs before `cargo test` so the kernel's
  runtime/e2e tests exercise the real WASM artifacts, not stubs; then
  `cargo test --workspace`. DB-gated tests self-skip without `DATABASE_URL`.
- *web job*: `npm ci && npm run build` (plus `vitest` for the fold-parity
  tests when wired into the script).

tests that need a database use a disposable postgres; locally that's the
`tabula_test` database (`postgresql://postgres:postgres@localhost:54322/tabula_test`).

## runtime topology (single box + Cloudflare)

```
 browser ──TLS──▶ Cloudflare ──TLS (origin cert)──▶ Caddy ─┬─ /api/* ──▶ kernel :8080 (HTTP+WS)
                                                           └─ /*     ──▶ static SPA (/srv/www)
 kernel ──▶ Postgres (Supabase-hosted or self-managed)     kernel ◀── /opt/tabula/plugins (ro mount)
 browser ──▶ Supabase Auth (login, token refresh — direct, not proxied)
```

- single origin: SPA and API share `{$DOMAIN}`, so cookies/CORS stay
  trivial and the CSP can be strict (`deploy/Caddyfile`).
- compose services (`deploy/docker-compose.yml`): `api` (kernel image,
  built by `deploy/Dockerfile`, multi-stage rust→debian-slim; plugins are
  *not* baked in, they mount read-only from `/opt/tabula/plugins`) and
  `caddy` (SPA fileserver + reverse proxy + TLS with the Cloudflare origin
  cert). durable state lives under `/opt/tabula/{.env,certs,www,plugins}` so
  repo re-clones touch nothing.
- state is externalized: the box carries no authoritative data. postgres
  holds everything (`PERSISTENCE_REPLAY.md`), so the container is cattle;
  `docker compose up -d --build` is the deploy and rollback is redeploying
  the previous image.

## configuration (kernel env)

| var | required | default | meaning |
|---|---|---|---|
| `DATABASE_URL` | yes | - | postgres DSN |
| `SUPABASE_URL` | yes | - | JWKS source (`/auth/v1/.well-known/jwks.json`) |
| `CORS_ALLOWED_ORIGIN` | yes | - | comma-separated origin allowlist |
| `PORT` | no | 8080 | listen port |
| `PLUGIN_DIR` | no | `plugins/dist` | directory of `*.wasm` components loaded at boot |
| `SNAPSHOT_EVERY` | no | 200 | snapshot cadence in records |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | no | unset | traces off when unset |

web build-time env (`web/.env`): `VITE_API_BASE_URL`, `VITE_WS_BASE_URL`,
`VITE_SUPABASE_URL`, `VITE_SUPABASE_PUBLISHABLE_DEFAULT_KEY`.

boot order (`kernel/src/main.rs`): dotenv → tracing/metrics → config →
DB pool → JWKS fetch (fails fast if supabase is unreachable at boot) → load
all plugins from `PLUGIN_DIR` (fails fast on an invalid component) → spawn
JWKS-refresh and ratelimit-prune tasks → serve. plugins load once at
boot; adding/upgrading a plugin is a deploy (drop the `.wasm` in
`/opt/tabula/plugins`, restart `api`). sessions pin plugin id+version at
creation, so a kernel restart with a newer plugin build changes nothing for
existing sessions until an explicit upgrade story (packs, `migrate`) says so.

## migrations

`kernel/migrations/*.sql`, applied in filename order, all idempotent
(`create table if not exists`). MVP applies them manually (`psql -f`); no
migration runner is embedded in the kernel. when that graduates, sqlx-migrate
is the natural fit, the files are already in its layout.

## observability

`kernel/src/observability.rs` (ported from hexmapper, service name
`tabula-kernel`):

- traces: `tracing` everywhere; OTLP export when
  `OTEL_EXPORTER_OTLP_ENDPOINT` is set, pretty logs to stdout otherwise.
  plugin `log` host-import output lands under target `plugin`, tagged but
  never trusted.
- metrics: prometheus exposition on `/metrics`. HTTP latency/status via
  `track_metrics` middleware, `ws_connections` gauge,
  `session_records_total` counter. grafana/alloy scrape configs are in
  hexmapper and not yet ported.
- health: `/healthz` (liveness). readiness beyond that is post-MVP.

signals worth alerting on once dashboards exist: unique-violation errors on
`log_records` (single-writer assumption broken, see
`PERSISTENCE_REPLAY.md`), fuel-exhaustion traps (plugin runaway), snapshot
write failures, WS lag-resync rate (broadcast buffer sizing).

## environments

| env | DB | auth | plugins | notes |
|---|---|---|---|---|
| local dev | `tabula_test` on :54322 (inside the hex_map supabase container) | real supabase project (or e2e's embedded JWKS in tests) | `plugins/dist` via build.sh | `cargo run -p tabula-kernel` + `npm run dev` (vite proxies nothing; CORS allows localhost) |
| CI | ephemeral postgres when provided; DB tests skip otherwise | embedded test JWKS only | built fresh every run | no external secrets needed for green CI |
| prod | managed postgres | production supabase project | `/opt/tabula/plugins` | Cloudflare → Caddy → kernel |

## not yet ported from hexmapper (deliberate MVP cuts)

- bootstrap/provisioning scripts (droplet setup, `/opt/tabula` layout
  creation) and the deploy convenience scripts. compose/Caddyfile/Dockerfile
  are done, the wrapper automation is not.
- alloy + grafana observability configs (metrics endpoint exists; nothing
  scrapes it yet).
- CI deploy job (build → push image → rsync SPA → compose up). current CI
  is verification only.
- backup automation for postgres (supabase-managed DBs cover this;
  self-managed would need pg_dump cadence. the log tables are the only
  thing that matters).
