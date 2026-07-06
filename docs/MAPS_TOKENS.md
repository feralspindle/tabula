# tabula: maps & tokens layer

status: proposal, post-MVP. elaborates the spec's deferred "hex map,
dungeon mapper" line. constrained throughout by the standing invariants: the
`Delta` enum stays closed, replay never executes plugin code, all rules live
in plugins, and the frontend stays game-blind (`FRONTEND_ARCHITECTURE.md`'s
growth rule: a new layout surface, not a game-specific renderer). two pieces
need owner blessing before implementation: the core spatial component
schemas (§2, they extend the host-owned `core.*` vocabulary) and the
ephemeral channel addition to the WS protocol (§5).

## 1. the stance: maps are world state, rendered generically

a map is not a new subsystem. it is entities with spatial components,
mutated by plugin commands, folded from the same log, drawn by a
generic canvas the layout configures. everything already true of sheets
stays true of maps:

- token positions are component values → map history is replayable for free
  (a campaign's every movement is in the log; "replay the ambush" is a UI
  feature, not new persistence).
- movement legality is a *rule* → it lives in `decide`, not in the kernel
  and not in the canvas. speed limits, terrain costs, "you can't move
  through walls," zone-of-control: all plugin logic, all optional.
- the map canvas is the third generic renderer (after SheetRenderer and the
  dice/feed widgets): it knows *grids and images*, never *games*.

what IS new: core spatial vocabulary (§2), an asset store for images (§4),
an ephemeral channel for non-state interactions (§5), and a visibility
question the broadcast architecture makes genuinely hard (§6).

## 2. core spatial components (host-owned schemas, like `core.name`)

spatial interop shouldn't be reinvented per game system. a token's position
means the same thing in shadowdark and CoC, and cross-system tooling (the
canvas, future Module map packs) needs one vocabulary. following the
`core.name` precedent exactly: host-registered schemas, granted to System
plugins, written only through plugin commands (invariant #7 intact):

```jsonc
// core.map — this entity IS a map/scene
{ "name": "The Gullet — Level 2",
  "grid": { "kind": "square" | "hex-pointy" | "hex-flat" | "none",
            "size": 5, "unit": "ft" },          // size = world units per cell
  "bounds": { "cols": 40, "rows": 30 },          // optional; gridless maps omit
  "background": { "asset": "sha256:…", "px_per_cell": 70 } }   // §4

// core.position — this entity is ON a map
{ "map": "<map entity uuid>",
  "x": 12, "y": 7,          // square: col/row. hex: axial q/r. gridless: world units
  "facing": 90 }             // optional, degrees

// core.token — how to draw it
{ "image": { "asset": "sha256:…" },   // or { "text": "G3" } / { "shape": "circle", "color": "#…" }
  "scale": 1.0,                        // cells occupied (2.0 = large creature)
  "tint": "#a33",                      // optional
  "badge": "7/11" }                    // optional small label (e.g. HP), plugin-maintained
```

design notes:

- `core.position.map` points at a map entity: multiple maps coexist in
  one session (dungeon level 1 and 2; the overworld hex map and a battle
  grid), and moving a token between maps is one `Set`. the active map per
  viewer is client UI state, with a layout-declared default and a GM
  "focus everyone here" ephemeral (§5).
- coordinates are integers in grid space (axial q/r for hex, the
  hexmapper convention), decimals only for `grid.kind = none`. integer
  positions keep deltas canonical and diff-friendly.
- a sheet entity and its token are the same entity. Wilhelmina is one
  uuid carrying `core.name` + `shadowdark.*` + `core.position` +
  `core.token`. click the token → open the sheet; the badge shows her HP.
  entities that are only scenery (a boulder, a door marker) just carry
  spatial components and no sheet. the existing "sheet = has
  nameComponent" discovery rule is unaffected.
- grants: the spatial trio is granted to System plugins alongside
  `core.name`. Module plugins (when implemented) get them too, since
  placing content on maps is the Module use case.

these schemas are owner-contract adjacent: they extend the host-owned
`core.*` vocabulary that every plugin and the canvas will depend on.
additive-only evolution, blessed at V0 (§8).

## 3. commands and the movement loop

the kernel gains no built-in commands. plugins declare them, packs get
stock templates (`PLUGIN_GENERATION.md`), and the layout's new `map` surface
(§7) tells the canvas which commands to emit:

- `move-token { entity, map, x, y }`: the workhorse. a permissive system's
  implementation is three lines (authorize owner-or-GM, `Set core.position`);
  a tactical system checks speed/terrain/turn order and refuses with a rule
  error, which surfaces as the drag snapping back with the message. the
  stock pack template is the permissive version.
- `place-token`, `remove-token`: GM-facing; spawn-with-components or
  strip spatial components (the entity and its sheet survive leaving the
  map: `Remove core.position`, not `Despawn`).
- `create-map`, `update-map`: GM-facing map lifecycle.

write-frequency discipline: commit on drop, preview on the ephemeral
channel. a drag emits ephemeral ghost positions (§5) so other players see
the token in motion, and exactly one `move-token` command when it lands.
one drag = one record = one log entry. the log stays a history of *moves*,
not mouse samples; snapshot cadence and broadcast volume are untouched. a
40-move combat round is 40 records, noise-free and replayable as an
animation.

## 4. assets (the world stays JSON; bytes live elsewhere)

map backgrounds and token art are the first binary content in the system.
the world and log never carry blobs; components reference assets by
content hash:

- kernel asset store: `POST /sessions/{id}/assets` (member-uploaded,
  size/type-limited, rate-limited) → kernel hashes the bytes → stores at
  `sha256:…` (disk or S3-compatible; `ASSET_DIR`/`ASSET_URL` config) →
  returns the ref. `GET /assets/{hash}` serves immutably
  (`Cache-Control: immutable`, the hash is the cache key forever).
- content addressing is the same discipline everywhere: an asset ref in
  a component is verifiable and permanent, so replay N years later renders
  the same map. the refs are already CID-shaped for the distribution
  layer (`DISTRIBUTION_ATPROTO_IPFS.md`): a Module map pack ships assets as
  IPFS content and the components reference them identically.
- dedup is free (same bytes → same ref). garbage collection (assets no
  component references) is an offline maintenance job, never load-bearing.

## 5. the ephemeral channel (new protocol surface, owner review)

maps introduce interactions that are *real-time but not state*: drag ghosts,
pings ("look HERE"), cursor presence, measurement-tape previews, GM
"focus this map." logging them would poison the log with noise; not sharing
them makes the table feel dead. the WS protocol gains one frame pair:

```jsonc
// client → server
{ "type": "ephemeral", "kind": "ping" | "drag" | "cursor" | "focus" | …, "payload": { … } }
// server → all other members (sender-attributed, never echoed back)
{ "type": "ephemeral", "from": "<user uuid>", "kind": …, "payload": { … } }
```

hard rules that keep this honest:

- never persisted, never folded, never in `world`, no seq. a client
  that misses an ephemeral missed nothing recoverable, by definition.
- relayed, not interpreted: the kernel validates size + rate and fans
  out; payloads are opaque to it. the layout/canvas defines meaning. (the
  session actor is not involved: ephemera fan out at the WS layer via a
  parallel broadcast channel, so they can't queue behind `decide`.)
- aggressively rate-limited per user (cursor/drag kinds coalesced to
  ~10 Hz server-side); dropped under pressure. correctness never depends
  on delivery.
- GM-only kinds (`focus`) are enforced by the connection's `is_gm`, the one
  piece of interpretation the kernel does.

this is a protocol addition (like `configure` was a WIT addition): small,
but wire contracts deserve the same review discipline.

## 6. visibility & fog of war: the honest hard problem

the session model broadcasts every record to every member and hands every
joiner the full world (`SESSIONS_REALTIME.md`). that is *correct* for the
current scope and *incompatible with secrets*: a hidden token that reaches
the client at all is discoverable by anyone who opens devtools. there are
two coherent postures, and the MVP of this layer takes the first:

tier 1 (this proposal): cosmetic hiding. `core.token.hidden: true`.
the canvas doesn't render it for non-GMs, the sheet list can filter it.
cheap, fully consistent with the architecture, and adequate for tables of
friends who trust each other not to open devtools. the doc and UI must not
oversell it: it is "minimized," not "secret."

tier 2 (deferred, needs an owner decision): redacted projections. true
secrecy requires per-role filtering at the kernel boundary. the sketch that
preserves the most architecture: records keep their `seq` and `cause` for
everyone (seq stays gapless client-side), but deltas touching
GM-only entities are stripped from frames sent to non-GM connections;
snapshots are filtered the same way. the genuinely hard part is *reveal*:
when a hidden entity becomes visible, non-GM clients lack its accumulated
state, so the kernel must send a synthetic entity-refresh (a mini-snapshot
of that entity) outside the fold, which breaks the "client world is a pure
fold of received records" property that `fold.js` parity currently
guarantees, and complicates the byte-identical-replay story *for clients*
(the server-side log and replay remain untouched). that trade, pure client
fold vs. real secrecy, is an architectural decision, not an implementation
detail: it belongs to the owner, and nothing in tier 1 forecloses either
answer.

line-of-sight fog (auto-revealing based on token vision) is tier 2's UI,
computed client-side for tier 1 GMs as a display aid only.

## 7. the frontend surface

per the growth rule, the layout gains a `map` top-level key and the app
gains one generic component:

```jsonc
"map": {
  "mapComponent": "core.map",
  "positionComponent": "core.position",
  "tokenComponent": "core.token",
  "moveCommand": "move-token",
  "placeCommand": "place-token",       // optional; hides the affordance if absent
  "removeCommand": "remove-token",
  "mapCommands": { "create": "create-map", "update": "update-map" }
}
```

MapCanvas (Pixi or bare canvas; SVG won't survive hundreds of tokens
panning at 60 fps. benchmark at V2, decide once):

- renders the active map entity's background + grid (square, both hex
  orientations, none), pan/zoom, cell-snapped or free drag per grid kind.
- tokens = entities whose `positionComponent.map` is the active map. drag →
  ephemeral ghosts → `moveCommand` on drop; a rule error animates the snap-
  back with the plugin's message. click → select; if the entity is a sheet,
  open it in the sheet pane (same uuid, no linkage table).
- draws pings/cursors/ghosts from ephemeral frames with sender attribution.
- reads ownership exactly as SheetRenderer does (`ownerComponent` + GM);
  the plugin remains the enforcer.

reactivity is already paid for: a `move-token` record folds into the store
and the canvas re-renders the one moved token, on every client, through the
exact pipeline sheet edits use today. hexmapper parity is the acceptance
target: the shadowdark plugin + a hex `core.map` must reproduce
hexmapper's hex-crawl view (this is the layer where the ancestor project is
finally superseded).

## 8. milestones

- V0, contract: owner blesses the `core.map`/`core.position`/
  `core.token` schemas and the ephemeral frame pair. grants wiring
  (trio granted like `core.name`, small `manifest.rs` change).
- V1, assets: content-addressed store, upload/serve endpoints, limits.
  independent of everything else; useful the moment it exists (token art on
  sheets).
- V2, the loop: MapCanvas with square grids: place, drag, drop,
  `move-token` in the shadowdark plugin (permissive template), rule-error
  snap-back. the full command loop on a map, end to end, with tests
  mirroring `shadowdark_plugin.rs` (a `Rig` that moves tokens and asserts
  refusals).
- V3, hex + parity: both hex orientations, axial coordinates,
  hexmapper-parity session on the shadowdark plugin.
- V4, presence: ephemeral channel: drag ghosts, pings, cursors, GM
  focus. (ordered after the state loop deliberately: ephemera polish the
  experience; the loop is the product.)
- V5, visibility: tier 1 `hidden` + GM-side vision aids; the tier 2
  redaction decision goes to the owner with V2-V4 experience in hand.
- later: measurement/rulers (ephemeral + grid math), map drawing
  tools (freehand walls as map-entity components), Module map packs via
  the distribution layer.

## 9. risks

- canvas performance is the only genuinely new engineering risk in the
  stack (everything else is established patterns). mitigate by choosing the
  renderer with a 500-token benchmark at V2, and by the ephemeral channel
  keeping drag traffic out of the state pipeline.
- `core.*` vocabulary creep: spatial components will tempt every
  future feature into `core.`. rule of thumb: `core.*` is for concepts the
  *generic renderers* need; anything only rules care about belongs to the
  plugin's namespace.
- oversold hiding (tier 1): mitigated by explicit UI language and this
  doc; the tier 2 path exists when it matters.
- asset storage growth: bounded by upload limits and per-session
  quotas; GC exists but is never urgent (orphaned assets cost disk, not
  correctness).
