-- Tabula MVP schema: sessions/membership + the delta log and snapshots.
-- owner_id / user_id reference Supabase auth.users(id); no FK because auth lives
-- in a separate schema managed by Supabase.

create table if not exists sessions (
    id                     uuid primary key default gen_random_uuid(),
    owner_id               uuid not null,
    name                   text not null,
    system_plugin_id       text not null,
    system_plugin_version  text not null,
    created_at             timestamptz not null default now()
);

create index if not exists sessions_owner_idx on sessions (owner_id);

create table if not exists session_members (
    session_id    uuid not null references sessions(id) on delete cascade,
    user_id       uuid not null,
    display_name  text not null default '',
    joined_at     timestamptz not null default now(),
    primary key (session_id, user_id)
);

create index if not exists session_members_user_idx on session_members (user_id);

-- The append-only delta log. seq is per-session, monotonic, gapless, starting
-- at 1; the composite primary key turns any seq race into a loud unique
-- violation instead of a silent gap or fork.
create table if not exists log_records (
    session_id  uuid not null references sessions(id) on delete cascade,
    seq         bigint not null check (seq >= 1),
    at          bigint not null,          -- GameTime, unix millis
    cause       jsonb not null,
    deltas      jsonb not null,
    created_at  timestamptz not null default now(),
    primary key (session_id, seq)
);

-- Periodic full-projection snapshots so cold loads are snapshot + tail.
-- world is the serialized World (canonical JSON bytes), opaque to SQL.
create table if not exists snapshots (
    session_id  uuid not null references sessions(id) on delete cascade,
    upto_seq    bigint not null check (upto_seq >= 0),
    world       bytea not null,
    created_at  timestamptz not null default now(),
    primary key (session_id, upto_seq)
);
