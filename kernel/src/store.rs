//! postgres persistence for the delta log and snapshots.
//!
//! seq discipline: each active session has exactly one writer (its session
//! actor), so `coalesce(max(seq),0) + 1` inside the INSERT is race-free in
//! practice; the `(session_id, seq)` primary key turns any violation of that
//! assumption into a hard unique-constraint error rather than a gap or fork.

use sqlx::{PgPool, Row};
use uuid::Uuid;

use tabula_core::{Cause, Delta, GameTime, LogRecord, World};

use crate::error::AppError;

/// appends one record, assigning the next gapless seq. returns the stored record.
pub async fn append_record(
    pool: &PgPool,
    session_id: Uuid,
    at: GameTime,
    cause: &Cause,
    deltas: &[Delta],
) -> Result<LogRecord, AppError> {
    let cause_json = serde_json::to_value(cause).map_err(internal)?;
    let deltas_json = serde_json::to_value(deltas).map_err(internal)?;

    let row = sqlx::query(
        r#"
        insert into log_records (session_id, seq, at, cause, deltas)
        values (
            $1,
            (select coalesce(max(seq), 0) + 1 from log_records where session_id = $1),
            $2, $3, $4
        )
        returning seq
        "#,
    )
    .bind(session_id)
    .bind(at.0 as i64)
    .bind(&cause_json)
    .bind(&deltas_json)
    .fetch_one(pool)
    .await?;

    let seq: i64 = row.get("seq");
    Ok(LogRecord {
        seq: seq as u64,
        at,
        cause: cause.clone(),
        deltas: deltas.to_vec(),
    })
}

/// records with `seq > after_seq`, in seq order. `after_seq = 0` loads everything.
pub async fn load_records_after(
    pool: &PgPool,
    session_id: Uuid,
    after_seq: u64,
) -> Result<Vec<LogRecord>, AppError> {
    let rows = sqlx::query(
        r#"
        select seq, at, cause, deltas from log_records
        where session_id = $1 and seq > $2
        order by seq
        "#,
    )
    .bind(session_id)
    .bind(after_seq as i64)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(LogRecord {
                seq: row.get::<i64, _>("seq") as u64,
                at: GameTime(row.get::<i64, _>("at") as u64),
                cause: serde_json::from_value(row.get("cause")).map_err(internal)?,
                deltas: serde_json::from_value(row.get("deltas")).map_err(internal)?,
            })
        })
        .collect()
}

pub async fn max_seq(pool: &PgPool, session_id: Uuid) -> Result<u64, AppError> {
    let seq: i64 =
        sqlx::query_scalar("select coalesce(max(seq), 0) from log_records where session_id = $1")
            .bind(session_id)
            .fetch_one(pool)
            .await?;
    Ok(seq as u64)
}

pub async fn save_snapshot(
    pool: &PgPool,
    session_id: Uuid,
    upto_seq: u64,
    world: &World,
) -> Result<(), AppError> {
    let bytes = serde_json::to_vec(world).map_err(internal)?;
    sqlx::query(
        r#"
        insert into snapshots (session_id, upto_seq, world)
        values ($1, $2, $3)
        on conflict (session_id, upto_seq) do nothing
        "#,
    )
    .bind(session_id)
    .bind(upto_seq as i64)
    .bind(bytes)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn latest_snapshot(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Option<(u64, World)>, AppError> {
    let row = sqlx::query(
        r#"
        select upto_seq, world from snapshots
        where session_id = $1
        order by upto_seq desc
        limit 1
        "#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        let upto_seq = row.get::<i64, _>("upto_seq") as u64;
        let world: World =
            serde_json::from_slice(&row.get::<Vec<u8>, _>("world")).map_err(internal)?;
        Ok((upto_seq, world))
    })
    .transpose()
}

/// cold load: latest snapshot + tail fold. returns the projection and the seq it
/// reflects. replay never executes plugin code, this is a pure fold.
pub async fn load_world(pool: &PgPool, session_id: Uuid) -> Result<(World, u64), AppError> {
    let (mut world, from_seq) = match latest_snapshot(pool, session_id).await? {
        Some((seq, world)) => (world, seq),
        None => (World::new(), 0),
    };

    let tail = load_records_after(pool, session_id, from_seq).await?;
    let mut last_seq = from_seq;
    for record in &tail {
        tabula_core::apply_record(&mut world, record);
        last_seq = record.seq;
    }
    Ok((world, last_seq))
}

fn internal(e: serde_json::Error) -> AppError {
    AppError::Plugin(format!("serialization: {e}"))
}
