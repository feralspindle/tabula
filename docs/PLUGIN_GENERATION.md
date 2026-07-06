# tabula: plugin generation design (proposal)

status: proposal, needs owner review. the rule-language semantics defined
here would become the third irreversible contract (alongside the `Delta` schema
and the WIT interface), so nothing below is buildable until the owner blesses
§3 and §4.

goal: a UI where someone who is not a programmer (possibly assisted by an LLM)
turns a game's rules into something the kernel can run, without weakening any
existing invariant.

---

## 1. the central decision: interpret data packs, don't generate code

there are two ways to make "UI output" executable:

A. codegen: the UI emits rust (or any language), a build service compiles
it to a WASM component per game system.

B. one interpreter, many data packs: we write *one* WASM component, the
rules interpreter, carefully, by hand, once. every UI-authored game system
is a pure-data artifact (a pack: JSON containing components, schemas,
layouts, commands, and rule expressions). the kernel instantiates the
interpreter component and hands it the pack; the pair behaves exactly like a
hand-written plugin behind the existing WIT `guest` interface.

this design chooses B. reasons, in order of importance:

1. determinism is inherited, not re-proven. invariant #4 (all
   nondeterminism through host imports) must hold for every plugin. with
   codegen we'd have to trust generated code, or an LLM's output, on every
   publish. with an interpreter, we prove the property once for the
   interpreter and the language it evaluates; the language simply has no
   construct that can express nondeterminism (§3.4). packs are safe by
   construction.
2. no toolchain in the serving path. no build farm, no cargo, no
   compile-wait in the authoring loop. the UI saves JSON; it is runnable
   immediately. publishing a game system is a database write.
3. packs are auditable. a pack is a readable document. a GM (or a
   marketplace reviewer, later) can inspect exactly what a system does. a
   compiled component is opaque.
4. the whole existing pipeline is unchanged. the interpreter is a normal
   `system-plugin` component: zero WASI imports, fuel-metered, capability-
   validated per decide. `tabula-core`, the session actor, the store, the WS
   protocol, and the frontend renderer need no changes at all.

hand-written rust plugins (like `shadowdark` today) remain the escape hatch
for anything the language can't express. both kinds sit behind the same WIT
world; the kernel doesn't care which kind it's running.

### why not "LLM writes rust"?

because then the trust boundary is the LLM. sandboxing contains malice, but
not nondeterminism bugs, panics in edge cases, or subtle rule errors that
poison a session's log forever. the right role for an LLM is *front-end*:
translate prose rules into the constrained pack format (§6.3), where every
construct is validatable, previewable, and testable before a single delta is
appended.

---

## 2. the pack artifact

a pack is one JSON document (canonical form; content-hash = version identity):

```jsonc
{
  "pack_format": 1,
  "id": "mothership",              // same rules as plugin id today
  "version": "0.3.0",
  "name": "Mothership 1e",
  "components": {                   // → JSON Schemas, exactly as manifests today
    "mothership.stats": { "type": "object", "properties": { /* … */ } },
    "mothership.stress": { /* … */ }
  },
  "sheet_layout": { /* same layout language the renderer already reads,
                       including the existing hooks: create, dice,
                       editCommand, ownerComponent, lastRollComponent */ },
  "commands": [ /* §3 — the rule language lives here */ ],
  "migrations": [ /* §5 */ ],
  "tests": [ /* §6.2 — scenario fixtures with forced host-import results */ ]
}
```

storage: a `packs` table in postgres (`id`, `version`, `pack jsonb`,
`hash bytea`, `created_by`, timestamps). `PluginRuntime` grows a second
plugin kind:

```rust
enum LoadedPlugin {
    Compiled(Component),                  // today's path — unchanged
    Pack { pack: Arc<PackDoc>, /* interpreter component is shared */ },
}
```

manifest, grants, and schema registry are derived from the pack by the same
`ParsedManifest` code path (packs are *not* allowed to declare `core.*`
components, exactly like today).

getting the pack into the interpreter instance needs one WIT addition
(owner contract change, flagged for review):

```wit
// added to interface guest:
configure: func(pack: json) -> result<plugin-manifest, string>;
```

called once by the kernel right after instantiation, before any `decide`.
hand-written plugins implement it as a no-op returning their static manifest.
(alternatives considered: passing the pack in `context` on every decide,
wasteful and ugly; baking the pack into a composed component, a build step
returning through the back door. the single `configure` export is the clean
option.)

---

## 3. the rule language (owner contract #3, proposal)

design stance: a total, first-order expression language over JSON values,
with effects restricted to an enumerated set. not a general-purpose
language. if a rule can't be written in it, the answer is a hand-written
plugin, not a language extension. extensions require an owner-blessed
semantics revision, same as the `Delta` enum.

the source of truth is a JSON AST (the UI builds structures, not text).
a textual surface syntax can be layered on later purely as sugar; semantics
attach to the AST.

### 3.1 values and types

JSON values only: `null`, `bool`, `int` (i64), `decimal` (fixed-point, scale
4, no IEEE floats anywhere, for byte-identical replay), `string`,
`array`, `object`. arithmetic on `int`/`decimal` with explicit overflow →
rule error (never wraparound, never silent truncation).

### 3.2 expressions (pure)

- literals, `let` bindings, arithmetic (`+ - * / div mod`, `div_euclid`
  semantics for the D&D-style `(score - 10) / 2`), comparison, boolean
  logic (short-circuit), string concat/format.
- path access: `payload.stats.str`, `component(entity, "mothership.stats").str`
  (missing path → `null`, never a trap; explicit `require(x, "message")`
  converts `null` to a rule error).
- `cond` / `match` on literals.
- bounded collection ops only: `map`, `filter`, `fold`, `sum`, `len`, over
  arrays that already exist in the world/payload. no unbounded loops, no
  recursion, no user-defined functions in v1. (fuel metering remains the
  backstop, but the language shouldn't be able to spin in the first place.)

### 3.3 statements (a command body)

```jsonc
{
  "name": "make-save",
  "params": { /* JSON Schema for the payload — validated before eval */ },
  "authorize": "owner" | "gm" | "any" | { "expr": /* predicate */ },
  "body": [
    { "let": "stats", "expr": { "component": ["$entity", "mothership.stats"] } },
    { "if": /* cond */, "then": [ /* … */ ], "else": [ { "fail": "You can't." } ] },
    { "roll": "1d100", "as": "r" },
    { "set": ["$entity", "mothership.last-roll", { /* object expr */ }] },
    { "spawn": { "as": "e", "with": { "core.name": /* expr */ } } },
    { "remove": [...] }, { "despawn": [...] }
  ]
}
```

effects are exactly the closed set the architecture already has:

| statement | maps to |
|---|---|
| `spawn` / `despawn` / `set` / `remove` | the four `Delta` ops, accumulated in order |
| `roll`, `random`, `now` | the existing host imports, results bound to names |
| `log` | host `log` import |
| `fail "msg"` | rule error. the whole command produces no deltas (atomicity is free: deltas are only handed to the kernel on success) |

`authorize` sugar compiles to the same ownership predicate `shadowdark`
implements by hand today (`ownerComponent`-style check, GM override).

### 3.4 determinism, by construction

the language has no clock, no RNG, no I/O, no float, no iteration over
anything unordered (object iteration is key-sorted), no ambient state. every
run of `decide(cmd, world, host_results)` is a pure function. invariant #4
holds for every pack ever authored because it holds for the evaluator.
the property test for this is mechanical: fuzz packs + commands, run twice
with a scripted host, assert identical deltas.

### 3.5 derived display values

formulas the sheet shows but that aren't attested state (e.g. ability
modifiers) go in the layout as expressions in the *same* language, evaluated
client-side by a small JS evaluator that mirrors §3.2 exactly, maintained
with parity tests like `fold.js` is today. anything attested (roll results,
grand totals) stays a written component, as `shadowdark.last-roll` is now.
(option worth a spike: `jco transpile` the interpreter component and run the
real evaluator in the browser. zero-drift by definition.)

---

## 4. what the owner must bless before G1 (see §7)

1. this document's §3 semantics (value model, expression set, statement set,
   error model): contract #3.
2. the one-line WIT addition (`configure`): amendment to contract #2.
3. the `pack_format: 1` envelope (§2): it's the migration unit for packs.

---

## 5. pack evolution

- pack version bumps are content-addressed; the log's `cause.plugin`
  records `{id, version}` as today, so any record is traceable to the exact
  pack that produced it. the interpreter's own version is recorded alongside
  (`cause.plugin.id = "mothership", version = "0.3.0+interp1.2"` or a second
  field. small `Cause` discussion for the owner, or encode in version
  string to avoid touching contract #1).
- component schema changes ship with declarative migrations in the pack
  (`rename-field`, `add-field-with-default`, `drop-field`, `map-values`).
  the interpreter's `migrate` export applies them, which finally gives the
  `TODO(M6)` migrate-at-snapshot-load wiring a real consumer. per the
  existing working agreement, migrations run against component values at
  snapshot load, never against the log.

---

## 6. the authoring UI (web, new `/studio` area)

three layers, in increasing ambition. all of them read/write the same pack
JSON; none require new kernel concepts beyond §2.

### 6.1 structured editors (the workhorse)

- components: form-based field editor (name, type, range, enum, default)
  that *generates* the JSON Schema. authors never see raw JSON Schema.
- sheet layout: builder over the existing widget vocabulary (text,
  number, stat, track, list, action) with live preview using the *actual*
  `SheetRenderer.vue` against a fixture world. the renderer is already
  fully generic, so the preview is the real thing, not a mock.
- commands: block/form-based rule editor over the §3 AST. pick an
  effect, bind expressions through pickers (component fields, payload
  params, roll results). the AST-as-source-of-truth is what makes this
  tractable; there is no parsing step to get wrong.

### 6.2 the test bench (what makes generation trustworthy)

packs carry scenario tests:

```jsonc
{ "name": "crit fails drain stress",
  "world": { /* fixture entities */ },
  "command": { "name": "make-save", "actor": "…", "payload": { /* … */ } },
  "host": { "rolls": [{ "expr": "1d100", "total": 99, "detail": "…" }] },
  "expect": { "deltas": [ /* … */ ] } }        // or { "error": "…" }
```

the kernel exposes a scripted host mode (test builds / a dev-only
endpoint) where `roll`/`random`/`now` return the scenario's forced values.
this is the same trick `session_e2e` uses for auth, applied to
nondeterminism. the UI runs the pack's tests on every edit and blocks
publish on red. these tests are also exactly the harness that makes
LLM-drafted rules verifiable (§6.3).

### 6.3 AI-assisted import (front-end, not compiler)

"paste the SRD chapter" → an LLM (via the kernel, server-side) drafts pack
fragments: proposed components, commands, layout. crucially the LLM's output
is only ever pack JSON, which is then (1) schema-validated against the
pack format, (2) shown in the same structured editors for human review, and
(3) run against generated + hand-added scenario tests before it can be
published. the LLM can be wrong; it cannot be unsafe, nondeterministic, or
outside its namespace, because the target language can't express those
things.

### 6.4 publishing

draft packs live per-user; publishing writes an immutable
(`id`, `version`, `hash`) row and makes the system selectable at session
creation. existing sessions keep the pack version they started with;
upgrading a session is a GM action that runs §5 migrations at the next
snapshot load.

---

## 7. milestones

- G0, contract: owner reviews/edits/blesses §3 + §4. deliverable:
  `docs/RULES_DSL.md` (normative semantics) authored/approved by owner.
- G1, interpreter + counter parity: interpreter component (new
  `plugins/interpreter`, hand-written rust, heavy review, it is now the
  single most security-critical guest). pack loading in `PluginRuntime`,
  `configure` wiring, `packs` table. acceptance: `counter` rewritten as a
  pack passes the existing `runtime_counter.rs` tests unmodified.
- G2, shadowdark parity (the real gate): shadowdark rewritten as a
  pack; the existing `shadowdark_plugin.rs` acceptance tests pass against
  it. this is the empirical proof the language is expressive *enough*. if
  it can't express shadowdark, fix the design at G0/G1, don't ship. fuzz +
  determinism property tests (§3.4) land here.
- G3, studio structured editors: components/layout/commands editors,
  live `SheetRenderer` preview, pack validation, draft save + publish.
- G4, studio test bench: scripted-host execution, scenario editor,
  red/green gating on publish.
- G5, AI-assist: prose → draft pack, always landing in the G3 editors
  behind the G4 gate.

## 8. risks

- language scope creep: every game will want one more construct.
  mitigation: G2's parity gate defines "enough"; additions thereafter go
  through owner contract review, and the escape hatch (native plugins)
  absorbs the long tail.
- interpreter bug = every pack bugs. mitigation: it's one small,
  loop-free evaluator; fuzzed determinism tests; fuel; and the capability
  validator still checks every batch it emits. the kernel never trusted
  plugins anyway.
- two evaluators drift (guest rust vs. client JS for derived values).
  mitigation: shared golden-vector test suite generated from the rust side,
  run under vitest; or the `jco` spike to eliminate the JS one.
- pack size / configure cost: packs are KBs of JSON; `configure` is
  once per instantiation. non-issue at MVP scale; content-hash caching if it
  ever is.
