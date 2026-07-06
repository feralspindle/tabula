//! session REST endpoints: create, list, detail (with plugin manifest for the
//! schema-driven renderer), and join-by-URL. the realtime work happens over
//! the WS endpoint in `session::ws`.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::authz;
use crate::error::AppError;
use crate::session::ws;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sessions", post(create_session).get(list_sessions))
        .route("/sessions/{id}", get(session_detail))
        .route("/sessions/{id}/join", post(join_session))
        .route("/sessions/{id}/ws", get(ws::ws_handler))
        .route("/plugins", get(list_plugins))
}

#[derive(Deserialize)]
struct CreateSession {
    name: String,
    system_plugin_id: String,
}

#[derive(Serialize, sqlx::FromRow)]
struct SessionSummary {
    id: Uuid,
    owner_id: Uuid,
    name: String,
    system_plugin_id: String,
    system_plugin_version: String,
    is_gm: bool,
}

async fn create_session(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateSession>,
) -> Result<Json<SessionSummary>, AppError> {
    let name = body.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(AppError::BadRequest("name must be 1–120 chars".into()));
    }
    let plugin = state.runtime().get(&body.system_plugin_id).ok_or_else(|| {
        AppError::BadRequest(format!("unknown plugin {:?}", body.system_plugin_id))
    })?;
    if plugin.manifest.plugin_type != crate::runtime::PluginType::System {
        return Err(AppError::BadRequest(
            "sessions require a System plugin".into(),
        ));
    }

    let row: SessionSummary = sqlx::query_as(
        r#"
        insert into sessions (owner_id, name, system_plugin_id, system_plugin_version)
        values ($1, $2, $3, $4)
        returning id, owner_id, name, system_plugin_id, system_plugin_version, true as is_gm
        "#,
    )
    .bind(user.user_id)
    .bind(name)
    .bind(&plugin.manifest.id)
    .bind(&plugin.manifest.version)
    .fetch_one(state.pool())
    .await?;

    tracing::info!(session = %row.id, owner = %user.user_id, plugin = %row.system_plugin_id, "session created");
    Ok(Json(row))
}

async fn list_sessions(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<SessionSummary>>, AppError> {
    let rows: Vec<SessionSummary> = sqlx::query_as(
        r#"
        select s.id, s.owner_id, s.name, s.system_plugin_id, s.system_plugin_version,
               (s.owner_id = $1) as is_gm
        from sessions s
        where s.owner_id = $1
           or exists (select 1 from session_members m where m.session_id = s.id and m.user_id = $1)
        order by s.created_at desc
        "#,
    )
    .bind(user.user_id)
    .fetch_all(state.pool())
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize)]
struct SessionDetail {
    id: Uuid,
    owner_id: Uuid,
    name: String,
    system_plugin_id: String,
    system_plugin_version: String,
    is_gm: bool,
    members: Vec<Member>,
    /// parsed plugin manifest: component schemas, commands, sheet layout -
    /// everything the generic sheet renderer needs.
    manifest: Value,
}

#[derive(Serialize, sqlx::FromRow)]
struct Member {
    user_id: Uuid,
    display_name: String,
}

async fn session_detail(
    State(state): State<AppState>,
    user: AuthUser,
    Path(session_id): Path<Uuid>,
) -> Result<Json<SessionDetail>, AppError> {
    if !authz::is_session_member(state.pool(), user.user_id, session_id).await? {
        return Err(AppError::Forbidden);
    }

    let row = crate::session::load_session_row(state.pool(), session_id).await?;
    let plugin = state
        .runtime()
        .get(&row.system_plugin_id)
        .ok_or_else(|| AppError::Plugin(format!("plugin {} not loaded", row.system_plugin_id)))?;

    let members: Vec<Member> = sqlx::query_as(
        "select user_id, display_name from session_members where session_id = $1 order by joined_at",
    )
    .bind(session_id)
    .fetch_all(state.pool())
    .await?;

    Ok(Json(SessionDetail {
        id: row.id,
        owner_id: row.owner_id,
        is_gm: row.owner_id == user.user_id,
        name: row.name,
        system_plugin_id: row.system_plugin_id,
        system_plugin_version: row.system_plugin_version,
        members,
        manifest: serde_json::to_value(&plugin.manifest)
            .map_err(|e| AppError::Plugin(e.to_string()))?,
    }))
}

/// join-by-URL: knowing the session UUID grants membership. prototype
/// carryover from hexmapper, slated for replacement by revocable invite
/// tokens before any public exposure (docs/AUTH_SECURITY.md).
async fn join_session(
    State(state): State<AppState>,
    user: AuthUser,
    Path(session_id): Path<Uuid>,
) -> Result<Json<SessionSummary>, AppError> {
    let row = crate::session::load_session_row(state.pool(), session_id).await?;

    sqlx::query(
        r#"
        insert into session_members (session_id, user_id, display_name)
        values ($1, $2, $3)
        on conflict (session_id, user_id) do update set display_name = excluded.display_name
        "#,
    )
    .bind(session_id)
    .bind(user.user_id)
    .bind(&user.display_name)
    .execute(state.pool())
    .await?;

    Ok(Json(SessionSummary {
        id: row.id,
        owner_id: row.owner_id,
        is_gm: row.owner_id == user.user_id,
        name: row.name,
        system_plugin_id: row.system_plugin_id,
        system_plugin_version: row.system_plugin_version,
    }))
}

#[derive(Serialize)]
struct PluginSummary {
    id: String,
    version: String,
    plugin_type: crate::runtime::PluginType,
}

async fn list_plugins(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<Json<Vec<PluginSummary>>, AppError> {
    let mut plugins: Vec<PluginSummary> = state
        .runtime()
        .plugins()
        .map(|p| PluginSummary {
            id: p.manifest.id.clone(),
            version: p.manifest.version.clone(),
            plugin_type: p.manifest.plugin_type,
        })
        .collect();
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(plugins))
}
