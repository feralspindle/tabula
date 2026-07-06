# tabula: the plugin boundary

status: descriptive. this documents the boundary as built and tested
(`kernel/tests/runtime_counter.rs`, `kernel/tests/shadowdark_plugin.rs`).
the normative contract is `wit/tabula.wit` (owner contract #2); if this doc
and the WIT disagree, the WIT wins. the pack interpreter proposed in
`docs/PLUGIN_GENERATION.md` sits behind this exact same boundary.

## the shape in one sentence

a plugin is an advisory pure function: the kernel pushes it a command,
the plugin pulls what it needs through seven host imports, and it returns
either a *proposal* of mutations in a closed vocabulary or a rule refusal.
it never has the authority to change anything itself.

```
                        ┌─────────────────────── KERNEL (session actor) ──────────────────────────┐
  client ──command──▶   │ authz → decide ─────▶ validate → append(log) → fold(world) → broadcast  │
                        └───────────│──────────────▲────────────────────────────────────────────  ┘
                                    │ push         │ returns
                                    ▼              │
                        ┌──────── PLUGIN (WASM component, fuel-metered) ────────┐
                        │  decide(command, context) → result<list<delta>,       │
                        │                                    rule-error>        │
                        │        │ pull (host imports, the only capabilities)   │
                        │        ▼                                              │
                        │  get-component / entities-with     ← reads projection │
                        │  roll / random / now               ← all nondeterminism
                        │  new-entity-id / log               ← ids, diagnostics │
                        └───────────────────────────────────────────────────────┘
```

## in: the command, and almost nothing else

`decide` receives two arguments:

```
command {
  id: string          // uuid, becomes cause.command_id in the log
  name: string        // must be in the plugin's declared commands
  actor: string       // issuing user id
  actor-is-gm: bool   // the kernel's authz verdict — plugins never do auth lookups
  payload: json       // client's arguments, validated per the command's params-schema
}
context: json         // deliberately minimal — today just { "session": <id> }
```

the pushed context stays tiny on purpose. everything else is *pulled*
through the imports, which is what makes the purity invariant checkable:
`decide` is a pure function of (command, context, the sequence of
host-import results). if the host scripts those imports, forcing `roll` to
return 17, the output is fully reproducible. (the pack test-bench in
`PLUGIN_GENERATION.md` §6.2 is built on exactly this property.)

the seven imports split into three groups:

- reads: `get-component(entity, key) → option<json>`,
  `entities-with(key) → list<ids>`. bounded queries against the live
  projection; the plugin never holds the world, it asks questions.
- nondeterminism: `roll(expr)` (host RNG through ttrpg-dice-engine,
  returning the total *and* a JSON breakdown so the log preserves the
  evidence), `random(min, max)` (inclusive), `now()`. these are the *only*
  sources. plugin components are built from `wasm32-unknown-unknown` and so
  structurally have zero WASI imports; there is no ambient clock or RNG
  to sneak past this list.
- bookkeeping: `new-entity-id()` (host mints uuid v7 so entity ids stay
  host-ordered) and `log(level, msg)` (routed to host tracing only; never
  touches the delta log).

## out: a proposal in a closed vocabulary, or a refusal

the success path returns `list<delta>`, where delta is exactly the four ops
mirroring the closed `Delta` enum (owner contract #1):

```
spawn(entity) | despawn(entity) | set{entity, component, value: json} | remove{entity, component}
```

the failure path returns `rule-error { message }`: "the rules say no." it
goes back to the issuing client only; nothing is appended on refusal, so
the log contains only things that happened.

the critical property: the return value is untrusted input. the WIT says
it outright ("this type grants no authority"). before anything touches the
log, the kernel runs the batch through four gates:

1. parse (`kernel/src/runtime/mod.rs`, `parse_delta`): entity strings
   must be real uuids, component keys well-formed `namespace.name`, values
   valid JSON. garbage here is a plugin fault (500-class), not a rule error.
2. capability validation (`tabula-core::validate_deltas`): every
   `set`/`remove` must be in a namespace the plugin declared in its manifest
   (plus granted `core.*` keys); every `set` value must pass the registered
   JSON Schema; spawn/despawn/set consistency is checked batch-locally
   (spawn-then-set in one batch is fine; set-after-despawn is not). one bad
   delta rejects the whole batch. atomicity is all-or-none by construction.
3. append: the kernel (never the plugin) wraps the batch in a
   `LogRecord` with `cause` {command_id, command name, plugin id+version,
   actor} and the gapless per-session seq.
4. fold: the same game-agnostic `apply` that replay uses projects the
   record into the world. replay never crosses the plugin boundary at all; a
   plugin could be deleted and every session it ran remains perfectly
   reconstructible from the log.

## lifecycle calls (the other two exports)

- `manifest()`: called once at load. the plugin *declares* its side of the
  contract: component schemas (whose key namespaces become its write
  grants), command names + payload schemas, and the sheet layout the generic
  frontend renderer consumes. everything the validator later enforces is
  rooted here. grants are derived from the manifest, never requested at
  runtime.
- `migrate(key, old_version, value) → value`: schema evolution, run against
  component values at snapshot load only, never against the log. (wiring at
  snapshot load is pending, `TODO(M6)` in `kernel/src/session/actor.rs`.)

the pack proposal (`PLUGIN_GENERATION.md`) adds exactly one call to this
surface, `configure(pack) → result<manifest, string>`, invoked once between
instantiation and the first `decide`, and changes nothing else about the
boundary. that is the strongest argument for the interpreter approach:
generated systems inherit this entire enforcement stack because they live
behind the same exports as hand-written rust.

## one asymmetry worth knowing

reads are live, writes are deferred. `get-component` sees the current
projection mid-`decide`, but the plugin's own deltas don't exist until the
kernel validates/appends/folds them after `decide` returns. a plugin can
never observe its own uncommitted writes, which is what keeps a record
atomic and the log the single source of truth.

## enforcement summary

| layer | enforces | where |
|---|---|---|
| component build (no WASI) | no ambient clock/RNG/IO/network | `plugins/build.sh` (wasm32-unknown-unknown → `wasm-tools component new`) |
| fuel metering | bounded compute per call | `kernel/src/runtime/mod.rs` (`FUEL_PER_CALL`) |
| delta parse | well-formed ids/keys/JSON | `kernel/src/runtime/mod.rs` |
| capability validator | namespaces, schemas, batch consistency | `crates/tabula-core/src/validate.rs` |
| session actor | cause attribution, gapless seq, atomic append, fold, fan-out | `kernel/src/session/actor.rs` |
