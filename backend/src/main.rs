use std::{path::PathBuf, sync::Arc};
use tokio::{fs, sync::Mutex};
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| backend::DEFAULT_DB_PATH.to_string());
    let db_path = PathBuf::from(db_path);
    let conn = backend::init_database(&db_path);

    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string());
    let upload_dir = PathBuf::from(upload_dir);
    let _ = fs::create_dir_all(&upload_dir).await;

    let (max_size, max_size_label) = backend::read_max_size_config();

    let db = Arc::new(Mutex::new(conn));

    // 创建认证 backend 和 session store
    let auth_backend = backend::AuthBackend::new(db.clone());
    let session_store = backend::DuckDBStore::new(db.clone());

    let state = backend::AppState {
        upload_dir,
        db: db.clone(),
        max_size,
        max_size_label,
        auth_backend,
        session_store,
    };

    // Reconciliation: Mark any 'processing' files as 'failed' on startup
    let _ = backend::reconcile_processing_files(&state.db).await;

    let mut app = backend::build_api_router(state.clone());

    let web_dist = std::env::var("WEB_DIST").unwrap_or_else(|_| "frontend/dist".to_string());
    let web_dist_path = PathBuf::from(web_dist);
    if web_dist_path.exists() {
        let index_path = web_dist_path.join("index.html");
        app = app.fallback_service(
            ServeDir::new(&web_dist_path).not_found_service(ServeFile::new(index_path)),
        );
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{port}");
    println!("MapFlow server running at http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server failed");
}
