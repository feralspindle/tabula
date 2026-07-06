# tabula: MVP architecture specification

status: accepted design, ready for implementation
audience: coding agents and project owner
date: 2026-07-03

---

## 1. what tabula is

tabula is a system-agnostic virtual tabletop (VTT) engine. the core design goal is a clean separation between a generic engine kernel (entity storage, persistence, realtime sync, plugin runtime) and game-system-specific rules (shadowdark, VtM, CoC, ...) which live entirely inside sandboxed WASM plugins. the long-term differentiators, a no-code plugin generator and decentralized ATProto/IPFS content distribution, depend on getting this kernel right, but are explicitly out of scope for the MVP.

the prior project, `hexmapper` (shadowdark-specific VTT, vue + axum + supabase, live in production), serves two roles: a parts bin (infra, crates, UI components to port) and the acceptance target (tabula is validated when a shadowdark System plugin can reproduce hexmapper's character sheet + dice functionality).

## 2. MVP scope

### in scope
1. kernel: schema-driven entity store with runtime component registration.
2. persistence: delta log (append-only, host-owned closed vocabulary) + snapshots.
3. plugin runtime: wasmtime + WASM Component Model, one WIT-defined host interface.
4. one hand-written System plugin: shadowdark. character sheets (create/edit/roll) and dice resolution only.
5. realtime: websocket session server broadcasting applied log records to connected clients.
6. frontend: vue 3 app rendering schema-driven character sheets and a dice roller from plugin-declared schemas/layouts.
7. sessions & auth: minimal. session creation, join-by-URL, GM vs player roles (port hexmapper's model).

### explicitly deferred (do NOT build)
- no-code plugin generator, trigger/condition/effect DSL, LLM rule authoring
- ATProto/IPFS package registry, manifests, AppView
- hex map, dungeon mapper, vault, notebook, calendar, chat, photos
- Module plugins (additive content). design the type distinction, implement System plugins only
- theming tiers, marketplace, payments
- voice/video

## 3. tech stack

| layer | choice | notes |
|---|---|---|
| kernel/server | rust, axum 0.8, tokio | same stack as hexmapper server |
| plugin runtime | wasmtime, WASM Component Model, WIT | sandboxed; plugins in any WASM-targeting language |
| persistence | postgres via sqlx (supabase-hosted) | delta log + snapshots + session/auth tables |
| auth | supabase JWT (JWKS / ES256) | port `auth/` + `authz.rs` from hexmapper |
| realtime | axum websockets | host broadcasts applied LogRecords |
| frontend | vue 3 + vite + pinia + tailwind | port dice roller UI + apiClient patterns from hexmapper |
| dice | `ttrpg-dice-engine` (vendored crate from hexmapper) | host-side, exposed to plugins via WIT (determinism) |
| observability | tracing + OTel + prometheus | port hexmapper's `observability.rs` and grafana dashboards |
| deploy | docker compose + Caddy (DigitalOcean) | port hexmapper's `deploy/` |

## 4. kernel design

### 4.1 entity store (NOT an ECS library)

decision (from ADR review): do not use hecs or any rust ECS crate. ECS libraries assume compile-time component types; tabula's components are declared at runtime by plugins. instead:

- entity: opaque `EntityId` (UUID v7 recommended for log locality).
- component: `ComponentKey` = namespaced string, e.g. `shadowdark.stats`, `core.name`. values are structured data (CBOR on the wire/log; `serde_json::Value`-equivalent in memory is acceptable for MVP).
- component schemas: plugins register JSON-Schema-style component definitions at load time. the host validates every `Set` delta's value against the registered schema before accepting it.
- projection (`World`): in-memory `entity → component → value` map per active session. rebuilt by folding the delta log (or snapshot + tail).

### 4.2 delta log (persistence strategy)

core insight this design rests on: the mutations-out WASM boundary makes classic event sourcing structurally unavailable, because semantic domain events are born and die inside the plugin. the host can only honestly attest to (a) the command it received and (b) the deltas the plugin returned. therefore the log records what changed, in a closed vocabulary the host owns and no plugin can extend:

```rust
/// The complete, closed instruction set of the log. Host-owned. Game-agnostic.
enum Delta {
    Spawn   { entity: EntityId },
    Despawn { entity: EntityId },
    Set     { entity: EntityId, component: ComponentKey, value: CborValue },
    Remove  { entity: EntityId, component: ComponentKey },
}

struct LogRecord {
    seq: u64,            // per-session, monotonic, gapless
    at: GameTime,        // host session clock
    cause: Cause,        // provenance: command id, plugin id + version, actor
    deltas: Vec<Delta>,  // applied atomically — all or none
}
```

- one `apply` function folds deltas into the projection. it is game-agnostic and must remain so forever: replay never executes plugin code.
- replay: `for record in log { for d in record.deltas { apply(world, d) } }`.
- snapshots: periodic full-projection snapshots so cold loads are snapshot + tail. schema evolution = plugin-exported `migrate` run against component values at snapshot load, never against the log.
- `cause` is mandatory on every record. it preserves auditability the delta vocabulary can't express (which action, which plugin+version, which actor, or `timer`/`gm-override`).
- postgres table sketch: `log_records(session_id, seq, at, cause jsonb, deltas jsonb/bytea, created_at)` with `PRIMARY KEY (session_id, seq)`; `snapshots(session_id, upto_seq, world bytea)`.

### 4.3 plugin boundary (WIT / Component Model)

plugin types: *System* plugin, exactly one per session, owns the rules; *Module* plugin, additive content (deferred, but keep the enum).

contract shape (WIT, hand-authored, see §8):

host → plugin (imports the plugin can call):
- `roll(expr: string) -> roll-result`: host-side dice via ttrpg-dice-engine, host RNG
- `random(bounds) -> u64`: host-supplied RNG
- `now() -> game-time`: session clock
- `get-component(entity, key) -> option<value>` / bounded query functions
- `log(level, msg)`: diagnostics

plugin → host (exports the host calls):
- `manifest() -> plugin-manifest`: id, version, plugin type, declared component schemas, declared commands, sheet layout(s)
- `decide(command, context) -> result<list<delta>, rule-error>`: the single rules entry point
- `migrate(component-key, old-version, value) -> value`: schema evolution at snapshot load

determinism rules (invariants):
- plugins get NO ambient capabilities: no clock, no RNG, no I/O, no network. all nondeterminism flows through host imports so it can be recorded in `cause`/context and replayed.
- `decide` must be a pure function of (command, context, host-import results).

command flow (the one loop everything goes through):
```
client command (WS)
  → host authz check (session membership, role)
  → host assembles context (relevant entities/components)
  → plugin.decide(command, context)
  → returns Vec<Delta> or rule error
  → host capability validator (schema validation; plugin may only touch
    namespaces it declared, plus core.* it's been granted)
  → append one LogRecord (atomic)
  → fold into projection
  → broadcast record to session clients
```
rule errors return to the issuing client only; nothing is logged to the delta log on failure.

### 4.4 sessions, realtime, auth

- session = one game table: `session_id`, GM user, member list, active System plugin id+version, log, projection.
- join model ported from hexmapper: session UUID in URL is the invite; supabase auth (magic link / Discord OAuth); owner = GM.
- WS protocol: client sends `command` frames; server pushes `record` frames (seq-ordered) plus `snapshot` on join. clients apply the same fold logic client-side (mirror `apply` in JS) to maintain a local projection.
- late joiner / reconnect: send `upto_seq`, server replays tail.

## 5. frontend (MVP)

- vue 3 SPA. pinia store holding the client-side projection, folded from WS records.
- schema-driven sheet renderer: the shadowdark plugin's manifest declares component schemas + a sheet layout description; the frontend renders sheets generically from these (field widgets: number, text, track/pips, rollable stat). no shadowdark-specific vue components. that's the point.
- dice roller UI ported from hexmapper (macros/history can come later; MVP = roll expression + result display + roll-from-sheet).
- views: Home (create/join session), Session (sheet list, active sheet, dice, member list).

## 6. shadowdark System plugin (MVP validation target)

written in rust, compiled to a WASM component. declares:
- components: `shadowdark.identity` (name, class, level, ancestry, ...), `shadowdark.stats` (six abilities), `shadowdark.hp`, `shadowdark.luck`, `shadowdark.inventory` (gear slots)
- commands: `create-character`, `update-sheet-field`, `roll-check {stat, advantage?}`, `spend-luck`, `roll-dice {expr}`
- sheet layout for the generic renderer

acceptance test: a group can create shadowdark characters, edit sheets, and make stat checks/dice rolls in a live session, with the full history reconstructable by replay, matching the character+dice slice of hexmapper.

## 7. what to port from hexmapper (verbatim or lightly adapted)

- `server/src/auth/` (supabase JWKS ES256 validation), `authz.rs` patterns
- `error.rs` (AppError → HTTP mapping), `ratelimit.rs`, `observability.rs`, `config.rs`
- `vendor/ttrpg-dice-engine` as a workspace member
- `deploy/` (docker compose, Caddyfile, alloy, grafana dashboards, deploy/bootstrap scripts)
- frontend: apiClient, auth store, dice roller components, WS handling patterns from `realtimeProtocol`

do not port: domain-per-feature handler/projection modules (hex, dungeon, vault, notebook, ...) or the per-aggregate event/projector system in `server/src/events/`. tabula's delta log replaces that organizing principle.

## 8. invariants & working agreements for coding agents

these belong in the repo's `CLAUDE.md` and in property tests:

1. three irreversible contracts are hand-authored by the owner and treated as read-only by the agent unless explicitly asked: the `Delta`/`LogRecord` schema, the WIT host interface, and (later) the DSL semantics. the agent's role on these is adversarial review, not authorship.
2. the `Delta` enum is closed. never add variants to solve a feature problem; solve it in components or `cause`.
3. replay never executes plugin code. if a change would require plugin logic during replay, the design is wrong.
4. all nondeterminism flows through host imports. no clock/RNG/I/O inside plugins.
5. every log record has a `cause`.
6. deltas in a record apply atomically.
7. plugins only write component namespaces they declared (plus granted `core.*`).
8. property tests to maintain: (a) fold(log) == fold(snapshot + tail); (b) replay of a recorded session is byte-identical to the live projection; (c) schema validation rejects any `Set` not matching a registered schema; (d) `seq` is gapless per session.
9. first-of-kind implementations (first domain slice, first plugin command, first WS frame type) reviewed carefully; nth-of-kind may be delegated wholesale.

## 9. suggested milestone order

1. M0, skeleton: new repo, workspace layout (`kernel/`, `plugins/shadowdark/`, `web/`), ported infra (auth, error, observability, deploy), CI.
2. M1, log core: Delta/LogRecord types, postgres persistence, apply/fold, snapshots, property tests (no plugins yet; drive with test fixtures).
3. M2, plugin runtime: WIT interface, wasmtime host, manifest loading, component schema registration, capability validator; a trivial "counter" test plugin.
4. M3, session loop: WS server, command flow end-to-end with the test plugin, reconnect/tail replay.
5. M4, shadowdark plugin: components, commands, dice via host import.
6. M5, frontend: projection store, schema-driven sheet renderer, dice UI, session views.
7. M6, parity check: run a real session; compare against hexmapper's character+dice experience; write up gaps.

## 10. open questions (decide during build, don't block on)

- CBOR vs JSON for log/wire encoding (CBOR preferred for log storage; JSON acceptable for MVP wire format)
- snapshot cadence policy (every N records vs time-based)
- context assembly strategy for `decide`: host-pushed full relevant set vs plugin-pulled via query imports (MVP: pulled via bounded query imports is simpler to keep honest)
- exact `core.*` component set the host itself owns (`core.name` at minimum)
