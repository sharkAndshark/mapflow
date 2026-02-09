use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use axum_login::AuthSession;
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
    username: String,
    role: String,
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

    Ok(Json(LoginResponse {
        username: user.username,
        role: user.role,
    }))
}

async fn logout(mut auth_session: AuthSession<crate::AuthBackend>) -> impl IntoResponse {
    if auth_session.logout().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::NO_CONTENT
}

async fn check_auth(auth_session: AuthSession<crate::AuthBackend>) -> impl IntoResponse {
    match auth_session.user {
        Some(user) => Json(LoginResponse {
            username: user.username,
            role: user.role,
        })
        .into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn init_system(
    State(state): State<AppState>,
    Json(req): Json<InitRequest>,
) -> Result<impl IntoResponse, Response> {
    // 验证密码复杂度
    if let Err(e) = crate::validate_password_complexity(&req.password) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid password: {}", e),
            }),
        )
            .into_response());
    }

    let conn = state.db.lock().await;

    // 检查系统是否已初始化
    if is_initialized(&conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to check initialization status: {}", e),
            }),
        )
            .into_response()
    })? {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "System already initialized".to_string(),
            }),
        )
            .into_response());
    }

    // 创建管理员用户
    let password_hash = crate::hash_password(&req.password).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to hash password: {}", e),
            }),
        )
            .into_response()
    })?;

    // 使用当前的UTC时间作为created_at
    use chrono::Utc;
    let created_at = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let user_id = uuid::Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO users (id, username, password_hash, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        duckdb::params![&user_id, &req.username, &password_hash, "admin", &created_at,],
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to create admin user: {}", e),
            }),
        )
            .into_response()
    })?;

    // 标记系统为已初始化
    set_initialized(&conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to mark system as initialized: {}", e),
            }),
        )
            .into_response()
    })?;

    drop(conn);

    Ok(Json(InitResponse {
        message: "System initialized successfully".to_string(),
    }))
}
