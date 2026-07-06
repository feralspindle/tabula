# tabula: sessions & realtime

status: descriptive, documenting the session runtime as built
(`kernel/src/session/*`). the end-to-end behavior is pinned by
`kernel/tests/session_e2e.rs` (real Router, real postgres, real WS clients,
real JWTs).

## the actor model: one session, one writer

every live session is exactly one tokio task, its **session actor**, which
exclusively owns the session's moving parts:

```
                 ┌──────────────── SessionActor (one per live session) ────────────────┐
 WS conn A ──┐   │  World (Arc<Mutex>)      ← the live projection                      │
 WS conn B ──┼──▶│  PluginInstance          ← wasmtime store, NOT Send-shared          │
 WS conn C ──┘   │  SchemaRegistry + Grants ← derived from the manifest at spawn       │
   mpsc mailbox  │  last_seq                                                           │
   (cap 64)      │        │ handle_command: decide → validate → append → fold          │
                 └────────┼─────────────────────────────────────────────────────────── ┘
                          ▼ broadcast::channel (cap 256)
                 every subscribed WS connection receives ServerFrame::Record
```

why an actor rather than shared state + locks:

- the single-writer guarantee is load-bearing for persistence: the
  gapless-seq INSERT in `store.rs` is race-free *only because* one task does
  all appends for a session (`PERSISTENCE_REPLAY.md`).
- commands serialize naturally. two players' commands interleave at the
  mailbox, not mid-decide; each `decide` sees a consistent world.
- the wasmtime `Store` is single-threaded state anyway; owning it in one
  task avoids a mutex around every plugin call.

messages (`SessionMsg`): `Command { input, reply }` (oneshot reply with the
appended record or the error), `Snapshot { reply }` (clone of the live world
+ seq, for joins and lag recovery), `Tail { after_seq, reply }` (DB read of
records after a seq, for reconnects).

### lifecycle

`SessionRegistry::get_or_spawn` (keyed by session id) spawns the actor on
first contact: load the session row → resolve the pinned plugin → build
registry/grants from its manifest → `load_world` (snapshot + tail, pure fold)
→ instantiate the plugin → serve the mailbox. when every `SessionHandle`
drops, the mailbox closes and the actor exits; the registry respawns on next
contact (state is all in postgres, so actor death is always recoverable,
worst case a client reconnects and refolds).

### the one loop (`handle_command`)

1. command name must be declared in the manifest (else BadRequest).
2. `decide(cmd, context)`: plugin proposes deltas; the actor holds no
   world lock across this call (the plugin reads via host imports that lock
   per-read).
3. empty proposal → `Rule("command produced no changes")`. a no-op is not a
   log entry.
4. `validate_deltas`: capability/schema/consistency gate, all-or-none.
5. build `Cause` (command id, name, plugin ref, `Actor::User` or
   `Actor::GmOverride` per the connection's authz), invariant #5.
6. `append_record`: the gapless INSERT.
7. fold into the live world with the same `apply_record` replay uses.
8. snapshot if `seq % SNAPSHOT_EVERY == 0` (failure logged, not fatal).
9. broadcast `ServerFrame::Record` to all subscribers.

failures at steps 1-6 reply only to the issuing connection; the log and the
other players never see them.

## wire protocol

JSON text frames (`kernel/src/session/protocol.rs`), tag field `type`:

```jsonc
// client → server (the only client frame)
{ "type": "command", "id": "<uuid, client-minted>", "name": "roll-check", "payload": { … } }

// server → client
{ "type": "snapshot", "seq": 42, "world": { "<entity>": { "<component>": value } } }
{ "type": "record",   "record": { "seq": 43, "at": …, "cause": { … }, "deltas": [ … ] } }
{ "type": "error",    "command_id": "<uuid|null>", "message": "…" }   // issuer only
```

success is *not* answered directly: the issuing client learns its command
landed the same way everyone else does (the broadcast record) and
correlates via `record.cause.command_id`. only failures produce a direct
`error` frame. this keeps a single ordered state stream per client with no
"my own echo" special case.

## connection handshake (`kernel/src/session/ws.rs`)

`GET /api/sessions/{id}/ws?token=<jwt>[&after_seq=N]`

1. verify the JWT from the query param (browsers can't set headers on WS
   upgrades, see `AUTH_SECURITY.md` for the trade-off), rate-limit check,
   then membership check (`Forbidden` for strangers) and GM determination,
   all *before* upgrading.
2. on upgrade: subscribe to the broadcast first, then catch up. fresh
   join → one `snapshot` frame; reconnect (`after_seq=N`) → the record tail
   from the DB. subscription-before-catchup means a record landing during
   catch-up is buffered, not lost; the client's seq dedup (`record.seq <=
   known` is dropped) handles the overlap. this gapless handoff is asserted
   in `session_e2e.rs`.
3. then the two-way pump: incoming command frames (per-message rate limit,
   parse, forward to the actor mailbox) and outgoing broadcast records.

slow consumers: the broadcast channel buffers 256 records; a consumer
that lags off the end gets `RecvError::Lagged` and is force-resynced with a
fresh `snapshot` frame rather than disconnected. correctness never depends on
a client keeping up, snapshots are always available.

GM status is evaluated at connect time and rides the connection (it feeds
`actor_is_gm` on every command). MVP simplification: owner = GM, immutable
per session, so there is no revocation window to worry about.

## client mirror (`web/src/stores/sessionStore.js`)

the frontend implements the same discipline in reverse: fold every `record`
into a local world with `fold.js` (parity-tested mirror of `apply`), dedup by
seq, reconnect with `after_seq`, exponential backoff 1s→10s, and a pending
map from command id → promise (10 s timeout) settled by matching record or
error frame. details in `FRONTEND_ARCHITECTURE.md`.

## failure matrix

| failure | blast radius | recovery |
|---|---|---|
| rule error / validation reject | issuing client only | user retries; nothing logged |
| plugin trap / fuel exhaustion | issuing command fails (500-class to issuer) | actor keeps serving; instance state unchanged (world lives host-side) |
| WS drop | one client | reconnect + `after_seq` tail |
| slow client lags broadcast | one client | forced snapshot resync |
| actor panic / kernel restart | session's live connections | respawn on next contact; `load_world` refolds from postgres |
| snapshot write failure | none (logged) | next cadence point retries |
| postgres down | appends fail; commands error to issuers | log intact; resume when DB returns |
