mod auth;
mod auth_routes;
mod config;
mod crs;
mod db;
mod handlers;
mod http_errors;
mod import;
mod mbtiles;
mod models;
mod password;
mod postgis;
mod public;
mod routes;
mod session_store;
mod static_assets;
mod test_routes;
mod tiles;
mod upload;
mod validation;

#[cfg(windows)]
pub mod tray;
pub use auth::{AuthBackend, User};
pub use auth_routes::build_auth_router;
pub use config::{
    format_bytes, init_max_size_config, read_cookie_secure, read_max_size_config,
    read_preview_zoom_config,
};
pub use db::{
    ensure_app_secret, init_database, is_initialized, reconcile_processing_files, set_initialized,
    DEFAULT_DB_PATH, PROCESSING_RECONCILIATION_ERROR,
};
pub use handlers::validate_slug;
pub use models::{
    AppState, ErrorResponse, FileItem, FileSchemaResponse, PreviewMeta, PublicTileMeta,
    PublicTileUrl, PublishRequest, PublishResponse,
};
pub use password::{hash_password, validate_password_complexity, verify_password, PasswordError};
pub use routes::{build_api_router, build_test_router};
pub use session_store::DuckDBStore;
#[cfg(feature = "embed-web-dist")]
pub use static_assets::serve_embedded_spa;
pub use validation::{validate_geojson, validate_shapefile_zip};

pub fn initialize_app_secret(conn: &duckdb::Connection) -> Result<(), String> {
    ensure_app_secret(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use std::sync::OnceLock;
    use tempfile::TempDir;
    use tokio::sync::{Mutex, RwLock};
    use tower::util::ServiceExt;

    static ENV_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();

    async fn setup_state(max_size: u64) -> (AppState, TempDir) {
        let temp_dir = TempDir::new().expect("temp dir");
        let upload_dir = temp_dir.path().join("uploads");
        tokio::fs::create_dir_all(&upload_dir).await.ok();
        let upload_dir_canonical = upload_dir
            .canonicalize()
            .unwrap_or_else(|_| upload_dir.clone());

        let conn = duckdb::Connection::open_in_memory().expect("Failed to create test database");
        crate::db::ensure_spatial_extension(&conn).expect("spatial extension");

        conn.execute_batch(
            r"
        CREATE TABLE files (
            id VARCHAR PRIMARY KEY,
            name VARCHAR NOT NULL,
            type VARCHAR NOT NULL,
            size BIGINT NOT NULL,
            uploaded_at TIMESTAMP NOT NULL,
            status VARCHAR NOT NULL,
            crs VARCHAR,
            crs_type VARCHAR DEFAULT 'standard',
            data_bounds VARCHAR,
            path VARCHAR NOT NULL,
            table_name VARCHAR,
            error VARCHAR,
            is_public BOOLEAN DEFAULT FALSE,
            tile_format VARCHAR,
            minzoom INTEGER,
            maxzoom INTEGER,
            tile_bounds VARCHAR,
            tile_source VARCHAR DEFAULT 'duckdb'
        );

        CREATE TABLE IF NOT EXISTS published_files (
            file_id VARCHAR PRIMARY KEY,
            slug VARCHAR UNIQUE NOT NULL,
            published_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            minzoom INTEGER,
            maxzoom INTEGER,
            use_aliases BOOLEAN DEFAULT TRUE,
            FOREIGN KEY (file_id) REFERENCES files(id)
        );

        CREATE TABLE dataset_columns (
            source_id VARCHAR NOT NULL,
            normalized_name VARCHAR NOT NULL,
            original_name VARCHAR NOT NULL,
            ordinal BIGINT NOT NULL,
            mvt_type VARCHAR NOT NULL,
            PRIMARY KEY (source_id, normalized_name)
        );
        ",
        )
        .unwrap();

        let conn = Arc::new(Mutex::new(conn));
        let state = AppState {
            upload_dir,
            upload_dir_canonical,
            db: conn.clone(),
            max_size: Arc::new(RwLock::new(max_size)),
            max_size_label: Arc::new(RwLock::new(format_bytes(max_size))),
            auth_backend: AuthBackend::new(conn.clone()),
            session_store: DuckDBStore::new(conn),
        };

        (state, temp_dir)
    }

    async fn response_json<T: serde::de::DeserializeOwned>(
        response: axum::response::Response,
    ) -> T {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        serde_json::from_slice(&body).expect("json")
    }

    #[tokio::test]
    async fn list_returns_seeded_items() {
        let (state, _temp_dir) = setup_state(1024).await;
        let uploaded_at = "2026-02-04T10:00:00Z";
        let file_path = state.upload_dir.join("seed-1").join("existing.geojson");
        let item = FileItem {
            id: "seed-1".to_string(),
            name: "existing".to_string(),
            file_type: "geojson".to_string(),
            size: 42,
            uploaded_at: uploaded_at.to_string(),
            status: "uploaded".to_string(),
            crs: None,
            crs_type: None,
            path: file_path.to_string_lossy().to_string(),
            table_name: None,
            error: None,
            is_public: Some(false),
            public_slug: None,
            tile_format: None,
            minzoom: None,
            maxzoom: None,
            use_aliases: None,
            tile_source: None,
        };

        let conn = state.db.lock().await;
        let size = item.size as i64;
        conn.execute(
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            duckdb::params![
                &item.id,
                &item.name,
                &item.file_type,
                size,
                &item.uploaded_at,
                &item.status,
                &item.crs,
                &item.path,
                &item.table_name,
                &item.error,
                false,
            ],
        )
        .unwrap();
        drop(conn);

        let app = build_test_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/files")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let items: Vec<FileItem> = response_json(response).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "existing");
        assert_eq!(items[0].status, "uploaded");
    }

    #[test]
    fn read_cookie_secure_from_env() {
        let _guard = ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env lock");

        std::env::remove_var("COOKIE_SECURE");
        assert!(!read_cookie_secure());

        std::env::set_var("COOKIE_SECURE", "false");
        assert!(!read_cookie_secure());

        std::env::set_var("COOKIE_SECURE", "true");
        assert!(read_cookie_secure());

        std::env::set_var("COOKIE_SECURE", "invalid");
        assert!(!read_cookie_secure());

        std::env::remove_var("COOKIE_SECURE");
    }

    #[test]
    fn initialize_app_secret_sets_and_reuses_persisted_secret() {
        let _guard = ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env lock");

        let conn = duckdb::Connection::open_in_memory().expect("db");
        conn.execute_batch(
            "CREATE TABLE system_settings (
                key VARCHAR PRIMARY KEY,
                value VARCHAR NOT NULL
            )",
        )
        .expect("create system_settings");

        initialize_app_secret(&conn).expect("initialize app secret");

        let first: String = conn
            .prepare("SELECT value FROM system_settings WHERE key = 'app_secret'")
            .expect("prepare persisted query")
            .query_row([], |row| row.get(0))
            .expect("read persisted app secret");
        assert!(!first.is_empty());

        initialize_app_secret(&conn).expect("reinitialize app secret");
        let second: String = conn
            .prepare("SELECT value FROM system_settings WHERE key = 'app_secret'")
            .expect("prepare persisted query again")
            .query_row([], |row| row.get(0))
            .expect("read persisted app secret again");
        assert_eq!(second, first);
    }

    #[test]
    fn read_max_size_config_default_and_custom() {
        let _guard = ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env lock");

        let default_mb: u64 = 1024;
        let bytes_per_mb: u64 = 1024 * 1024;

        std::env::remove_var("UPLOAD_MAX_SIZE_MB");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, default_mb * bytes_per_mb);
        assert_eq!(label, "1GB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "12");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, 12 * bytes_per_mb);
        assert_eq!(label, "12MB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "0");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, default_mb * bytes_per_mb);
        assert_eq!(label, "1GB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "nope");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, default_mb * bytes_per_mb);
        assert_eq!(label, "1GB");
        std::env::remove_var("UPLOAD_MAX_SIZE_MB");
    }

    #[test]
    fn read_preview_zoom_config_default_and_custom() {
        let _guard = ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env lock");

        // Test defaults
        std::env::remove_var("PREVIEW_MIN_ZOOM");
        std::env::remove_var("PREVIEW_MAX_ZOOM");
        let (min, max) = read_preview_zoom_config();
        assert_eq!(min, 0);
        assert_eq!(max, 22);

        // Test custom values
        std::env::set_var("PREVIEW_MIN_ZOOM", "5");
        std::env::set_var("PREVIEW_MAX_ZOOM", "18");
        let (min, max) = read_preview_zoom_config();
        assert_eq!(min, 5);
        assert_eq!(max, 18);

        // Test invalid values fall back to defaults
        std::env::set_var("PREVIEW_MIN_ZOOM", "invalid");
        std::env::set_var("PREVIEW_MAX_ZOOM", "invalid");
        let (min, max) = read_preview_zoom_config();
        assert_eq!(min, 0);
        assert_eq!(max, 22);

        // Test out-of-range values are clamped
        std::env::set_var("PREVIEW_MIN_ZOOM", "-5");
        std::env::set_var("PREVIEW_MAX_ZOOM", "30");
        let (min, max) = read_preview_zoom_config();
        assert_eq!(min, 0); // clamped to 0
        assert_eq!(max, 22); // clamped to 22

        // Test min > max is corrected (max clamped to min)
        std::env::set_var("PREVIEW_MIN_ZOOM", "15");
        std::env::set_var("PREVIEW_MAX_ZOOM", "5");
        let (min, max) = read_preview_zoom_config();
        assert_eq!(min, 15);
        assert_eq!(max, 15); // clamped to min_zoom

        std::env::remove_var("PREVIEW_MIN_ZOOM");
        std::env::remove_var("PREVIEW_MAX_ZOOM");
    }
}
