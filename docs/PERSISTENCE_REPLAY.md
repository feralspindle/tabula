# tabula: persistence & replay

status: descriptive, documenting the log/snapshot layer as built
(`kernel/src/store.rs`, `kernel/migrations/0001_init.sql`,
`crates/tabula-core`). this layer embodies invariants #3 (replay never
executes plugin code), #5 (every record has a cause), #6 (atomic records),
and #8 (the four property tests).

## the model: the log is the truth, the world is a cache

a session's authoritative state is its append-only delta log. the
in-memory `World` (and every snapshot of it) is a *projection*, a pure fold
of the log that can be discarded and rebuilt at any time:

```
world(n) = fold(apply, World::new(), records[1..=n])
```

`apply` (`crates/tabula-core/src/world.rs`) is game-agnostic and total:
it never fails, never consults a plugin, and treats ill-formed deltas as
no-ops (`Set` on a nonexistent entity doesn't implicitly spawn; `Spawn` of an
existing entity doesn't clobber). totality is what makes replay
unconditional: any log that was ever written can always be folded, even if
the plugin that produced it no longer exists.

## schema

two tables carry the entire game state (`kernel/migrations/0001_init.sql`):

```sql
log_records (
    session_id  uuid    references sessions on delete cascade,
    seq         bigint  check (seq >= 1),
    at          bigint,        -- GameTime, unix millis
    cause       jsonb,         -- { command_id, command, plugin{id,version}, actor }
    deltas      jsonb,         -- [ { op: spawn|despawn|set|remove, ... } ]
    primary key (session_id, seq)
)

snapshots (
    session_id  uuid    references sessions on delete cascade,
    upto_seq    bigint  check (upto_seq >= 0),
    world       bytea,         -- canonical JSON bytes of World (NOT jsonb)
    primary key (session_id, upto_seq)
)
```

two deliberate choices worth knowing:

- `cause`/`deltas` are `jsonb` (queryable: "every record this player
  caused" is one SQL predicate), but `world` is `bytea`. postgres `jsonb`
  normalizes key order and number formatting; storing snapshot bytes opaquely
  is what lets the byte-identical replay test compare snapshots literally.
- a record is one row. all deltas of a command live in one `deltas`
  array, so atomicity (invariant #6) is not a transaction discipline the code
  must remember. a record is physically indivisible.

## seq discipline: gapless without locks

`seq` is per-session, monotonic, gapless, starting at 1. it is assigned
*inside* the INSERT (`store.rs::append_record`):

```sql
insert into log_records (session_id, seq, ...)
values ($1, (select coalesce(max(seq), 0) + 1 from log_records where session_id = $1), ...)
returning seq
```

this is race-free because of the actor model, not because of SQL: each
live session has exactly one writer, its session actor (see
`SESSIONS_REALTIME.md`), so there is never a concurrent append for the same
session. the `(session_id, seq)` primary key is the tripwire: if the
single-writer assumption is ever violated (two actors for one session, e.g.
after a kernel split-brain), the second writer gets a loud unique-constraint
error instead of a silent gap or fork. gaplessness is property test (d),
verified against real postgres in `kernel/tests/store_db.rs`.

`GameTime.at` is wall-clock milliseconds stamped by the kernel at append. it
is *presentation* metadata (feed timestamps); ordering authority is `seq`
alone.

## canonical serialization: why replay is byte-identical

`World` is `BTreeMap<EntityId, BTreeMap<ComponentKey, Value>>`, ordered maps
all the way down, so a given state has exactly one JSON serialization.
combined with `bytea` snapshots, this yields the strongest test we have,
property (b): serialize the live projection after a recorded run, replay the
log from empty into a fresh `World`, serialize that, and the *bytes* must be
equal. any nondeterminism smuggled into `apply`, any HashMap iteration order,
any float formatting drift fails the suite immediately. (this is also why the
proposed rule language in `PLUGIN_GENERATION.md` bans IEEE floats.)

## snapshots: pure acceleration, no authority

snapshots exist only so cold loads don't fold years of log. properties:

- written every `SNAPSHOT_EVERY` records (default 200) by the session
  actor, after the record that made `seq % N == 0`. a failed snapshot write
  is logged and ignored: the log is intact, so nothing is lost.
- immutable and append-only per `(session_id, upto_seq)`;
  `on conflict do nothing` makes the write idempotent.
- disposable. `truncate snapshots` loses nothing but load time. no code
  path treats a snapshot as authoritative over the log.

cold load (`store.rs::load_world`) is: latest snapshot (or empty world at
seq 0) + fold of `load_records_after(upto_seq)`. property (a),
`fold(log) == fold(snapshot + tail)`, is tested both in-memory
(`tabula-core/tests/properties.rs`) and against postgres
(`kernel/tests/store_db.rs`).

schema evolution hook: when component schemas change across plugin
versions, the plugin's `migrate` export runs against snapshot-loaded
component values only, never against the log (working agreement; wiring
pending at `TODO(M6)` in `kernel/src/session/actor.rs`). the log stays
forever in the vocabulary it was written in; replay of old logs applies old
values and migration happens at the projection boundary.

## what is NOT persisted

- rule errors / refused commands: nothing appends on failure; the log
  records only things that happened.
- the world between snapshots: it's recomputable.
- plugin code or manifests: sessions pin `{system_plugin_id, version}`
  in their row; the component artifact lives in `PLUGIN_DIR` on disk (packs,
  when they land, will be DB rows, see `PLUGIN_GENERATION.md` §2).
- presence/connection state: WS connections are ephemeral by design.

## retention & growth (MVP posture)

the log is kept forever, that's the product (permanent, auditable campaign
history; a "session replay" feature is a UI over `load_records_after(0)`).
math check: a very active table producing 1,000 records/session-night at
~1 KB each is ~1 MB/night; snapshot rows are bounded by world size. no
compaction is planned; if it's ever needed, the only correct form is
"snapshot + truncate records below `upto_seq`", which sacrifices replay
before that point and therefore needs an explicit owner decision.
