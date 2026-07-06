# Tabula

System-agnostic virtual tabletop engine: a generic kernel (entity store, delta log,
realtime sync, WASM plugin runtime) with all game-system rules living in sandboxed
WASM Component Model plugins. See `docs/ARCHITECTURE.md` for the accepted MVP spec.

## Layout

- `crates/tabula-core` — pure log core: `Delta`, `LogRecord`, `World`, `apply`,
  schema registry, delta validation. No I/O, no async, no plugin knowledge.
- `kernel/` — Axum server: persistence (Postgres), wasmtime plugin host, sessions,
  WebSocket realtime, auth (Supabase JWT). The only crate that touches a database
  or executes plugins.
- `wit/` — the WIT host interface (`tabula.wit`). One of the three irreversible
  contracts (see below).
- `plugins/` — WASM component plugins, each its own Cargo workspace targeting
  `wasm32-wasip2`. `counter` is the runtime test fixture; `shadowdark` is the MVP
  System plugin.
- `vendor/ttrpg-dice-engine` — vendored dice crate. Host-side only; plugins reach
  it through the `roll` host import.
- `web/` — Vue 3 + Vite + Pinia + Tailwind frontend.

## Invariants (enforced by review and property tests — do not violate)

1. **Three irreversible contracts are owner-authored** and treated as read-only
   unless the owner explicitly asks: the `Delta`/`LogRecord` schema
   (`crates/tabula-core/src/delta.rs`), the WIT host interface (`wit/tabula.wit`),
   and (later) the DSL semantics. Claude Code's role on these is adversarial
   review, not authorship.
2. **The `Delta` enum is closed**: `Spawn`, `Despawn`, `Set`, `Remove`. Never add a
   variant to solve a feature problem; solve it in components or `cause`.
3. **Replay never executes plugin code.** `apply` is game-agnostic and total. If a
   change would require plugin logic during replay, the design is wrong.
4. **All nondeterminism flows through host imports.** Plugins get no ambient clock,
   RNG, I/O, or network. `decide` must be a pure function of
   (command, context, host-import results).
5. **Every log record has a `cause`** (command id, plugin id+version, actor).
6. **Deltas in a record apply atomically** — all or none.
7. **Plugins only write component namespaces they declared** in their manifest,
   plus `core.*` keys they have been granted. The kernel's capability validator
   enforces this on every `decide` result.
8. **Property tests to maintain** (in `crates/tabula-core/tests` and kernel tests):
   (a) fold(log) == fold(snapshot + tail); (b) replay of a recorded session is
   byte-identical to the live projection; (c) schema validation rejects any `Set`
   not matching a registered schema; (d) `seq` is gapless per session.

## Working agreements

- First-of-kind implementations (first domain slice, first plugin command, first
  WS frame type) get careful review; Nth-of-kind may be delegated.
- Rule errors from `decide` return to the issuing client only; nothing is appended
  to the delta log on failure.
- Schema evolution: plugin-exported `migrate` runs against component values at
  snapshot load, never against the log.

## Commands

- `cargo test` — native workspace (core + kernel; DB tests are skipped unless
  `DATABASE_URL` is set).
- `cargo build --target wasm32-wasip2 --release` inside a `plugins/*` directory
  builds that plugin component (rustup target `wasm32-wasip2` required).
- `kernel/migrations/*.sql` — apply in order to the Postgres database.
- Frontend: `cd web && npm install && npm run dev`.
