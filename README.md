# tabula

a system-agnostic virtual tabletop engine. the kernel knows nothing about any
game; rules live in sandboxed WASM Component Model plugins, and every change at
the table is one record in an append-only delta log.

the table is a ledger: `command → plugin.decide → validate → append → fold
→ broadcast`. replay never executes plugin code.

see `docs/README.md` for the full architecture doc set (spec, plugin
boundary, persistence/replay, realtime, frontend, security, operations) and
`CLAUDE.md` for the invariants.

## layout

| path | what |
|---|---|
| `crates/tabula-core` | pure log core: closed `Delta` vocabulary, `World` projection, `apply`, schema registry, capability validator |
| `kernel/` | axum server: postgres delta log + snapshots, wasmtime plugin host, sessions, websocket realtime, supabase JWT auth |
| `wit/tabula.wit` | the host ⟷ plugin contract (owner-authored) |
| `plugins/counter` | trivial test System plugin |
| `plugins/shadowdark` | shadowdark System plugin (MVP target: sheets + dice) |
| `vendor/ttrpg-dice-engine` | vendored dice crate, host-side |
| `web/` | vue 3 frontend: schema-driven sheet renderer, dice, session views |

## quick start

```sh
# 1. plugins (requires rustup target wasm32-unknown-unknown + wasm-tools)
./plugins/build.sh

# 2. database: apply kernel/migrations/*.sql to your postgres, then
cp .env.example .env   # fill in DATABASE_URL, SUPABASE_URL, CORS_ALLOWED_ORIGIN

# 3. kernel
cargo run -p tabula-kernel

# 4. frontend
cd web && cp .env.example .env && npm install && npm run dev
```

## tests

```sh
cargo test --workspace          # core property tests + runtime tests
DATABASE_URL=… cargo test       # also runs store + session e2e against postgres
cd web && npm test              # client fold parity tests
```

plugins are sandboxed hard: they compile from `wasm32-unknown-unknown`, so the
resulting components have zero WASI imports. no clock, RNG, filesystem, or
network exists for a plugin except the host imports declared in `wit/tabula.wit`.
