//! store integration tests, require a real Postgres with the migrations
//! applied. skipped (pass trivially) when DATABASE_URL is unset so `cargo test`
//! works offline; CI and local runs against Supabase exercise them for real.

use uuid::Uuid;

use tabula_core::{Actor, Cause, Delta, EntityId, GameTime, World};
use tabula_kernel::store;

async fn pool_or_skip() -> Option<sqlx::PgPool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping store DB tests");
        return None;
    };
    Some(
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect to DATABASE_URL"),
    )
}

async fn make_session(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar(
        r#"
        insert into sessions (owner_id, name, system_plugin_id, system_plugin_version)
        values (gen_random_uuid(), '__store_db_test', 'test', '0')
        returning id
        "#,
    )
    .fetch_one(pool)
    .await
    .expect("insert test session")
}

fn cause() -> Cause {
    Cause {
        command_id: Uuid::new_v4(),
        command: Some("test".into()),
        plugin: None,
        actor: Actor::System,
    }
}

/// invariant 8d: seq is gapless per session, starting at 1, and survives a
/// round-trip through the log unchanged.
#[tokio::test]
async fn seq_is_gapless_and_roundtrips() {
    let Some(pool) = pool_or_skip().await else {
        return;
    };
    let session = make_session(&pool).await;

    let mut appended = Vec::new();
    for i in 0..25u64 {
        let e = EntityId::new();
        let deltas = vec![Delta::Spawn { entity: e }];
        let rec = store::append_record(&pool, session, GameTime(i * 10), &cause(), &deltas)
            .await
            .expect("append");
        appended.push(rec);
    }

    for (i, rec) in appended.iter().enumerate() {
        assert_eq!(rec.seq, i as u64 + 1, "seq must be gapless from 1");
    }

    let loaded = store::load_records_after(&pool, session, 0)
        .await
        .expect("load");
    assert_eq!(loaded, appended, "log round-trip must be lossless");

    sqlx::query("delete from sessions where id = $1")
        .bind(session)
        .execute(&pool)
        .await
        .expect("cleanup");
}

/// invariant 8a at the store level: cold load (snapshot + tail) equals a full
/// fold of the log.
#[tokio::test]
async fn snapshot_plus_tail_matches_full_fold() {
    let Some(pool) = pool_or_skip().await else {
        return;
    };
    let session = make_session(&pool).await;

    let mut live = World::new();
    for i in 0..30u64 {
        let e = EntityId(Uuid::from_u128(0xABC0 + i as u128));
        let deltas = vec![Delta::Spawn { entity: e }];
        let rec = store::append_record(&pool, session, GameTime(i), &cause(), &deltas)
            .await
            .expect("append");
        tabula_core::apply_record(&mut live, &rec);

        if rec.seq == 17 {
            store::save_snapshot(&pool, session, rec.seq, &live)
                .await
                .expect("snapshot");
        }
    }

    let (cold, last_seq) = store::load_world(&pool, session).await.expect("load_world");
    assert_eq!(last_seq, 30);
    assert_eq!(
        serde_json::to_vec(&cold).unwrap(),
        serde_json::to_vec(&live).unwrap(),
        "snapshot + tail must be byte-identical to the live projection"
    );

    sqlx::query("delete from sessions where id = $1")
        .bind(session)
        .execute(&pool)
        .await
        .expect("cleanup");
}
