//! session-scoped authorization checks, ported from hexmapper's authz model:
//! the session owner is the GM; membership is ownership or a session_members row.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

/// true if the user owns the session or is listed in session_members.
pub async fn is_session_member(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<bool, AppError> {
    let is_member: bool = sqlx::query_scalar(
        r#"
        select exists (
            select 1 from sessions where id = $1 and owner_id = $2
            union all
            select 1 from session_members where session_id = $1 and user_id = $2
        )
        "#,
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(is_member)
}

/// true if the user owns the session (owner = GM).
pub async fn is_session_gm(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<bool, AppError> {
    let is_gm: bool = sqlx::query_scalar(
        "select exists (select 1 from sessions where id = $1 and owner_id = $2)",
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(is_gm)
}
