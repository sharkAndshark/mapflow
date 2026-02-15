use axum::Router;

#[cfg(debug_assertions)]
use axum::{extract::State, http::StatusCode, routing::post, Json};

#[cfg(debug_assertions)]
use tokio::fs;

#[cfg(debug_assertions)]
use tracing::warn;

use crate::AppState;

#[cfg(debug_assertions)]
pub fn add_test_routes(router: Router<AppState>) -> Router<AppState> {
    if std::env::var("MAPFLOW_TEST_MODE").as_deref() == Ok("1") {
        tracing::info!("Test mode enabled (debug only): exposing POST /api/test/reset");
        router.route("/api/test/reset", post(reset_test_state))
    } else {
        router
    }
}

#[cfg(not(debug_assertions))]
pub fn add_test_routes(router: Router<AppState>) -> Router<AppState> {
    router
}

#[cfg(debug_assertions)]
async fn reset_test_state(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let conn = state.db.lock().await;

    // Drop per-dataset tables.
    // We use files.table_name as the source of truth.
    if let Ok(mut stmt) = conn.prepare("SELECT table_name FROM files WHERE table_name IS NOT NULL")
    {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, Option<String>>(0)) {
            for table in rows.flatten().flatten() {
                // table is normalized/safe, but quote anyway.
                let _ = conn.execute(&format!("DROP TABLE IF EXISTS \"{table}\""), []);
            }
        }
    }

    // Order matters because of foreign key constraints (published_files.file_id -> files.id).
    if let Err(e) = conn.execute_batch(
        "DELETE FROM published_files;\nDELETE FROM dataset_columns;\nDELETE FROM files;\nDELETE FROM sessions;\nDELETE FROM users;\nDELETE FROM system_settings;",
    ) {
        warn!(error = ?e, "Test reset DB error");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "DB cleanup failed" })),
        );
    }

    match fs::read_dir(&state.upload_dir).await {
        Ok(mut entries) => {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    if let Err(e) = fs::remove_dir_all(path).await {
                        warn!(error = ?e, "Test reset FS error");
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": "Upload dir cleanup failed" })),
                        );
                    }
                } else if let Err(e) = fs::remove_file(path).await {
                    warn!(error = ?e, "Test reset FS error");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": "Upload dir cleanup failed" })),
                    );
                }
            }
        }
        Err(e) => {
            warn!(error = ?e, "Test reset FS error");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Upload dir read failed" })),
            );
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "reset_complete" })),
    )
}
