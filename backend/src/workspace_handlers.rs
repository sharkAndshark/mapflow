use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use axum_login::AuthSession;
use chrono::Utc;
use duckdb::OptionalExt;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use crate::{
    models::ErrorResponse,
    workspace::{
        generate_deleted_workspace_name, validate_workspace_name, CurrentWorkspaceResponse,
        WorkspaceMemberWithInfo, WorkspaceResponse, WorkspaceWithMemberCount,
    },
    AppState, AuthBackend, User,
};

type WorkspaceRow = (String, String, String, bool, Option<String>, String);

type ApiResult<T> = Result<T, Response>;

fn err(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
        .into_response()
}

fn bad_req(message: &str) -> Response {
    err(StatusCode::BAD_REQUEST, message)
}

fn not_found(message: &str) -> Response {
    err(StatusCode::NOT_FOUND, message)
}

fn forbidden(message: &str) -> Response {
    err(StatusCode::FORBIDDEN, message)
}

fn conflict(message: &str) -> Response {
    err(StatusCode::CONFLICT, message)
}

fn internal_err<E: std::fmt::Debug>(e: E) -> Response {
    tracing::error!(error = ?e, "Internal server error");
    err(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
}

fn is_unique_constraint_error(err: &duckdb::Error) -> bool {
    let err_msg = err.to_string();
    err_msg.contains("UNIQUE") || err_msg.contains("unique")
}

fn workspace_name_conflict_or_internal(err: duckdb::Error) -> Response {
    if is_unique_constraint_error(&err) {
        return bad_req("工作空间名称已被使用");
    }
    internal_err(err)
}

#[allow(clippy::result_large_err)]
fn with_detached_workspace_members<F>(
    conn: &duckdb::Connection,
    workspace_id: &str,
    mut op: F,
) -> ApiResult<()>
where
    F: FnMut(&duckdb::Connection) -> ApiResult<()>,
{
    let mut member_stmt = conn
        .prepare("SELECT user_id, joined_at FROM workspace_members WHERE workspace_id = ?")
        .map_err(internal_err)?;
    let members: Result<Vec<(String, String)>, _> = member_stmt
        .query_map(duckdb::params![workspace_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(internal_err)?
        .collect();
    let members = members.map_err(internal_err)?;

    for (member_user_id, joined_at) in &members {
        conn.execute(
            "INSERT INTO workspace_member_backups (workspace_id, user_id, joined_at) VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
            duckdb::params![workspace_id, member_user_id, joined_at],
        )
        .map_err(internal_err)?;
    }

    conn.execute(
        "DELETE FROM workspace_members WHERE workspace_id = ?",
        duckdb::params![workspace_id],
    )
    .map_err(internal_err)?;

    if let Err(err) = op(conn) {
        for (member_user_id, joined_at) in &members {
            let _ = conn.execute(
                "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?)",
                duckdb::params![workspace_id, member_user_id, joined_at],
            );
        }
        return Err(err);
    }

    for (member_user_id, joined_at) in &members {
        conn.execute(
            "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?)",
            duckdb::params![workspace_id, member_user_id, joined_at],
        )
        .map_err(internal_err)?;
    }

    conn.execute(
        "DELETE FROM workspace_member_backups WHERE workspace_id = ?",
        duckdb::params![workspace_id],
    )
    .map_err(internal_err)?;

    Ok(())
}

async fn require_user(auth_session: &AuthSession<AuthBackend>) -> ApiResult<User> {
    auth_session
        .user
        .clone()
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Not authenticated"))
}

fn parse_workspace_row(row: WorkspaceRow) -> Result<crate::workspace::Workspace, String> {
    let (id, name, owner_id, is_personal, deleted_at_str, created_at_str) = row;
    let deleted_at = deleted_at_str
        .map(|s| {
            chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.and_utc())
                .map_err(|e| format!("Failed to parse deleted_at: {}", e))
        })
        .transpose()?;

    let created_at = chrono::NaiveDateTime::parse_from_str(&created_at_str, "%Y-%m-%d %H:%M:%S")
        .map_err(|e| format!("Failed to parse created_at: {}", e))?
        .and_utc();

    Ok(crate::workspace::Workspace {
        id,
        name,
        owner_id,
        is_personal,
        deleted_at,
        created_at,
    })
}

fn archived_workspace_original_name(name: &str, workspace_id: &str) -> String {
    let archived_suffix = format!("_deleted_{}", workspace_id);
    name.strip_suffix(&archived_suffix)
        .unwrap_or(name)
        .to_string()
}

pub async fn list_workspaces(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let mut stmt = conn
        .prepare(
            r"
            SELECT w.id, w.name, w.owner_id, w.is_personal, w.deleted_at, w.created_at,
                   (SELECT COUNT(*) FROM workspace_members WHERE workspace_id = w.id) as member_count
            FROM workspaces w
            JOIN workspace_members wm ON w.id = wm.workspace_id
            WHERE wm.user_id = ? AND w.deleted_at IS NULL
            ORDER BY w.is_personal DESC, w.created_at ASC
            ",
        )
        .map_err(internal_err)?;

    let rows: Result<Vec<_>, _> = stmt
        .query_map(duckdb::params![&user.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(internal_err)?
        .collect();

    let rows = rows.map_err(internal_err)?;

    let workspaces: Result<Vec<WorkspaceWithMemberCount>, String> = rows
        .into_iter()
        .map(
            |(id, name, owner_id, is_personal, _deleted_at_str, created_at_str, member_count)| {
                let created_at =
                    chrono::NaiveDateTime::parse_from_str(&created_at_str, "%Y-%m-%d %H:%M:%S")
                        .map_err(|e| format!("Failed to parse created_at: {}", e))?
                        .and_utc();

                Ok(WorkspaceWithMemberCount {
                    id,
                    name,
                    owner_id,
                    is_personal,
                    member_count,
                    created_at,
                })
            },
        )
        .collect();

    let workspaces = workspaces.map_err(internal_err)?;

    Ok(Json(workspaces))
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
}

pub async fn create_workspace(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let name = validate_workspace_name(&req.name).map_err(|e| bad_req(&e))?;

    let conn = state.db.lock().await;

    let existing: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM workspaces WHERE name = ?",
            duckdb::params![&name],
            |row| row.get(0),
        )
        .map_err(internal_err)?;

    if existing > 0 {
        return Err(conflict("工作空间名称已被使用"));
    }

    let workspace_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(internal_err)?;

    let insert_result = conn.execute(
        "INSERT INTO workspaces (id, name, owner_id, is_personal, created_at) VALUES (?, ?, ?, FALSE, ?)",
        duckdb::params![&workspace_id, &name, &user.id, &now],
    );

    if let Err(e) = insert_result {
        conn.execute_batch("ROLLBACK").ok();
        let err_msg = e.to_string();
        if err_msg.contains("UNIQUE") || err_msg.contains("unique") {
            return Err(conflict("工作空间名称已被使用"));
        }
        return Err(internal_err(e));
    }

    let member_result = conn.execute(
        "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?)",
        duckdb::params![&workspace_id, &user.id, &now],
    );

    if let Err(e) = member_result {
        conn.execute_batch("ROLLBACK").ok();
        return Err(internal_err(e));
    }

    conn.execute_batch("COMMIT").map_err(internal_err)?;

    info!(workspace_id = %workspace_id, name = %name, owner_id = %user.id, "Workspace created");

    Ok((
        StatusCode::CREATED,
        Json(WorkspaceResponse {
            id: workspace_id,
            name,
            owner_id: user.id,
            is_personal: false,
        }),
    ))
}

pub async fn get_workspace(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(workspace_id): AxumPath<String>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let deleted_at: Option<Option<String>> = conn
        .query_row(
            "SELECT deleted_at FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal_err)?;

    let deleted_at = deleted_at.ok_or_else(|| not_found("Workspace not found"))?;
    if deleted_at.is_some() {
        return Err(not_found("Workspace not found"));
    }

    let is_member = check_workspace_membership(&conn, &workspace_id, &user.id)?;
    if !is_member {
        return Err(forbidden("Not a member of this workspace"));
    }

    let row: Option<WorkspaceRow> = conn
        .query_row(
            "SELECT id, name, owner_id, is_personal, deleted_at, created_at FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(internal_err)?;

    let row = row.ok_or_else(|| not_found("Workspace not found"))?;

    let workspace = parse_workspace_row(row).map_err(internal_err)?;

    Ok(Json(workspace))
}

pub async fn list_members(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(workspace_id): AxumPath<String>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let deleted_at: Option<Option<String>> = conn
        .query_row(
            "SELECT deleted_at FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal_err)?;

    let deleted_at = deleted_at.ok_or_else(|| not_found("Workspace not found"))?;
    if deleted_at.is_some() {
        return Err(not_found("Workspace not found"));
    }

    let is_member = check_workspace_membership(&conn, &workspace_id, &user.id)?;

    if !is_member {
        return Err(forbidden("Not a member of this workspace"));
    }

    let owner_id: String = conn
        .query_row(
            "SELECT owner_id FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| row.get(0),
        )
        .map_err(internal_err)?;

    let mut stmt = conn
        .prepare(
            r"
            SELECT wm.user_id, u.username, wm.joined_at
            FROM workspace_members wm
            JOIN users u ON wm.user_id = u.id
            WHERE wm.workspace_id = ?
            ORDER BY wm.joined_at ASC
            ",
        )
        .map_err(internal_err)?;

    let rows: Result<Vec<_>, _> = stmt
        .query_map(duckdb::params![&workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(internal_err)?
        .collect();

    let rows = rows.map_err(internal_err)?;

    let members: Vec<WorkspaceMemberWithInfo> = rows
        .into_iter()
        .map(|(user_id, username, joined_at_str)| {
            let joined_at =
                chrono::NaiveDateTime::parse_from_str(&joined_at_str, "%Y-%m-%d %H:%M:%S")
                    .map(|dt| dt.and_utc())
                    .unwrap_or_else(|_| Utc::now());

            WorkspaceMemberWithInfo {
                user_id: user_id.clone(),
                username,
                joined_at,
                is_owner: user_id == owner_id,
            }
        })
        .collect();

    Ok(Json(members))
}

#[derive(Debug, Deserialize)]
pub struct InviteMemberRequest {
    pub username: String,
}

pub async fn invite_member(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(workspace_id): AxumPath<String>,
    Json(req): Json<InviteMemberRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;
    let username = req.username.trim();

    if username.is_empty() {
        return Err(bad_req("用户名不能为空"));
    }

    let is_member = check_workspace_membership(&conn, &workspace_id, &user.id)?;

    if !is_member {
        return Err(forbidden("Not a member of this workspace"));
    }

    let workspace_deleted: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM workspaces WHERE id = ? AND deleted_at IS NOT NULL",
            duckdb::params![&workspace_id],
            |row| row.get(0),
        )
        .map_err(internal_err)?;

    if workspace_deleted > 0 {
        return Err(not_found("Workspace not found"));
    }

    let target_user: Option<(String, String)> = conn
        .query_row(
            "SELECT id, username FROM users WHERE username = ?",
            duckdb::params![username],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(internal_err)?;

    let (target_user_id, target_username) = target_user.ok_or_else(|| not_found("用户不存在"))?;

    let already_member: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
            duckdb::params![&workspace_id, &target_user_id],
            |row| row.get(0),
        )
        .map_err(internal_err)?;

    if already_member > 0 {
        return Err(conflict("用户已在该工作空间中"));
    }

    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    conn.execute(
        "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?)",
        duckdb::params![&workspace_id, &target_user_id, &now],
    )
    .map_err(internal_err)?;

    info!(workspace_id = %workspace_id, invited_user_id = %target_user_id, invited_by = %user.id, "Member invited");

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "userId": target_user_id,
            "username": target_username
        })),
    ))
}

pub async fn remove_member(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath((workspace_id, member_user_id)): AxumPath<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let workspace_info: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT owner_id, deleted_at FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(internal_err)?;

    let (owner_id, deleted_at) = workspace_info.ok_or_else(|| not_found("Workspace not found"))?;

    if deleted_at.is_some() {
        return Err(not_found("Workspace not found"));
    }

    if member_user_id == owner_id {
        return Err(forbidden("不能移除工作空间所有者"));
    }

    let is_owner = user.id == owner_id;
    let is_self = user.id == member_user_id;

    if !is_owner && !is_self {
        return Err(forbidden("只有工作空间所有者可以移除成员"));
    }

    let rows_affected = conn
        .execute(
            "DELETE FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
            duckdb::params![&workspace_id, &member_user_id],
        )
        .map_err(internal_err)?;

    if rows_affected == 0 {
        return Err(not_found("成员不存在"));
    }

    info!(workspace_id = %workspace_id, removed_user_id = %member_user_id, removed_by = %user.id, "Member removed");

    Ok(StatusCode::NO_CONTENT)
}

pub async fn leave_workspace(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(workspace_id): AxumPath<String>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let workspace_info: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT owner_id, deleted_at FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(internal_err)?;

    let (owner_id, deleted_at) = workspace_info.ok_or_else(|| not_found("Workspace not found"))?;

    if deleted_at.is_some() {
        return Err(not_found("Workspace not found"));
    }

    if user.id == owner_id {
        return Err(bad_req("工作空间所有者不能离开，只能删除工作空间"));
    }

    let rows_affected = conn
        .execute(
            "DELETE FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
            duckdb::params![&workspace_id, &user.id],
        )
        .map_err(internal_err)?;

    if rows_affected == 0 {
        return Err(not_found("您不是该工作空间的成员"));
    }

    info!(workspace_id = %workspace_id, user_id = %user.id, "User left workspace");

    Ok(StatusCode::NO_CONTENT)
}

#[allow(clippy::result_large_err)]
fn check_workspace_membership(
    conn: &duckdb::Connection,
    workspace_id: &str,
    user_id: &str,
) -> ApiResult<bool> {
    let is_member: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM workspace_members wm JOIN workspaces w ON wm.workspace_id = w.id WHERE wm.workspace_id = ? AND wm.user_id = ? AND w.deleted_at IS NULL",
            duckdb::params![workspace_id, user_id],
            |row| row.get(0),
        )
        .map_err(internal_err)?;

    Ok(is_member > 0)
}

#[derive(Debug, Deserialize)]
pub struct SwitchWorkspaceRequest {
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
}

pub async fn switch_workspace(
    mut auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    Json(req): Json<SwitchWorkspaceRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let workspace_info: Option<(String, bool)> = conn
        .query_row(
            "SELECT name, is_personal FROM workspaces WHERE id = ? AND deleted_at IS NULL",
            duckdb::params![&req.workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(internal_err)?;

    let (name, is_personal) = workspace_info.ok_or_else(|| not_found("Workspace not found"))?;

    let is_member: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
            duckdb::params![&req.workspace_id, &user.id],
            |row| row.get(0),
        )
        .map_err(internal_err)?;

    if is_member == 0 {
        return Err(forbidden("不属于该工作空间"));
    }

    conn.execute(
        "UPDATE users SET current_workspace_id = ? WHERE id = ?",
        duckdb::params![&req.workspace_id, &user.id],
    )
    .map_err(internal_err)?;

    let updated_user = crate::User {
        id: user.id.clone(),
        username: user.username.clone(),
        password_hash: user.password_hash.clone(),
        role: user.role.clone(),
        current_workspace_id: Some(req.workspace_id.clone()),
    };

    auth_session.login(&updated_user).await.map_err(|e| {
        tracing::error!(error = ?e, "Failed to update session");
        internal_err("Failed to update session")
    })?;

    Ok(Json(CurrentWorkspaceResponse {
        id: req.workspace_id,
        name,
        is_personal,
    }))
}

#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceRequest {
    pub name: String,
}

pub async fn update_workspace(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(workspace_id): AxumPath<String>,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let name = validate_workspace_name(&req.name).map_err(|e| bad_req(&e))?;
    let conn = state.db.lock().await;

    let workspace_info: Option<(String, bool, Option<String>)> = conn
        .query_row(
            "SELECT owner_id, is_personal, deleted_at FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(internal_err)?;

    let (owner_id, is_personal, deleted_at) =
        workspace_info.ok_or_else(|| not_found("Workspace not found"))?;

    if user.id != owner_id {
        return Err(forbidden("Only workspace owner can update workspace"));
    }

    if deleted_at.is_some() {
        return Err(not_found("Workspace not found"));
    }

    if is_personal {
        return Err(bad_req("Cannot rename personal workspace"));
    }

    let existing: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM workspaces WHERE name = ? AND id != ? AND deleted_at IS NULL",
            duckdb::params![&name, &workspace_id],
            |row| row.get(0),
        )
        .map_err(internal_err)?;

    if existing > 0 {
        return Err(bad_req("工作空间名称已被使用"));
    }

    with_detached_workspace_members(&conn, &workspace_id, |conn| {
        conn.execute(
            "UPDATE workspaces SET name = ? WHERE id = ?",
            duckdb::params![&name, &workspace_id],
        )
        .map_err(workspace_name_conflict_or_internal)?;
        Ok(())
    })?;

    info!(workspace_id = %workspace_id, name = %name, "Workspace updated");

    Ok(Json(json!({ "id": workspace_id, "name": name })))
}

pub async fn delete_workspace(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(workspace_id): AxumPath<String>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let workspace_info: Option<(String, bool, String, Option<String>)> = conn
        .query_row(
            "SELECT owner_id, is_personal, name, deleted_at FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(internal_err)?;

    let (owner_id, is_personal, name, deleted_at) =
        workspace_info.ok_or_else(|| not_found("Workspace not found"))?;

    if user.id != owner_id {
        return Err(forbidden("Only workspace owner can delete workspace"));
    }

    if is_personal {
        return Err(forbidden("Cannot delete personal workspace"));
    }

    if deleted_at.is_some() {
        return Ok(StatusCode::NO_CONTENT);
    }

    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let archived_name = generate_deleted_workspace_name(&name, &workspace_id);

    with_detached_workspace_members(&conn, &workspace_id, |conn| {
        conn.execute_batch("BEGIN TRANSACTION")
            .map_err(internal_err)?;

        conn.execute(
                "DELETE FROM published_files WHERE file_id IN (SELECT id FROM files WHERE workspace_id = ?)",
                duckdb::params![&workspace_id],
            )
            .map_err(|err| {
                conn.execute_batch("ROLLBACK").ok();
                internal_err(err)
            })?;

        conn.execute(
            "UPDATE files SET is_public = FALSE WHERE workspace_id = ?",
            duckdb::params![&workspace_id],
        )
        .map_err(|err| {
            conn.execute_batch("ROLLBACK").ok();
            internal_err(err)
        })?;

        conn.execute(
            "UPDATE workspaces SET deleted_at = ?, name = ? WHERE id = ?",
            duckdb::params![&now, &archived_name, &workspace_id],
        )
        .map_err(|err| {
            conn.execute_batch("ROLLBACK").ok();
            internal_err(err)
        })?;

        conn.execute_batch("COMMIT").map_err(|err| {
            conn.execute_batch("ROLLBACK").ok();
            internal_err(err)
        })?;

        Ok(())
    })?;

    info!(workspace_id = %workspace_id, deleted_by = %user.id, "Workspace deleted (soft)");

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct RestoreWorkspaceRequest {
    pub name: Option<String>,
}

pub async fn restore_workspace(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(workspace_id): AxumPath<String>,
    Json(req): Json<RestoreWorkspaceRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let workspace_info: Option<(String, bool, String, Option<String>)> = conn
        .query_row(
            "SELECT owner_id, is_personal, name, deleted_at FROM workspaces WHERE id = ?",
            duckdb::params![&workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(internal_err)?;

    let (owner_id, _is_personal, current_name, deleted_at) =
        workspace_info.ok_or_else(|| not_found("Workspace not found"))?;

    if user.id != owner_id {
        return Err(forbidden("Only workspace owner can restore workspace"));
    }

    if deleted_at.is_none() {
        return Err(bad_req("工作空间未删除"));
    }

    if let Some(new_name) = &req.name {
        let name = validate_workspace_name(new_name).map_err(|e| bad_req(&e))?;

        let existing: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workspaces WHERE name = ? AND id != ? AND deleted_at IS NULL",
                duckdb::params![&name, &workspace_id],
                |row| row.get(0),
            )
            .map_err(internal_err)?;

        if existing > 0 {
            return Err(bad_req("工作空间名称已被使用"));
        }

        with_detached_workspace_members(&conn, &workspace_id, |conn| {
            conn.execute(
                "UPDATE workspaces SET name = ?, deleted_at = NULL WHERE id = ?",
                duckdb::params![&name, &workspace_id],
            )
            .map_err(workspace_name_conflict_or_internal)?;
            Ok(())
        })?;

        info!(workspace_id = %workspace_id, name = %name, "Workspace restored with new name");

        Ok(Json(json!({ "id": workspace_id, "name": name })))
    } else {
        let restored_name = archived_workspace_original_name(&current_name, &workspace_id);

        let existing: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workspaces WHERE name = ? AND id != ? AND deleted_at IS NULL",
                duckdb::params![&restored_name, &workspace_id],
                |row| row.get(0),
            )
            .map_err(internal_err)?;

        if existing > 0 {
            return Err(bad_req("工作空间名称已被使用"));
        }

        with_detached_workspace_members(&conn, &workspace_id, |conn| {
            conn.execute(
                "UPDATE workspaces SET name = ?, deleted_at = NULL WHERE id = ?",
                duckdb::params![&restored_name, &workspace_id],
            )
            .map_err(workspace_name_conflict_or_internal)?;
            Ok(())
        })?;

        info!(workspace_id = %workspace_id, "Workspace restored");

        Ok(Json(json!({ "id": workspace_id, "name": restored_name })))
    }
}

pub async fn list_archived_workspaces(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let mut stmt = conn
        .prepare(
            r"
            SELECT w.id, w.name, w.owner_id, w.is_personal, w.deleted_at, w.created_at
            FROM workspaces w
            JOIN workspace_members wm ON w.id = wm.workspace_id
            WHERE wm.user_id = ? AND w.deleted_at IS NOT NULL
            ORDER BY w.deleted_at DESC
            ",
        )
        .map_err(internal_err)?;

    let rows: Result<Vec<_>, _> = stmt
        .query_map(duckdb::params![&user.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(internal_err)?
        .collect();

    let rows = rows.map_err(internal_err)?;

    let workspaces: Result<Vec<serde_json::Value>, String> = rows
        .into_iter()
        .map(
            |(id, name, owner_id, is_personal, deleted_at_str, created_at_str)| {
                let deleted_at = deleted_at_str
                    .map(|s| {
                        chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                            .map(|dt| dt.and_utc())
                            .map_err(|e| format!("Failed to parse deleted_at: {}", e))
                    })
                    .transpose()?;

                let created_at =
                    chrono::NaiveDateTime::parse_from_str(&created_at_str, "%Y-%m-%d %H:%M:%S")
                        .map_err(|e| format!("Failed to parse created_at: {}", e))?
                        .and_utc();
                let display_name = archived_workspace_original_name(&name, &id);

                Ok(json!({
                    "id": id,
                    "name": display_name,
                    "ownerId": owner_id,
                    "isPersonal": is_personal,
                    "deletedAt": deleted_at,
                    "createdAt": created_at
                }))
            },
        )
        .collect();

    let workspaces = workspaces.map_err(internal_err)?;

    Ok(Json(workspaces))
}

pub async fn get_current_workspace(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
) -> ApiResult<impl IntoResponse> {
    let user = require_user(&auth_session).await?;
    let conn = state.db.lock().await;

    let workspace_info: Option<(String, String, bool)> =
        if let Some(current_workspace_id) = &user.current_workspace_id {
            conn.query_row(
                r"
            SELECT w.id, w.name, w.is_personal
            FROM workspaces w
            JOIN workspace_members wm ON w.id = wm.workspace_id
            WHERE w.id = ? AND wm.user_id = ? AND w.deleted_at IS NULL
            LIMIT 1
            ",
                duckdb::params![current_workspace_id, &user.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(internal_err)?
        } else {
            None
        };

    let workspace_info = match workspace_info {
        Some(info) => Some(info),
        None => conn
            .query_row(
                r"
                SELECT w.id, w.name, w.is_personal
                FROM workspaces w
                JOIN workspace_members wm ON w.id = wm.workspace_id
                WHERE wm.user_id = ? AND w.deleted_at IS NULL
                ORDER BY w.is_personal DESC, w.created_at ASC
                LIMIT 1
                ",
                duckdb::params![&user.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(internal_err)?,
    };

    match workspace_info {
        Some((id, name, is_personal)) => Ok(Json(CurrentWorkspaceResponse {
            id,
            name,
            is_personal,
        })),
        None => Err(not_found("No workspace found")),
    }
}
