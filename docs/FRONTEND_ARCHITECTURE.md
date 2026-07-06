# tabula: frontend architecture, or how a plugin becomes a UI

status: descriptive, documenting the frontend as built (`web/src`). the
layout language here is renderer-defined (not one of the three irreversible
contracts), but plugins depend on it, so changes must stay additive. related
docs: `PLUGIN_BOUNDARY.md` (kernel⟷plugin), `PLUGIN_GENERATION.md` (authoring
proposal; packs reuse this layout language verbatim).

## the principle: the frontend is game-blind

`web/src` contains zero game-specific components. there is no
`ShadowdarkSheet.vue` and there never will be one. everything the UI knows
about a game system arrives at runtime as data, over two channels:

1. the manifest (static, per system): REST `GET /sessions/{id}` returns
   the session detail including the plugin's parsed manifest: component
   schemas, command declarations, and above all the `sheet_layout` JSON.
   the layout is the plugin's *rendering program*; the frontend is its
   interpreter.
2. the world (dynamic, per session): the websocket delivers a snapshot
   (`entity → component-key → value`) and then a stream of log records that
   the client folds locally.

a new game system therefore requires shipping exactly one artifact (the
plugin) and zero frontend deploys. that is the acceptance test for every
frontend change: *would this still render a system we've never seen?*

```
  plugin manifest ──REST /sessions/{id}──▶ sessionStore.layout ──▶ SheetRenderer / views
                                                                        │ interprets
  delta log ──WS snapshot+records──▶ fold.js ──▶ sessionStore.world ────┘ reads
                                                                        │ user acts
  kernel  ◀──────────── WS { type:"command", id, name, payload } ───────┘ emits
```

## the unidirectional loop (no client-side writes, ever)

the frontend never mutates `world` in response to user input. every edit,
roll, create, and action becomes a command frame; state changes only ever
arrive back as record frames, which `lib/fold.js` (a line-for-line JS mirror
of `tabula-core::apply`, kept honest by parity unit tests) folds into the
projection. consequences:

- all clients converge identically, including the issuer. there is no
  optimistic-update reconciliation problem because there are no optimistic
  updates; on a LAN-speed round trip the record is back before the input
  blurs.
- command → response correlation is by uuid: `sendCommand` registers the
  command id in a pending map (10s timeout); the promise resolves when a
  record arrives whose `cause.command_id` matches, or rejects on an `error`
  frame (rule errors are sent to the issuing client only, so other players
  never see your refused command).
- reconnect is trivial: the store remembers `seq` and reconnects with
  `?after_seq=N`; the kernel replays the tail; records with `seq <= known`
  are deduped. exponential backoff 1s→10s.

## the layout language

`sheet_layout` is one JSON object. top-level keys, all consumed generically:

| key | consumed by | meaning |
|---|---|---|
| `title` | SessionView | heading for the sheet list panel |
| `nameComponent` | sessionStore | sheet discovery: any entity carrying this component key *is* a sheet (list label = its value) |
| `ownerComponent` | SheetRenderer, sessionStore | component whose `user_id` field marks the owner → drives client-side editability (`canEdit = isGm ∨ owner == me`) |
| `lastRollComponent` | sessionStore | feed harvest: any `set` of this key in an incoming record becomes a roll entry in the activity feed |
| `create` | SessionView | `{ command, label, fields:[{name,label,type,required}] }`: renders the "new sheet" form and names the command it submits |
| `dice` | DiceRoller | `{ command, exprArg }`: wires the free-form dice roller to a plugin command |
| `editCommand` | SheetRenderer | command used by all editable widgets (default `update-sheet-field`) |
| `sections` | SheetRenderer | the sheet body: `[{ label, fields:[…] }]` |

each field in a section binds one widget to one location in the world:

```jsonc
{
  "widget":   "stat",                 // vocabulary below
  "label":    "STR",
  "component": "shadowdark.stats",    // which component on the sheet entity
  "field":    "str",                  // path within it ("" = whole value)
  "maxField": "max",                  // track only: sibling field for the max
  "roll":     { "command": "roll-check", "args": { "stat": "str" } },  // click-to-roll
  "action":   { "command": "spend-luck", "label": "Spend" }            // side button
}
```

### widget vocabulary (the frontend's closed enum)

| widget | renders | edit path |
|---|---|---|
| `text` | labeled input | commit-on-change → `editCommand {entity, component, field, value}` |
| `number` | numeric input | same, value coerced to number |
| `stat` | big value + derived modifier, click-to-roll | inline input below; `roll.command` with `roll.args` + current advantage |
| `track` | current / max pair | two commits, `field` and `maxField` |
| `list` | array of `{name, qty}` rows with add/remove | commits the whole array |
| *(any field)* `action` | small command button on the label | `action.command`, empty payload + entity |

this vocabulary is deliberately the frontend analogue of the closed `Delta`
enum: small, generic, grown additively. a game need is met by composing
widgets or (rarely) adding one to the vocabulary, never by a game-specific
component. unknown widget names currently fall through to the text input,
which is the forward-compatibility story: an old client renders a new
widget's data as raw text rather than breaking.

cross-cutting renderer behavior derived from the layout, not coded per game:

- the advantage selector (DIS / — / ADV) appears automatically on any
  section containing rollable fields, and its state is appended to every
  `roll` command payload as `advantage`.
- editability disables inputs (and hides add/remove affordances) for
  non-owners, but the kernel-side plugin remains the actual enforcer.
  `canEdit` is UX, not security (see `PLUGIN_BOUNDARY.md`; the intruder tests
  in `shadowdark_plugin.rs` prove the server refuses regardless).

## worked example: the shadowdark layout

the plugin declares (abridged from `plugins/shadowdark/src/lib.rs`):

```jsonc
{
  "title": "Shadowdark Character",
  "nameComponent": "core.name",
  "ownerComponent": "shadowdark.owner",
  "lastRollComponent": "shadowdark.last-roll",
  "create": { "command": "create-character", "label": "New Character",
              "fields": [ { "name": "name", "label": "Name", "type": "text", "required": true },
                          { "name": "class", "label": "Class", "type": "text" },
                          { "name": "ancestry", "label": "Ancestry", "type": "text" } ] },
  "dice": { "command": "roll-dice", "exprArg": "expr" },
  "editCommand": "update-sheet-field",
  "sections": [
    { "label": "Abilities",
      "fields": [ { "widget": "stat", "label": "STR", "component": "shadowdark.stats",
                    "field": "str", "roll": { "command": "roll-check", "args": { "stat": "str" } } },
                  /* dex, con, int, wis, cha likewise */ ] },
    { "label": "Vitals",
      "fields": [ { "widget": "track", "label": "HP", "component": "shadowdark.hp",
                    "field": "current", "maxField": "max" },
                  { "widget": "number", "label": "Luck", "component": "shadowdark.luck",
                    "field": "tokens", "action": { "command": "spend-luck", "label": "Spend" } } ] },
    { "label": "Gear",
      "fields": [ { "widget": "list", "label": "Items", "component": "shadowdark.inventory",
                    "field": "items" } ] }
  ]
}
```

from this single document the frontend derives: the sheet list (entities with
`core.name`), the "New Character" form, the whole character sheet with
rollable ability blocks and the advantage selector, the HP track, the luck
spend button, inventory editing, the free dice roller, ownership-gated
inputs, and the roll feed (every `shadowdark.last-roll` set, with the
attested breakdown the host's `roll` import produced). none of those words
appear in `web/src`.

## session lifecycle (store's responsibility)

`sessionStore.enter(id)`:
1. `POST /sessions/{id}/join`: idempotent. (interim: knowing the session
   UUID currently grants membership, a prototype carryover slated for
   replacement by real invite tokens; see `AUTH_SECURITY.md`.)
2. `GET /sessions/{id}`: detail: name, members, `is_gm`, manifest (layout).
3. open WS with the supabase access token (`?token=`); receive `snapshot
   { seq, world }`, then live `record` frames → fold → reactive re-render.

pinia reactivity does the rest: widgets are computed views over
`world[entityId][component]`, so a record folding in updates every affected
widget on every client with no diffing logic of our own.

## known deviation from the principle (to fix, not to keep)

`SheetRenderer.statModifier` hardcodes `floor((value − 10) / 2)`, a D&D-ism
that happens to fit shadowdark. it is display-only (the authoritative
modifier is computed inside the plugin during `roll-check`), but it is still
game logic in the game-blind layer, and it will be wrong for the first system
whose modifiers work differently. the planned fix is the derived-value
expressions of `PLUGIN_GENERATION.md` §3.5: the layout gains an optional
`derived` expression per field (same expression language as rule packs),
evaluated client-side by a parity-tested evaluator; the hardcoded formula
then becomes shadowdark's declared expression. until then, treat
`statModifier` as a placeholder with a known expiry.

## growth path (post-MVP surfaces, same pattern)

maps/tokens, notebooks, and chat (all deferred) should enter the same way
the sheet did: a new layout surface (e.g. a `map` top-level key naming
the components that carry position/appearance) interpreted by a new generic
renderer, with all mutations expressed as plugin commands and all state as
components folded from the log. the test stays the same: if a feature
requires the frontend to know a game's name, it's designed wrong.
