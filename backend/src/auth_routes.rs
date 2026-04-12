use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use axum_login::AuthSession;
use duckdb::OptionalExt;
use serde::{Deserialize, Serialize};

use crate::{
    db::{is_initialized, set_initialized},
    AppState,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InitRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    id: String,
    username: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_workspace: Option<CurrentWorkspace>,
}

#[derive(Debug, Serialize)]
pub struct CurrentWorkspace {
    id: String,
    name: String,
    slug: String,
}

#[derive(Debug, Serialize)]
pub struct InitResponse {
    message: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    error: String,
}

pub fn build_auth_router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/check", get(check_auth))
        .route("/api/auth/init", post(init_system))
}

async fn login(
    mut auth_session: AuthSession<crate::AuthBackend>,
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, Response> {
    let user = auth_session
        .authenticate((req.username, req.password))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Authentication error: {}", e),
                }),
            )
                .into_response()
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid username or password".to_string(),
                }),
            )
                .into_response()
        })?;

    auth_session.login(&user).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to create session: {}", e),
            }),
        )
            .into_response()
    })?;

    let current_workspace = if let Some(ref workspace_id) = user.current_workspace_id {
        let conn = state.db.lock().await;
        let workspace_info: Option<(String, String)> = conn
            .query_row(
                "SELECT name, COALESCE(slug, id) FROM workspaces WHERE id = ? AND deleted_at IS NULL",
                duckdb::params![workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .ok()
            .flatten();
        drop(conn);

        workspace_info.map(|(name, slug)| CurrentWorkspace {
            id: workspace_id.clone(),
            name,
            slug,
        })
    } else {
        None
    };

    Ok(Json(LoginResponse {
        id: user.id,
        username: user.username,
        role: user.role,
        current_workspace,
    }))
}

async fn logout(mut auth_session: AuthSession<crate::AuthBackend>) -> impl IntoResponse {
    // Always return 204 NO_CONTENT, even if session is already deleted/expired
    // The end state (user logged out) is correct regardless of deletion result
    let _ = auth_session.logout().await;
    StatusCode::NO_CONTENT
}

async fn check_auth(
    auth_session: AuthSession<crate::AuthBackend>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match auth_session.user {
        Some(user) => {
            let current_workspace = if let Some(ref workspace_id) = user.current_workspace_id {
                let conn = state.db.lock().await;
                let workspace_info: Option<(String, String)> = conn
                    .query_row(
                        "SELECT name, COALESCE(slug, id) FROM workspaces WHERE id = ? AND deleted_at IS NULL",
                        duckdb::params![workspace_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .ok()
                    .flatten();
                drop(conn);

                workspace_info.map(|(name, slug)| CurrentWorkspace {
                    id: workspace_id.clone(),
                    name,
                    slug,
                })
            } else {
                None
            };

            Json(LoginResponse {
                id: user.id,
                username: user.username,
                role: user.role,
                current_workspace,
            })
            .into_response()
        }
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn init_system(
    State(state): State<AppState>,
    Json(req): Json<InitRequest>,
) -> Result<impl IntoResponse, Response> {
    // Validate password complexity
    if let Err(e) = crate::validate_password_complexity(&req.password) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid password: {}", e),
            }),
        )
            .into_response());
    }

    // Hash password BEFORE beginning transaction to avoid long-running transaction
    // Bcrypt with cost=12 can take ~500ms, during which the DB lock would be held
    let password_hash = crate::hash_password(&req.password).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to hash password: {}", e),
            }),
        )
            .into_response()
    })?;

    let conn = state.db.lock().await;

    // Use transaction to prevent TOCTOU race condition
    // Ensures check and insert are atomic operations
    conn.execute("BEGIN TRANSACTION", []).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to begin transaction: {}", e),
            }),
        )
            .into_response()
    })?;

    // Check if system is already initialized (within transaction)
    let already_initialized = is_initialized(&conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to check initialization status: {}", e),
            }),
        )
            .into_response()
    })?;

    if already_initialized {
        conn.execute("ROLLBACK", []).ok();
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "System already initialized".to_string(),
            }),
        )
            .into_response());
    }

    // Check if username already exists (prevent duplicates)
    let user_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM users WHERE username = ?",
            duckdb::params![&req.username],
            |row| row.get(0),
        )
        .map_err(|e| {
            conn.execute("ROLLBACK", []).ok();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to check if username exists: {}", e),
                }),
            )
                .into_response()
        })?;

    if user_exists > 0 {
        conn.execute("ROLLBACK", []).ok();
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!("Username '{}' already exists", req.username),
            }),
        )
            .into_response());
    }

    // Create admin user using pre-hashed password
    use chrono::Utc;
    let created_at = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let user_id = uuid::Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO users (id, username, password_hash, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        duckdb::params![&user_id, &req.username, &password_hash, "admin", &created_at,],
    )
    .map_err(|e| {
        conn.execute("ROLLBACK", []).ok();
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to create admin user: {}", e),
            }),
        )
            .into_response()
    })?;

    let workspace_id = uuid::Uuid::new_v4().to_string();
    let workspace_name = crate::workspace::make_personal_workspace_name(&req.username);
    let workspace_slug =
        crate::workspace::workspace_slug_base_from_name_or_id(&workspace_name, &workspace_id);

    conn.execute(
        "INSERT INTO workspaces (id, name, slug, owner_id, is_personal, created_at) VALUES (?, ?, ?, ?, TRUE, ?)",
        duckdb::params![&workspace_id, &workspace_name, &workspace_slug, &user_id, &created_at],
    )
    .map_err(|e| {
        conn.execute("ROLLBACK", []).ok();
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to create personal workspace: {}", e),
            }),
        )
            .into_response()
    })?;

    conn.execute(
        "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?)",
        duckdb::params![&workspace_id, &user_id, &created_at],
    )
    .map_err(|e| {
        conn.execute("ROLLBACK", []).ok();
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to add user to workspace: {}", e),
            }),
        )
            .into_response()
    })?;

    conn.execute(
        "UPDATE users SET current_workspace_id = ? WHERE id = ?",
        duckdb::params![&workspace_id, &user_id],
    )
    .map_err(|e| {
        conn.execute("ROLLBACK", []).ok();
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to set current workspace: {}", e),
            }),
        )
            .into_response()
    })?;

    tracing::info!(user_id = %user_id, username = %req.username, workspace_id = %workspace_id, "Admin user and personal workspace created");

    // Mark system as initialized
    set_initialized(&conn).map_err(|e| {
        conn.execute("ROLLBACK", []).ok();
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to mark system as initialized: {}", e),
            }),
        )
            .into_response()
    })?;

    // Commit transaction
    conn.execute("COMMIT", []).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to commit transaction: {}", e),
            }),
        )
            .into_response()
    })?;

    Ok(Json(InitResponse {
        message: "System initialized successfully".to_string(),
    }))
}
