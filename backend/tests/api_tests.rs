use axum::body::Body;
use axum::http::Request;
use backend::{
    build_api_router, build_test_router, init_database, reconcile_processing_files, AppState,
    AuthBackend, DuckDBStore, FileItem, PROCESSING_RECONCILIATION_ERROR,
};
use http_body_util::BodyExt; // for collect()
use mvt_reader::{feature::Value as MvtValue, Reader as MvtReader};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;
use tower::ServiceExt; // for oneshot

static TEST_MODE_SET: std::sync::Once = std::sync::Once::new();

fn ensure_test_mode() {
    TEST_MODE_SET.call_once(|| {
        std::env::set_var("MAPFLOW_TEST_MODE", "1");
    });
}

async fn wait_until_ready(app: &axum::Router, file_id: &str) -> FileItem {
    let mut last_status: Option<String> = None;
    let mut last_error: Option<String> = None;

    for _ in 0..120 {
        let request = Request::builder()
            .method("GET")
            .uri("/api/files")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let files: Vec<FileItem> = serde_json::from_slice(&body_bytes).unwrap();
        if let Some(f) = files.iter().find(|f| f.id == file_id) {
            last_status = Some(f.status.clone());
            last_error = f.error.clone();
            if f.status == "ready" {
                return f.clone();
            }
            if f.status == "failed" {
                panic!("File processing failed: {:?}", f.error);
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    panic!(
        "Timeout waiting for file to be ready (last_status={:?}, last_error={:?})",
        last_status, last_error
    );
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_fixture_bytes(rel_path_from_repo_root: &str) -> Vec<u8> {
    let p = repo_root().join(rel_path_from_repo_root);
    std::fs::read(&p).unwrap_or_else(|e| panic!("Failed to read fixture {p:?}: {e}"))
}

fn multipart_body(boundary: &str, filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn mvt_has_string_tag(tile: &[u8], want_key: &str, want_value: &str) -> bool {
    let reader = match MvtReader::new(tile.to_vec()) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let layers = match reader.get_layer_names() {
        Ok(v) => v,
        Err(_) => return false,
    };

    for (layer_index, _layer_name) in layers.into_iter().enumerate() {
        let features = match reader.get_features(layer_index) {
            Ok(v) => v,
            Err(_) => continue,
        };

        for f in features {
            let Some(props) = f.properties.as_ref() else {
                continue;
            };
            let Some(v) = props.get(want_key) else {
                continue;
            };
            if let MvtValue::String(s) = v {
                if s == want_value {
                    return true;
                }
            }
        }
    }

    false
}

// Helper to upload a simple GeoJSON file and return the file_id
async fn upload_geojson_file(app: &axum::Router) -> String {
    let boundary = "------------------------boundaryXYZ";
    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": { "name": "Test Point" },
                "geometry": {
                    "type": "Point",
                    "coordinates": [0.0, 0.0]
                }
            }
        ]
    }"#;

    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"points.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    file_item.id
}

// Helper to setup the app for testing
async fn setup_app() -> (axum::Router, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db),
    };

    let router = build_test_router(state);
    (router, temp_dir)
}

async fn setup_app_with_relative_upload_dir() -> (axum::Router, TempDir, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");

    std::fs::create_dir_all("tmp").expect("create tmp dir");
    let upload_temp_dir = tempfile::Builder::new()
        .prefix("test-uploads-")
        .tempdir_in("tmp")
        .expect("create upload temp dir");

    let current_dir = std::env::current_dir().expect("current dir");
    let upload_dir = upload_temp_dir
        .path()
        .strip_prefix(&current_dir)
        .expect("relative upload dir")
        .to_path_buf();
    let upload_dir_canonical = upload_temp_dir
        .path()
        .canonicalize()
        .unwrap_or_else(|_| upload_temp_dir.path().to_path_buf());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db),
    };

    let router = build_test_router(state);
    (router, temp_dir, upload_temp_dir)
}

async fn setup_app_with_auth() -> (axum::Router, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db),
    };

    let router = build_api_router(state);
    (router, temp_dir)
}

async fn setup_app_with_large_max_size() -> (axum::Router, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(100 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("100MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };

    let router = build_test_router(state);
    (router, temp_dir)
}

#[tokio::test]
async fn test_upload_empty_body_returns_400() {
    let (app, _temp) = setup_app().await;

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header("content-type", "multipart/form-data; boundary=boundary")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("Invalid multipart form"));
}

#[tokio::test]
async fn test_upload_missing_file_field_returns_400() {
    let (app, _temp) = setup_app().await;

    let boundary = "------------------------boundaryNOFILE";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nhello\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["error"], "No file uploaded");
}

#[tokio::test]
async fn test_upload_missing_filename_returns_400() {
    let (app, _temp) = setup_app().await;

    let boundary = "------------------------boundaryNOFILENAME";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"\r\n\r\n{{}}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["error"], "Missing file name");
}

#[tokio::test]
async fn test_preview_nonexistent_id_returns_404() {
    let (app, _temp) = setup_app().await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/files/no-such-id/preview")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["error"], "File not found");
}

#[tokio::test]
async fn test_preview_not_ready_returns_409() {
    let (app, _temp) = setup_app().await;

    let boundary = "------------------------boundaryNR";
    let geojson_content = r#"{ "type": "FeatureCollection", "features": [] }"#;
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"empty.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    // Immediately request preview. It should be rejected until status=ready.
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/preview", file_item.id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["error"], "File is not ready for preview");
}

#[tokio::test]
async fn test_tile_not_ready_returns_409() {
    let (app, _temp) = setup_app().await;

    let boundary = "------------------------boundaryTNR";
    let geojson_content = r#"{ "type": "FeatureCollection", "features": [] }"#;
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"empty.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/tiles/0/0/0", file_item.id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["error"], "File is not ready for preview");
}

#[tokio::test]
async fn test_tile_invalid_coords_returns_400() {
    let (app, _temp) = setup_app().await;

    // z < 0
    let request = Request::builder()
        .method("GET")
        .uri("/api/files/nope/tiles/-1/0/0")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    // x out of range for z=0 (max x is 0)
    let request = Request::builder()
        .method("GET")
        .uri("/api/files/nope/tiles/0/1/0")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    // y out of range for z=1 (max y is 1)
    let request = Request::builder()
        .method("GET")
        .uri("/api/files/nope/tiles/1/0/2")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("Invalid tile coordinates"));
}

#[tokio::test]
async fn test_upload_payload_too_large_returns_413() {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(1024)),
        max_size_label: Arc::new(RwLock::new("1KB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db),
    };

    let app = build_test_router(state);

    let boundary = "------------------------boundaryBIG";
    let big = "a".repeat(2048);
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"big.geojson\"\r\n\r\n{big}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(body_json["error"]
        .as_str()
        .unwrap_or("")
        .contains("File too large"));
}

#[tokio::test]
async fn test_upload_invalid_shapefile_zip_returns_400() {
    let (app, _temp) = setup_app().await;

    // Make a zip that does not contain any .shp
    let mut zip_bytes = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut zip_bytes);
        let mut zip = zip::ZipWriter::new(cursor);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file("readme.txt", options).unwrap();
        std::io::Write::write_all(&mut zip, b"not a shapefile").unwrap();
        zip.finish().unwrap();
    }

    let boundary = "------------------------boundaryZIP";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"bad.zip\"\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&zip_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["error"], "Missing .shp file in zip");
}

#[tokio::test]
async fn test_startup_reconciliation_marks_processing_as_failed() {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db),
    };

    // Seed a processing file.
    {
        let conn = state.db.lock().await;
        conn.execute(
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error)\
             VALUES (?1, ?2, ?3, ?4, NOW(), ?5, ?6, ?7, ?8, ?9)",
            duckdb::params![
                "seed-processing",
                "seed",
                "geojson",
                1_i64,
                "processing",
                None::<String>,
                "./uploads/seed-processing/seed.geojson",
                None::<String>,
                None::<String>,
            ],
        )
        .unwrap();
    }

    reconcile_processing_files(&state.db).await.unwrap();

    let app = build_test_router(state);
    let request = Request::builder()
        .method("GET")
        .uri("/api/files")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let files: Vec<FileItem> = serde_json::from_slice(&body_bytes).unwrap();
    let item = files.iter().find(|f| f.id == "seed-processing").unwrap();
    assert_eq!(item.status, "failed");
    assert_eq!(item.error.as_deref(), Some(PROCESSING_RECONCILIATION_ERROR));
}

#[tokio::test]
async fn test_upload_invalid_extension() {
    let (app, _temp) = setup_app().await;

    let boundary = "------------------------boundary123";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\r\nHello World\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        body_json["error"],
        "Unsupported file type. Use .zip, .geojson, .json, .geojsonl, .kml, .gpx, .topojson, .mbtiles, or .pmtiles"
    );
}

#[tokio::test]
async fn test_upload_geojson_lifecycle() {
    let (app, _temp) = setup_app().await;

    // 1. Upload valid GeoJSON
    let boundary = "------------------------boundaryXYZ";
    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": { "name": "Test Point" },
                "geometry": {
                    "type": "Point",
                    "coordinates": [0.0, 0.0]
                }
            }
        ]
    }"#;

    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"points.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    // Clone app for reuse since oneshot consumes it
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(file_item.name, "points");
    assert_eq!(file_item.status, "uploaded");
    let file_id = file_item.id;

    // 2. Poll for status change (uploaded -> processing -> ready)
    // Processing happens in background tokio::spawn, so we need to wait.
    let ready_item = wait_until_ready(&app, &file_id).await;
    assert!(ready_item.crs.is_some(), "CRS should be detected");
    assert!(
        ready_item.table_name.is_some(),
        "table_name should be set when ready"
    );

    // 3. Check Preview Meta
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/preview", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // 4. Request a Tile (0/0/0 should cover the point at 0,0)
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/tiles/0/0/0", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/vnd.mapbox-vector-tile"
    );

    let tile_body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        !tile_body.is_empty(),
        "Expected non-empty MVT tile body for point at 0,0"
    );

    // 5. Verify MVT includes properties (tags)
    // We expect our uploaded GeoJSON property { "name": "Test Point" } to be present.
    assert!(
        mvt_has_string_tag(&tile_body, "name", "Test Point"),
        "Expected MVT to include string tag name=Test Point"
    );
}

#[tokio::test]
async fn test_feature_properties_endpoint_returns_null_for_missing_values() {
    let (app, _temp) = setup_app().await;

    // Two features share schema {name, class, speed_limit} but second feature omits speed_limit.
    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {
                    "name": "Road A",
                    "class": "primary",
                    "speed_limit": 30
                },
                "geometry": {
                    "type": "LineString",
                    "coordinates": [[0, 0], [0.1, 0.1]]
                }
            },
            {
                "type": "Feature",
                "properties": {
                    "name": "Road B",
                    "class": "secondary"
                },
                "geometry": {
                    "type": "LineString",
                    "coordinates": [[0, 0], [0.1, 0.1]]
                }
            }
        ]
    }"#;

    let boundary = "------------------------boundaryFEATURES";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"roads.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id;

    let _ready_item = wait_until_ready(&app, &file_id).await;

    // fid is 1-based (row_number()) and stable.
    // We query the second feature, which should have speed_limit = NULL.
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/features/2", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["fid"], 2);
    let props = body_json["properties"]
        .as_array()
        .expect("properties array");
    assert!(props.len() >= 2);

    let mut saw_name = false;
    let mut saw_class = false;
    let mut saw_speed_limit = false;
    let mut speed_limit_was_null = false;

    for p in props {
        let key = p["key"].as_str().unwrap_or("");
        if key == "name" {
            saw_name = true;
            assert_eq!(p["value"], "Road B");
        }
        if key == "class" {
            saw_class = true;
            assert_eq!(p["value"], "secondary");
        }
        if key == "speed_limit" {
            saw_speed_limit = true;
            speed_limit_was_null = p["value"].is_null();
        }
    }

    assert!(saw_name);
    assert!(saw_class);
    assert!(saw_speed_limit, "Expected speed_limit key to be present");
    assert!(
        speed_limit_was_null,
        "Expected missing speed_limit to be returned as JSON null"
    );
}

#[tokio::test]
async fn test_all_null_id_property_is_preserved_in_schema_and_feature_properties() {
    let (app, _temp) = setup_app().await;

    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {
                    "id": null,
                    "name": "A"
                },
                "geometry": {
                    "type": "Point",
                    "coordinates": [0.0, 0.0]
                }
            },
            {
                "type": "Feature",
                "properties": {
                    "id": null,
                    "name": "B"
                },
                "geometry": {
                    "type": "Point",
                    "coordinates": [1.0, 1.0]
                }
            }
        ]
    }"#;

    let boundary = "------------------------boundaryIDNULL";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"id-null.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    assert_eq!(upload_response.status(), axum::http::StatusCode::CREATED);
    let upload_body = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&upload_body).unwrap();
    let file_id = file_item.id;

    let _ready_item = wait_until_ready(&app, &file_id).await;

    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();
    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);
    let schema_body = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema_json: serde_json::Value = serde_json::from_slice(&schema_body).unwrap();
    let fields = schema_json["layers"][0]["fields"]
        .as_array()
        .expect("fields should be array");

    let mut found_id_field = false;
    let mut found_name_field = false;
    for field in fields {
        if field["name"] == "id" {
            found_id_field = true;
        }
        if field["name"] == "name" {
            found_name_field = true;
        }
    }
    assert!(
        found_id_field,
        "Expected schema to preserve source property field `id` even when all values are NULL"
    );
    assert!(found_name_field, "Expected schema to include `name` field");

    let feature_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/features/1", file_id))
        .body(Body::empty())
        .unwrap();
    let feature_response = app.oneshot(feature_request).await.unwrap();
    assert_eq!(feature_response.status(), axum::http::StatusCode::OK);
    let feature_body = feature_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let feature_json: serde_json::Value = serde_json::from_slice(&feature_body).unwrap();
    let props = feature_json["properties"]
        .as_array()
        .expect("properties should be array");

    let mut saw_id = false;
    let mut saw_name = false;
    for p in props {
        let key = p["key"].as_str().unwrap_or("");
        if key == "id" {
            saw_id = true;
            assert!(p["value"].is_null(), "Expected id value to be JSON null");
        }
        if key == "name" {
            saw_name = true;
            assert_eq!(p["value"], "A");
        }
    }
    assert!(saw_id, "Expected feature properties to include key `id`");
    assert!(
        saw_name,
        "Expected feature properties to include key `name`"
    );
}

#[tokio::test]
async fn test_geojson_with_ogc_fid_property_imports_successfully() {
    let (app, _temp) = setup_app().await;

    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {
                    "ogc_fid": 123,
                    "name": "A"
                },
                "geometry": {
                    "type": "Point",
                    "coordinates": [0.0, 0.0]
                }
            }
        ]
    }"#;

    let boundary = "------------------------boundaryOGCFID";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"ogc-fid.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    assert_eq!(upload_response.status(), axum::http::StatusCode::CREATED);
    let upload_body = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&upload_body).unwrap();
    let file_id = file_item.id;

    let _ready_item = wait_until_ready(&app, &file_id).await;

    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();
    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);
    let schema_body = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema_json: serde_json::Value = serde_json::from_slice(&schema_body).unwrap();
    let fields = schema_json["layers"][0]["fields"]
        .as_array()
        .expect("fields should be array");

    let mut found_ogc_fid = false;
    let mut found_name = false;
    for field in fields {
        if field["name"] == "ogc_fid" {
            found_ogc_fid = true;
        }
        if field["name"] == "name" {
            found_name = true;
        }
    }
    assert!(
        found_ogc_fid,
        "Expected schema to include real source property `ogc_fid`"
    );
    assert!(found_name, "Expected schema to include `name`");

    let feature_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/features/1", file_id))
        .body(Body::empty())
        .unwrap();
    let feature_response = app.oneshot(feature_request).await.unwrap();
    assert_eq!(feature_response.status(), axum::http::StatusCode::OK);
    let feature_body = feature_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let feature_json: serde_json::Value = serde_json::from_slice(&feature_body).unwrap();
    let props = feature_json["properties"]
        .as_array()
        .expect("properties should be array");

    let mut saw_ogc_fid = false;
    let mut saw_name = false;
    for p in props {
        let key = p["key"].as_str().unwrap_or("");
        if key == "ogc_fid" {
            saw_ogc_fid = true;
            assert_eq!(p["value"], 123);
        }
        if key == "name" {
            saw_name = true;
            assert_eq!(p["value"], "A");
        }
    }
    assert!(
        saw_ogc_fid,
        "Expected feature properties to include `ogc_fid`"
    );
    assert!(saw_name, "Expected feature properties to include `name`");
}

#[tokio::test]
async fn test_geojson_ogc_fid_workaround_handles_case_variant_placeholder_key() {
    let (app, _temp) = setup_app().await;

    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {
                    "ogc_fid": 123,
                    "__MAPFLOW_SRC_OGC_FID": "existing",
                    "name": "A"
                },
                "geometry": {
                    "type": "Point",
                    "coordinates": [0.0, 0.0]
                }
            }
        ]
    }"#;

    let boundary = "------------------------boundaryOGCFIDCASE";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"ogc-fid-case.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    assert_eq!(upload_response.status(), axum::http::StatusCode::CREATED);
    let upload_body = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&upload_body).unwrap();
    let file_id = file_item.id;

    let _ready_item = wait_until_ready(&app, &file_id).await;

    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();
    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn test_top_level_geojson_feature_with_ogc_fid_imports_successfully() {
    let (app, _temp) = setup_app().await;

    let geojson_content = r#"{
        "type": "Feature",
        "properties": {
            "ogc_fid": 456,
            "name": "Single"
        },
        "geometry": {
            "type": "Point",
            "coordinates": [1.0, 2.0]
        }
    }"#;

    let boundary = "------------------------boundaryTopLevelFeatureOGCFID";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"single-feature-ogc-fid.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    assert_eq!(upload_response.status(), axum::http::StatusCode::CREATED);
    let upload_body = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&upload_body).unwrap();
    let file_id = file_item.id;

    let _ready_item = wait_until_ready(&app, &file_id).await;

    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();
    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);
    let schema_body = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema_json: serde_json::Value = serde_json::from_slice(&schema_body).unwrap();
    let fields = schema_json["layers"][0]["fields"]
        .as_array()
        .expect("fields should be array");

    let mut found_ogc_fid = false;
    let mut found_name = false;
    for field in fields {
        if field["name"] == "ogc_fid" {
            found_ogc_fid = true;
        }
        if field["name"] == "name" {
            found_name = true;
        }
    }
    assert!(
        found_ogc_fid,
        "Expected schema to include real source property `ogc_fid` for top-level Feature"
    );
    assert!(found_name, "Expected schema to include `name`");

    let feature_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/features/1", file_id))
        .body(Body::empty())
        .unwrap();
    let feature_response = app.oneshot(feature_request).await.unwrap();
    assert_eq!(feature_response.status(), axum::http::StatusCode::OK);
    let feature_body = feature_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let feature_json: serde_json::Value = serde_json::from_slice(&feature_body).unwrap();
    let props = feature_json["properties"]
        .as_array()
        .expect("properties should be array");

    let mut saw_ogc_fid = false;
    let mut saw_name = false;
    for p in props {
        let key = p["key"].as_str().unwrap_or("");
        if key == "ogc_fid" {
            saw_ogc_fid = true;
            assert_eq!(p["value"], 456);
        }
        if key == "name" {
            saw_name = true;
            assert_eq!(p["value"], "Single");
        }
    }
    assert!(
        saw_ogc_fid,
        "Expected feature properties to include `ogc_fid` for top-level Feature"
    );
    assert!(saw_name, "Expected feature properties to include `name`");
}

#[tokio::test]
async fn test_schema_endpoint_returns_fields_and_types() {
    let (app, _temp) = setup_app().await;

    // Upload GeoJSON with multiple property types
    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {
                    "name": "Test Feature",
                    "class": "primary",
                    "count": 42,
                    "length": 123.45
                },
                "geometry": {
                    "type": "Point",
                    "coordinates": [0.0, 0.0]
                }
            }
        ]
    }"#;

    let boundary = "------------------------boundarySCHEMA";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id;

    let _ready_item = wait_until_ready(&app, &file_id).await;

    // Request schema
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // Verify response structure
    assert!(body_json["layers"].is_array());

    let layers = body_json["layers"]
        .as_array()
        .expect("layers should be an array");
    assert_eq!(
        layers.len(),
        1,
        "Regular datasets should return a single default layer"
    );

    let layer = &layers[0];
    assert_eq!(layer["id"], "default");
    assert!(layer["description"].is_null() || layer["description"].is_array());

    let fields = layer["fields"]
        .as_array()
        .expect("layer fields should be an array");

    // We expect to find our property fields
    let mut found_name = false;
    let mut found_class = false;
    let mut found_count = false;
    let mut found_length = false;

    for field in fields {
        let name = field["name"].as_str();
        let field_type = field["type"].as_str();

        if let Some(n) = name {
            match n {
                "name" => {
                    found_name = true;
                    assert_eq!(field_type, Some("VARCHAR"));
                }
                "class" => {
                    found_class = true;
                    assert_eq!(field_type, Some("VARCHAR"));
                }
                "count" => {
                    found_count = true;
                    assert_eq!(field_type, Some("INTEGER"));
                }
                "length" => {
                    found_length = true;
                    assert_eq!(field_type, Some("DOUBLE"));
                }
                _ => {}
            }
        }
    }

    assert!(found_name, "Expected to find 'name' field");
    assert!(found_class, "Expected to find 'class' field");
    assert!(found_count, "Expected to find 'count' field");
    assert!(found_length, "Expected to find 'length' field");
}

#[tokio::test]
async fn test_dynamic_table_preview_returns_null_zoom() {
    let (app, _temp) = setup_app().await;

    let boundary = "------------------------boundaryDZ";
    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "geometry": {"type": "Point", "coordinates": [0, 0]},
                "properties": {"name": "Test Point"}
            }
        ]
    }"#;

    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Get preview metadata
    let preview_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/preview", file_item.id))
        .body(Body::empty())
        .unwrap();

    let preview_response = app.oneshot(preview_request).await.unwrap();
    assert_eq!(preview_response.status(), axum::http::StatusCode::OK);

    let preview_bytes = preview_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let preview: serde_json::Value = serde_json::from_slice(&preview_bytes).unwrap();

    // Dynamic tables should return fixed preview zoom range (0, 22)
    assert_eq!(preview["minZoom"], 0);
    assert_eq!(preview["maxZoom"], 22);
}

#[tokio::test]
async fn test_schema_endpoint_returns_409_for_non_ready_file() {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db),
    };

    let app = build_test_router(state.clone());

    // Insert a file in 'processing' state directly to avoid race condition
    {
        let conn = state.db.lock().await;
        conn.execute(
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error)\
             VALUES (?1, ?2, ?3, ?4, NOW(), ?5, ?6, ?7, ?8, ?9)",
            duckdb::params![
                "test-processing-file",
                "test.geojson",
                "geojson",
                100_i64,
                "processing",
                None::<String>,
                "./uploads/test/test.geojson",
                None::<String>,
                None::<String>,
            ],
        )
        .expect("insert processing file");
    }

    // Request schema - should return 409 since file is not ready
    let request = Request::builder()
        .method("GET")
        .uri("/api/files/test-processing-file/schema")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_schema_endpoint_returns_404_for_nonexistent_file() {
    let (app, _temp) = setup_app().await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/files/nonexistent/schema")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_schema_endpoint_handles_minimal_fields() {
    let (app, _temp) = setup_app().await;

    // Upload GeoJSON with only geometry, no properties
    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [0.0, 0.0]
                }
            }
        ]
    }"#;

    let boundary = "------------------------boundaryMINIMAL";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"minimal.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id;

    let _ready_item = wait_until_ready(&app, &file_id).await;

    // Request schema
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // Verify response structure
    assert!(body_json["layers"].is_array());

    let layers = body_json["layers"]
        .as_array()
        .expect("layers should be an array");
    assert_eq!(
        layers.len(),
        1,
        "Regular datasets should return a single default layer"
    );

    let layer = &layers[0];
    assert_eq!(layer["id"], "default");

    let fields = layer["fields"]
        .as_array()
        .expect("layer fields should be an array");

    // With no properties, dataset_columns should only have metadata fields (fid, geom excluded)
    // So we expect an empty array or only metadata
    // The implementation excludes geom and fid, so empty array is expected
    assert_eq!(
        fields.len(),
        0,
        "Expected no property fields for feature with no properties"
    );
}

#[tokio::test]
async fn test_schema_endpoint_handles_many_fields() {
    let (app, _temp) = setup_app().await;

    // Generate GeoJSON with many properties (50 fields)
    let mut properties = serde_json::Map::new();
    for i in 0..50 {
        properties.insert(format!("field_{}", i), serde_json::json!(i));
    }

    let geojson_obj = serde_json::json!({
        "type": "FeatureCollection",
        "features": [{
            "type": "Feature",
            "properties": properties,
            "geometry": {
                "type": "Point",
                "coordinates": [0.0, 0.0]
            }
        }]
    });

    let geojson_content = geojson_obj.to_string();

    let boundary = "------------------------boundaryMANY";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"many.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id;

    let _ready_item = wait_until_ready(&app, &file_id).await;

    // Request schema
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // Verify response structure
    assert!(body_json["layers"].is_array());

    let layers = body_json["layers"]
        .as_array()
        .expect("layers should be an array");
    assert_eq!(
        layers.len(),
        1,
        "Regular datasets should return a single default layer"
    );

    let layer = &layers[0];
    assert_eq!(layer["id"], "default");

    let fields = layer["fields"]
        .as_array()
        .expect("layer fields should be an array");

    // Should have all 50 fields
    assert_eq!(fields.len(), 50, "Expected 50 property fields");

    // Verify all fields have correct structure (name and type)
    for field in fields {
        assert!(field["name"].is_string(), "Field name should be a string");
        assert!(field["type"].is_string(), "Field type should be a string");

        let name = field["name"].as_str().unwrap();
        assert!(
            name.starts_with("field_"),
            "Field name should start with 'field_'"
        );

        // All generated fields are integers
        assert_eq!(
            field["type"], "INTEGER",
            "Generated fields should be INTEGER type"
        );
    }

    // Verify we can find our expected fields
    let field_names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();

    for i in [0, 25, 49].iter() {
        let expected = format!("field_{}", i);
        assert!(
            field_names.contains(&expected.as_str()),
            "Expected to find field {}",
            expected
        );
    }
}

#[tokio::test]
async fn test_upload_shapefile_zip_lifecycle() {
    let (app, _temp) = setup_app().await;

    let zip_bytes = read_fixture_bytes("frontend/tests/fixtures/roads.zip");
    assert!(
        !zip_bytes.is_empty(),
        "roads.zip fixture should not be empty"
    );

    let boundary = "------------------------boundaryROADS";
    let body = multipart_body(boundary, "roads.zip", &zip_bytes);

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(file_item.name, "roads");
    assert_eq!(file_item.status, "uploaded");
    assert_eq!(file_item.file_type, "shapefile");

    let file_id = file_item.id;
    let ready_item = wait_until_ready(&app, &file_id).await;
    assert_eq!(ready_item.status, "ready");
    assert!(ready_item.table_name.is_some());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/preview", file_id))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/tiles/0/0/0", file_id))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/vnd.mapbox-vector-tile"
    );
    let tile_body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        !tile_body.is_empty(),
        "Expected non-empty MVT tile body for shapefile dataset"
    );
}

#[tokio::test]
async fn test_upload_kml_lifecycle() {
    let (app, _temp) = setup_app().await;

    let kml_bytes = read_fixture_bytes("testdata/sample/formats/sample.kml");
    let boundary = "------------------------boundaryKML";
    let body = multipart_body(boundary, "sample.kml", &kml_bytes);

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(file_item.name, "sample");
    assert_eq!(file_item.status, "uploaded");
    assert_eq!(file_item.file_type, "kml");

    let file_id = file_item.id;
    let ready_item = wait_until_ready(&app, &file_id).await;
    assert_eq!(ready_item.status, "ready");
    assert!(ready_item.table_name.is_some());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/preview", file_id))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/tiles/0/0/0", file_id))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let tile_body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        !tile_body.is_empty(),
        "Expected non-empty MVT tile body for KML"
    );
}

#[tokio::test]
async fn test_persistence_across_restart_keeps_ready_dataset() {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("persist.duckdb");
    let conn1 = init_database(&db_path);
    let db1 = Arc::new(tokio::sync::Mutex::new(conn1));
    let state1 = AppState {
        upload_dir: upload_dir.clone(),
        upload_dir_canonical: upload_dir_canonical.clone(),
        db: db1.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db1.clone()),
        session_store: DuckDBStore::new(db1),
    };
    let app1 = build_test_router(state1);

    let geojson_bytes = read_fixture_bytes("frontend/tests/fixtures/sample.geojson");
    let boundary = "------------------------boundaryPERSIST";
    let body = multipart_body(boundary, "sample.geojson", &geojson_bytes);

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let response = app1.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id;

    let ready_item = wait_until_ready(&app1, &file_id).await;
    assert_eq!(ready_item.status, "ready");

    drop(app1);

    // Simulate restart: new DB connection and router, same DB file + upload dir.
    let conn2 = init_database(&db_path);
    let db2 = Arc::new(tokio::sync::Mutex::new(conn2));
    reconcile_processing_files(&db2).await.unwrap();

    let state2 = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db2.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db2.clone()),
        session_store: DuckDBStore::new(db2),
    };
    let app2 = build_test_router(state2);

    let request = Request::builder()
        .method("GET")
        .uri("/api/files")
        .body(Body::empty())
        .unwrap();
    let response = app2.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let files: Vec<FileItem> = serde_json::from_slice(&body_bytes).unwrap();
    let persisted = files.iter().find(|f| f.id == file_id).expect("file exists");
    assert_eq!(persisted.status, "ready");
    assert!(persisted.table_name.is_some());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/tiles/0/0/0", file_id))
        .body(Body::empty())
        .unwrap();
    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/vnd.mapbox-vector-tile"
    );
}

// OSM Tile Golden Tests

#[derive(Debug, serde::Deserialize)]
struct SampleTile {
    z: u64,
    x: u64,
    y: u64,
    #[serde(rename = "type")]
    tile_type: String,
    expected_features: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct DatasetConfig {
    name: String,
    fixture: String,
    sample_tiles: Vec<SampleTile>,
}

#[derive(Debug, serde::Deserialize)]
struct OsmTestConfig {
    datasets: Vec<DatasetConfig>,
}

fn load_osm_test_config() -> OsmTestConfig {
    let path = repo_root().join("testdata/smoke/osm_tile_test_samples.json");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read OSM test config {:?}: {}", path, e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse OSM test config JSON {:?}: {}", path, e))
}

async fn test_tile_golden_samples_for_dataset(config: &DatasetConfig) {
    let (app, _temp) = setup_app_with_large_max_size().await;

    println!("Testing OSM tiles for dataset: {}", config.name);
    println!("  Fixture: {}", config.fixture);
    println!("  Sample tiles: {}", config.sample_tiles.len());

    // Upload fixture
    let fixture_bytes = read_fixture_bytes(&config.fixture);
    let boundary = "------------------------boundaryGOLDEN";
    let fixture_path = PathBuf::from(&config.fixture);
    let filename = fixture_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("fixture.geojson");
    let body = multipart_body(boundary, filename, &fixture_bytes);

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(Body::from(body))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();

    if status != axum::http::StatusCode::CREATED {
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error_msg = String::from_utf8_lossy(&body_bytes);
        panic!("Upload failed with status {}: {}", status, error_msg);
    }

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id;

    // Wait for ready
    let _ready_item = wait_until_ready(&app, &file_id).await;

    let mut update_commands = Vec::new();

    // Test each sample tile
    for sample in &config.sample_tiles {
        let z = sample.z;
        let x = sample.x;
        let y = sample.y;
        let tile_type = &sample.tile_type;

        // Fetch tile
        let request = Request::builder()
            .method("GET")
            .uri(format!("/api/files/{}/tiles/{}/{}/{}", file_id, z, x, y))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();

        if status != axum::http::StatusCode::OK {
            let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
            let error_msg = String::from_utf8_lossy(&body_bytes);
            panic!(
                "Tile request failed for {} z={} {} ({},{}): {} - {}",
                config.name, z, tile_type, x, y, status, error_msg
            );
        }

        let tile_body = response.into_body().collect().await.unwrap().to_bytes();
        let tile_bytes = tile_body.as_ref();

        // Verify tile is valid MVT
        assert!(
            !tile_bytes.is_empty(),
            "Tile should not be empty for {} z={} {} ({},{})",
            config.name,
            z,
            tile_type,
            x,
            y
        );

        let reader = MvtReader::new(tile_bytes.to_vec());
        assert!(
            reader.is_ok(),
            "Tile should be valid MVT for {} z={} {} ({},{})",
            config.name,
            z,
            tile_type,
            x,
            y
        );

        // Get feature count
        let feature_count = if let Ok(r) = reader {
            let features = r.get_features(0);
            if let Ok(feat_vec) = features {
                feat_vec.len()
            } else {
                0
            }
        } else {
            0
        };

        // Verify expected feature count
        match sample.expected_features {
            Some(expected) => {
                assert_eq!(
                    feature_count, expected,
                    "Feature count mismatch for {} z={} {} ({},{}): expected {}, got {}",
                    config.name, z, tile_type, x, y, expected, feature_count
                );
                println!(
                    "  ✓ z={} {} ({},{}): {} features",
                    tile_type, z, x, y, feature_count
                );
            }
            None => {
                // First run: output update command
                println!(
                    "  UPDATE NEEDED: z={} {} ({},{}): has {} features",
                    tile_type, z, x, y, feature_count
                );
                update_commands.push(format!(
                    "  {{\"z\": {}, \"x\": {}, \"y\": {}, \"type\": \"{}\", \"expected_features\": {}}}",
                    z, x, y, tile_type, feature_count
                ));
            }
        }
    }

    // If there are tiles without expected features, output update commands and panic
    if !update_commands.is_empty() {
        eprintln!("\n========== UPDATE REQUIRED ==========");
        eprintln!("Some tiles are missing expected_features. Update the config file:");
        eprintln!("\nFile: testdata/smoke/osm_tile_test_samples.json");
        eprintln!("\nDataset: {}", config.name);
        for cmd in &update_commands {
            eprintln!("{}", cmd);
        }
        eprintln!("\nThen re-run the test.");
        eprintln!("====================================\n");
        panic!("Golden file needs feature count updates. Run the commands above.");
    }

    println!("✓ All tiles match for {}", config.name);
}

// Sample-based OSM tile tests (default, fast ~3s)
#[tokio::test]
async fn test_tile_golden_osm_lines_samples() {
    let config = load_osm_test_config();
    let dataset_config = config
        .datasets
        .iter()
        .find(|d| d.name == "sf_lines")
        .expect("sf_lines dataset not found in config");
    test_tile_golden_samples_for_dataset(dataset_config).await;
}

#[tokio::test]
async fn test_tile_golden_osm_points_samples() {
    let config = load_osm_test_config();
    let dataset_config = config
        .datasets
        .iter()
        .find(|d| d.name == "sf_points")
        .expect("sf_points dataset not found in config");
    test_tile_golden_samples_for_dataset(dataset_config).await;
}

#[tokio::test]
async fn test_tile_golden_osm_polygons_samples() {
    let config = load_osm_test_config();
    let dataset_config = config
        .datasets
        .iter()
        .find(|d| d.name == "sf_polygons")
        .expect("sf_polygons dataset not found in config");
    test_tile_golden_samples_for_dataset(dataset_config).await;
}

#[tokio::test]
async fn test_tile_golden_osm_simple_polygons_samples() {
    let config = load_osm_test_config();
    let dataset_config = config
        .datasets
        .iter()
        .find(|d| d.name == "sf_simple_polygons")
        .expect("sf_simple_polygons dataset not found in config");
    test_tile_golden_samples_for_dataset(dataset_config).await;
}

#[tokio::test]
async fn test_tile_golden_osm_multipoints_samples() {
    let config = load_osm_test_config();
    let dataset_config = config
        .datasets
        .iter()
        .find(|d| d.name == "sf_multipoints")
        .expect("sf_multipoints dataset not found in config");
    test_tile_golden_samples_for_dataset(dataset_config).await;
}

#[tokio::test]
async fn test_tile_golden_osm_multilinestrings_samples() {
    let config = load_osm_test_config();
    let dataset_config = config
        .datasets
        .iter()
        .find(|d| d.name == "sf_multilinestrings")
        .expect("sf_multilinestrings dataset not found in config");
    test_tile_golden_samples_for_dataset(dataset_config).await;
}

// Database schema tests for authentication tables
#[test]
fn test_users_schema() {
    use backend::init_database;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let conn = init_database(&db_path);

    // Verify users table exists by querying it
    let result = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get::<_, i64>(0));
    // Table should exist even if empty (COUNT(*) returns 0)
    assert!(result.is_ok(), "users table should exist");

    // Verify we can query the structure using PRAGMA
    let mut stmt = conn.prepare("PRAGMA table_info(users)").unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(
        columns,
        vec!["id", "username", "password_hash", "role", "created_at"]
    );
}

#[test]
fn test_sessions_schema() {
    use backend::init_database;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let conn = init_database(&db_path);

    // Verify sessions table exists
    let result = conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| {
        row.get::<_, i64>(0)
    });
    assert!(result.is_ok(), "sessions table should exist");

    // Verify structure
    let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(columns, vec!["id", "data", "expiry_date", "created_at"]);
}

#[test]
fn test_system_settings_schema() {
    use backend::init_database;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let conn = init_database(&db_path);

    // Verify system_settings table exists
    let result = conn.query_row("SELECT COUNT(*) FROM system_settings", [], |row| {
        row.get::<_, i64>(0)
    });
    assert!(result.is_ok(), "system_settings table should exist");

    // Verify structure
    let mut stmt = conn.prepare("PRAGMA table_info(system_settings)").unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(columns, vec!["key", "value"]);
}

#[test]
fn test_is_initialized_not_set_by_default() {
    use backend::{init_database, is_initialized};

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let conn = init_database(&db_path);
    assert!(!is_initialized(&conn).unwrap());
}

#[test]
fn test_set_and_check_initialized() {
    use backend::{init_database, is_initialized, set_initialized};

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let conn = init_database(&db_path);
    assert!(!is_initialized(&conn).unwrap());

    set_initialized(&conn).unwrap();
    assert!(is_initialized(&conn).unwrap());
}

#[tokio::test]
async fn test_concurrent_init_system_requests() {
    use backend::{hash_password, init_database, is_initialized, set_initialized};
    use duckdb::params;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");
    let conn = Arc::new(Mutex::new(init_database(&db_path)));

    let make_init_request = |conn: Arc<Mutex<duckdb::Connection>>| {
        tokio::spawn(async move {
            let c = conn.lock().await;

            let tx_result = c.execute("BEGIN TRANSACTION", []);
            if tx_result.is_err() {
                return false;
            }

            let already_init = is_initialized(&c).unwrap_or(false);

            if already_init {
                let _ = c.execute("ROLLBACK", []);
                return false;
            }

            let password_hash = hash_password("Test123!@#").unwrap();
            let user_id = uuid::Uuid::new_v4().to_string();
            let created_at = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

            let result = c.execute(
                "INSERT INTO users (id, username, password_hash, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![&user_id, "admin", &password_hash, "admin", &created_at],
            );

            if result.is_err() {
                let _ = c.execute("ROLLBACK", []);
                return false;
            }

            let _ = set_initialized(&c);
            let _ = c.execute("COMMIT", []);

            true
        })
    };

    let tasks = (0..5)
        .map(|_| make_init_request(conn.clone()))
        .collect::<Vec<_>>();

    let mut success_count = 0;
    let mut failure_count = 0;

    for task in tasks {
        match task.await {
            Ok(true) => success_count += 1,
            Ok(false) => failure_count += 1,
            Err(_) => failure_count += 1,
        }
    }

    assert_eq!(
        success_count, 1,
        "Exactly one init request should succeed (got {})",
        success_count
    );

    assert_eq!(
        failure_count, 4,
        "All other requests should fail (got {})",
        failure_count
    );

    let conn = conn.lock().await;
    let user_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .unwrap();

    assert_eq!(
        user_count, 1,
        "Only one admin user should be created (got {})",
        user_count
    );
}

#[tokio::test]
async fn test_publish_file_with_custom_slug() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-custom-map"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["url"], "/tiles/my-custom-map/{z}/{x}/{y}");
    assert_eq!(body_json["slug"], "my-custom-map");
    assert_eq!(body_json["is_public"], true);
}

#[tokio::test]
async fn test_publish_file_with_default_slug() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    eprintln!(
        "Response JSON: {}",
        serde_json::to_string_pretty(&body_json).unwrap()
    );

    assert_eq!(
        body_json["url"],
        format!("/tiles/{}/{{z}}/{{x}}/{{y}}", file_id)
    );
    assert_eq!(body_json["slug"], file_id);
    assert_eq!(body_json["is_public"], true);
}

#[tokio::test]
async fn test_publish_file_with_empty_body_uses_file_id() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(
        body_json["url"],
        format!("/tiles/{}/{{z}}/{{x}}/{{y}}", file_id)
    );
    assert_eq!(body_json["slug"], file_id);
}

#[tokio::test]
async fn test_publish_file_already_published() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-map"}"#))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let publish_again_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "another-slug"}"#))
        .unwrap();

    let response = app.oneshot(publish_again_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("already published"));
}

#[tokio::test]
async fn test_publish_file_not_ready() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-map"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"].as_str().unwrap().contains("not ready"));
}

#[tokio::test]
async fn test_publish_file_slug_conflict() {
    let (app, _temp) = setup_app().await;

    let file_id_1 = upload_geojson_file(&app).await;
    let file_id_2 = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id_1).await;
    wait_until_ready(&app, &file_id_2).await;

    let publish_request_1 = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id_1))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "same-slug"}"#))
        .unwrap();

    let response = app.clone().oneshot(publish_request_1).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let publish_request_2 = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id_2))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "same-slug"}"#))
        .unwrap();

    let response = app.oneshot(publish_request_2).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("already in use"));
}

#[tokio::test]
async fn test_publish_file_invalid_slug() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "invalid slug!"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    eprintln!(
        "Error response for invalid slug: {}",
        body_json["error"].as_str().unwrap()
    );

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("Slug can only contain letters, numbers, hyphens, and underscores"));
}

#[tokio::test]
async fn test_publish_file_slug_too_long() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let long_slug = "a".repeat(101);

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(format!(r#"{{"slug": "{}"}}"#, long_slug)))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    eprintln!(
        "Error response for slug too long: {}",
        body_json["error"].as_str().unwrap()
    );

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("Slug must be 100 characters or less"));
}

#[tokio::test]
async fn test_unpublish_file() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-map"}"#))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let unpublish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/unpublish", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(unpublish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["message"], "File unpublished");
}

#[tokio::test]
async fn test_unpublish_file_not_published() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/public-url", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["error"], "File not published");
}

#[tokio::test]
async fn test_public_url_endpoint() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-map"}"#))
        .unwrap();

    app.clone().oneshot(publish_request).await.unwrap();

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/public-url", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["slug"], "my-map");
    assert_eq!(body_json["url"], "/tiles/my-map/{z}/{x}/{y}");
}

#[tokio::test]
async fn test_public_url_endpoint_not_published() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/public-url", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["error"], "File not published");
}

#[tokio::test]
async fn test_public_tiles_endpoint() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-map"}"#))
        .unwrap();

    app.clone().oneshot(publish_request).await.unwrap();

    let request = Request::builder()
        .method("GET")
        .uri("/tiles/my-map/10/527/351")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/vnd.mapbox-vector-tile"
    );
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "public, max-age=300"
    );

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(!body_bytes.is_empty(), "Tile data should not be empty");
}

#[tokio::test]
async fn test_public_tiles_endpoint_nonexistent_slug() {
    let (app, _temp) = setup_app().await;

    let request = Request::builder()
        .method("GET")
        .uri("/tiles/nonexistent-slug/10/527/351")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["error"], "Public tile not found");
}

#[tokio::test]
async fn test_public_tiles_endpoint_unpublished_file() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-map"}"#))
        .unwrap();

    app.clone().oneshot(publish_request).await.unwrap();

    let unpublish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/unpublish", file_id))
        .body(Body::empty())
        .unwrap();

    app.clone().oneshot(unpublish_request).await.unwrap();

    let request = Request::builder()
        .method("GET")
        .uri("/tiles/my-map/10/527/351")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

// Helper to create a minimal valid MBTiles file for testing
fn create_test_mbtiles(temp_dir: &Path, name: &str) -> PathBuf {
    create_test_mbtiles_with_format(temp_dir, name, "pbf")
}

// Helper to create an MBTiles file with specific format
fn create_test_mbtiles_with_format(temp_dir: &Path, name: &str, format: &str) -> PathBuf {
    use rusqlite::Connection;

    let mbtiles_path = temp_dir.join(format!("{}.mbtiles", name));
    let conn = Connection::open(&mbtiles_path).expect("Failed to create test MBTiles");

    // Create metadata table
    conn.execute(
        "CREATE TABLE metadata (name TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .expect("Failed to create metadata table");

    // Insert required metadata
    conn.execute(
        &format!(
            "INSERT INTO metadata (name, value) VALUES ('format', '{}')",
            format
        ),
        [],
    )
    .expect("Failed to insert format");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('name', ?1)",
        [name],
    )
    .expect("Failed to insert name");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('minzoom', '0')",
        [],
    )
    .expect("Failed to insert minzoom");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('maxzoom', '2')",
        [],
    )
    .expect("Failed to insert maxzoom");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('bounds', '-180.0,-85.0,180.0,85.0')",
        [],
    )
    .expect("Failed to insert bounds");

    // Create tiles table
    conn.execute(
        "CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB, PRIMARY KEY (zoom_level, tile_column, tile_row))",
        [],
    )
    .expect("Failed to create tiles table");

    // Insert a simple MVT tile at z=0, x=0, y=0 (TMS: y=0 for z=0)
    // This is a minimal valid MVT with an empty layer
    // Gzip compress it to match real-world MBTiles behavior
    let empty_mvt = vec![0x1d, 0x00, 0x08, 0x00]; // MVT magic + empty layer

    let gzipped_data = if format == "pbf" {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&empty_mvt).expect("Failed to compress");
        encoder.finish().expect("Failed to finish compression")
    } else {
        empty_mvt
    };

    conn.execute(
        "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (0, 0, 0, ?1)",
        [gzipped_data],
    )
    .expect("Failed to insert test tile");

    // Add json metadata with vector_layers
    let json_metadata = serde_json::json!({
        "vector_layers": [{
            "id": name,
            "description": format!("Test layer: {}", name),
            "minzoom": 0,
            "maxzoom": 2,
            "fields": {
                "name": "String",
                "class": "String",
                "speed_limit": "Number"
            }
        }]
    });

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('json', ?1)",
        [json_metadata.to_string()],
    )
    .expect("Failed to insert json metadata");

    mbtiles_path
}

// Helper to create an invalid MBTiles file (missing metadata table)
fn create_invalid_mbtiles(temp_dir: &Path) -> PathBuf {
    use rusqlite::Connection;

    let mbtiles_path = temp_dir.join("invalid.mbtiles");
    let conn = Connection::open(&mbtiles_path).expect("Failed to create test file");

    // Create only tiles table, missing metadata
    conn.execute(
        "CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB)",
        [],
    )
    .expect("Failed to create tiles table");

    mbtiles_path
}

// Helper to create MBTiles file with multiple layers
fn create_test_mbtiles_with_multiple_layers(temp_dir: &Path) -> PathBuf {
    use rusqlite::Connection;
    use std::io::Write;

    let mbtiles_path = temp_dir.join("multi_layer.mbtiles");
    let conn = Connection::open(&mbtiles_path).expect("Failed to create test MBTiles");

    // Create metadata table
    conn.execute(
        "CREATE TABLE metadata (name TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .expect("Failed to create metadata table");

    // Insert required metadata
    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('format', 'pbf')",
        [],
    )
    .expect("Failed to insert format");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('name', 'multi_layer_test')",
        [],
    )
    .expect("Failed to insert name");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('minzoom', '0')",
        [],
    )
    .expect("Failed to insert minzoom");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('maxzoom', '2')",
        [],
    )
    .expect("Failed to insert maxzoom");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('bounds', '-180.0,-85.0,180.0,85.0')",
        [],
    )
    .expect("Failed to insert bounds");

    // Create tiles table
    conn.execute(
        "CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB, PRIMARY KEY (zoom_level, tile_column, tile_row))",
        [],
    )
    .expect("Failed to create tiles table");

    // Insert a simple MVT tile at z=0, x=0, y=0
    let empty_mvt = vec![0x1d, 0x00, 0x08, 0x00];

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&empty_mvt).expect("Failed to compress");
    let gzipped_data = encoder.finish().expect("Failed to finish compression");

    conn.execute(
        "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (0, 0, 0, ?1)",
        [gzipped_data],
    )
    .expect("Failed to insert test tile");

    // Add json metadata with multiple vector_layers
    let json_metadata = serde_json::json!({
        "vector_layers": [
            {
                "id": "roads",
                "description": "Road network layer",
                "minzoom": 0,
                "maxzoom": 2,
                "fields": {
                    "name": "String",
                    "highway": "String",
                    "speed_limit": "Number"
                }
            },
            {
                "id": "buildings",
                "description": "Building footprint layer",
                "minzoom": 0,
                "maxzoom": 2,
                "fields": {
                    "height": "Number",
                    "building_type": "String",
                    "address": "String"
                }
            }
        ]
    });

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('json', ?1)",
        [json_metadata.to_string()],
    )
    .expect("Failed to insert json metadata");

    mbtiles_path
}

// Helper to create MBTiles file with malformed json
fn create_test_mbtiles_with_malformed_json(temp_dir: &Path) -> PathBuf {
    use rusqlite::Connection;
    use std::io::Write;

    let mbtiles_path = temp_dir.join("malformed_json.mbtiles");
    let conn = Connection::open(&mbtiles_path).expect("Failed to create test MBTiles");

    conn.execute(
        "CREATE TABLE metadata (name TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .expect("Failed to create metadata table");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('format', 'pbf')",
        [],
    )
    .expect("Failed to insert format");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('name', 'malformed_test')",
        [],
    )
    .expect("Failed to insert name");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('minzoom', '0')",
        [],
    )
    .expect("Failed to insert minzoom");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('maxzoom', '2')",
        [],
    )
    .expect("Failed to insert maxzoom");

    conn.execute(
        "CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB, PRIMARY KEY (zoom_level, tile_column, tile_row))",
        [],
    )
    .expect("Failed to create tiles table");

    let empty_mvt = vec![0x1d, 0x00, 0x08, 0x00];

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&empty_mvt).expect("Failed to compress");
    let gzipped_data = encoder.finish().expect("Failed to finish compression");

    conn.execute(
        "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (0, 0, 0, ?1)",
        [gzipped_data],
    )
    .expect("Failed to insert test tile");

    // Insert malformed JSON (missing closing brace)
    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('json', ?1)",
        ["{\"vector_layers\": [{\"id\": \"test\"}]"], // malformed JSON
    )
    .expect("Failed to insert json metadata");

    mbtiles_path
}

// Helper to create MBTiles file without json metadata
fn create_mbtiles_without_json(temp_dir: &Path) -> PathBuf {
    use rusqlite::Connection;

    let mbtiles_path = temp_dir.join("no_json.mbtiles");
    let conn = Connection::open(&mbtiles_path).expect("Failed to create test MBTiles");

    conn.execute(
        "CREATE TABLE metadata (name TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .expect("Failed to create metadata table");

    conn.execute(
        "INSERT INTO metadata (name, value) VALUES ('format', 'pbf')",
        [],
    )
    .expect("Failed to insert format");

    conn.execute(
        "CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB, PRIMARY KEY (zoom_level, tile_column, tile_row))",
        [],
    )
    .expect("Failed to create tiles table");

    let empty_mvt = vec![0x1d, 0x00, 0x08, 0x00];
    conn.execute(
        "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (0, 0, 0, ?1)",
        [empty_mvt],
    )
    .expect("Failed to insert test tile");

    mbtiles_path
}

#[tokio::test]
async fn test_upload_mbtiles_success() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_tiles");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_tiles.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(file_item.file_type, "mbtiles");
    assert_eq!(file_item.status, "uploaded");

    // Wait for processing to complete
    let file = wait_until_ready(&app, &file_item.id).await;
    assert_eq!(file.status, "ready");
}

#[tokio::test]
async fn test_mbtiles_tile_returns_correct_format() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_tiles");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_tiles.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Request tile
    let tile_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/tiles/0/0/0", file_item.id))
        .body(Body::empty())
        .unwrap();

    let tile_response = app.oneshot(tile_request).await.unwrap();

    // Should return 200 with MVT content type
    assert_eq!(tile_response.status(), axum::http::StatusCode::OK);
    let content_type = tile_response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok());
    assert_eq!(content_type, Some("application/vnd.mapbox-vector-tile"));
    let content_encoding = tile_response
        .headers()
        .get("content-encoding")
        .and_then(|v| v.to_str().ok());
    assert_eq!(content_encoding, Some("gzip"));
}

#[tokio::test]
async fn test_public_mbtiles_png_returns_correct_content_type() {
    let (app, temp) = setup_app().await;

    // Create PNG MBTiles
    let mbtiles_path = create_test_mbtiles_with_format(temp.path(), "test_png", "png");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_png.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Publish the file
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-png-tiles"}"#))
        .unwrap();

    let publish_response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(publish_response.status(), axum::http::StatusCode::OK);

    // Request public tile
    let tile_request = Request::builder()
        .method("GET")
        .uri("/tiles/my-png-tiles/0/0/0")
        .body(Body::empty())
        .unwrap();

    let tile_response = app.oneshot(tile_request).await.unwrap();

    // Should return 200 with PNG content type
    assert_eq!(tile_response.status(), axum::http::StatusCode::OK);
    let content_type = tile_response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok());
    assert_eq!(content_type, Some("image/png"));
    assert!(tile_response.headers().get("content-encoding").is_none());
}

#[tokio::test]
async fn test_mbtiles_tile_beyond_maxzoom_returns_204() {
    let (app, temp) = setup_app().await;

    // Create MBTiles with maxzoom=2
    let mbtiles_path = create_test_mbtiles(temp.path(), "test_tiles");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_tiles.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Request tile beyond maxzoom (maxzoom=2 in test data)
    let tile_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/tiles/3/0/0", file_item.id))
        .body(Body::empty())
        .unwrap();

    let tile_response = app.oneshot(tile_request).await.unwrap();

    // Should return 204 No Content (tile doesn't exist)
    assert_eq!(tile_response.status(), axum::http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_mbtiles_empty_tile_returns_204() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_tiles");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_tiles.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Request non-existent tile
    let tile_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/tiles/1/0/0", file_item.id))
        .body(Body::empty())
        .unwrap();

    let tile_response = app.oneshot(tile_request).await.unwrap();

    // Should return 204 No Content for missing tiles
    assert_eq!(tile_response.status(), axum::http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_mbtiles_preview_includes_bounds() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_tiles");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_tiles.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Get preview metadata
    let preview_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/preview", file_item.id))
        .body(Body::empty())
        .unwrap();

    let preview_response = app.oneshot(preview_request).await.unwrap();
    assert_eq!(preview_response.status(), axum::http::StatusCode::OK);

    let preview_bytes = preview_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let preview: serde_json::Value = serde_json::from_slice(&preview_bytes).unwrap();

    // Should include bbox from metadata
    assert!(preview["bbox"].is_array());
    let bbox = preview["bbox"].as_array().unwrap();
    assert_eq!(bbox.len(), 4);
    // Check bounds are approximately what we set (-180.0,-85.0,180.0,85.0)
    assert!((bbox[0].as_f64().unwrap() - (-180.0)).abs() < 0.01);
    assert!((bbox[1].as_f64().unwrap() - (-85.0)).abs() < 0.01);
    assert!((bbox[2].as_f64().unwrap() - 180.0).abs() < 0.01);
    assert!((bbox[3].as_f64().unwrap() - 85.0).abs() < 0.01);

    // Should include tileFormat
    assert_eq!(preview["tileFormat"], "mvt");

    // Should include minZoom and maxZoom from MBTiles metadata
    assert_eq!(preview["minZoom"], 0);
    assert_eq!(preview["maxZoom"], 2);
}

#[tokio::test]
async fn test_invalid_mbtiles_returns_400() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_invalid_mbtiles(temp.path());
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"invalid.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return 400 Bad Request
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("MBTiles file missing metadata table"));
}

#[tokio::test]
async fn test_mbtiles_feature_properties_returns_400() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_tiles");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_tiles.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Try to get feature properties (should fail for MBTiles)
    let props_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/features/1", file_item.id))
        .body(Body::empty())
        .unwrap();

    let props_response = app.oneshot(props_request).await.unwrap();

    // Should return 400 Bad Request
    assert_eq!(props_response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = props_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("Feature properties not available for MBTiles files"));
}

#[tokio::test]
async fn test_mbtiles_schema_returns_layers() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_tiles");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_tiles.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Get schema
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_item.id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.oneshot(schema_request).await.unwrap();

    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);

    let schema_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&schema_bytes).unwrap();

    // Verify layers structure
    assert!(schema["layers"].is_array());
    let layers = schema["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 1);

    let layer = &layers[0];
    assert_eq!(layer["id"], "test_tiles");
    assert_eq!(layer["description"], "Test layer: test_tiles");

    // Verify fields
    assert!(layer["fields"].is_array());
    let fields = layer["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 3);

    // Verify field types are lowercase
    let field_map: std::collections::HashMap<&str, &str> = fields
        .iter()
        .filter_map(|f| f["name"].as_str().zip(f["type"].as_str()))
        .collect();

    assert_eq!(field_map.get("name"), Some(&"string"));
    assert_eq!(field_map.get("class"), Some(&"string"));
    assert_eq!(field_map.get("speed_limit"), Some(&"number"));
}

#[tokio::test]
async fn test_mbtiles_schema_without_json_returns_empty() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_mbtiles_without_json(temp.path());
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"no_json.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Get schema
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_item.id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.oneshot(schema_request).await.unwrap();
    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);

    let schema_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&schema_bytes).unwrap();

    // Should return empty layers array
    assert!(schema["layers"].is_array());
    assert_eq!(schema["layers"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_mbtiles_schema_with_multiple_layers() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles_with_multiple_layers(temp.path());
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"multi_layer.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Get schema
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_item.id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.oneshot(schema_request).await.unwrap();
    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);

    let schema_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&schema_bytes).unwrap();

    // Verify layers structure
    assert!(schema["layers"].is_array());
    let layers = schema["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2, "Should have 2 layers");

    // Verify first layer (roads)
    let roads_layer = &layers[0];
    assert_eq!(roads_layer["id"], "roads");
    assert_eq!(roads_layer["description"], "Road network layer");
    let roads_fields = roads_layer["fields"].as_array().unwrap();
    assert_eq!(roads_fields.len(), 3);

    let roads_field_map: std::collections::HashMap<&str, &str> = roads_fields
        .iter()
        .filter_map(|f| f["name"].as_str().zip(f["type"].as_str()))
        .collect();

    assert_eq!(roads_field_map.get("name"), Some(&"string"));
    assert_eq!(roads_field_map.get("highway"), Some(&"string"));
    assert_eq!(roads_field_map.get("speed_limit"), Some(&"number"));

    // Verify second layer (buildings)
    let buildings_layer = &layers[1];
    assert_eq!(buildings_layer["id"], "buildings");
    assert_eq!(buildings_layer["description"], "Building footprint layer");
    let buildings_fields = buildings_layer["fields"].as_array().unwrap();
    assert_eq!(buildings_fields.len(), 3);

    let buildings_field_map: std::collections::HashMap<&str, &str> = buildings_fields
        .iter()
        .filter_map(|f| f["name"].as_str().zip(f["type"].as_str()))
        .collect();

    assert_eq!(buildings_field_map.get("height"), Some(&"number"));
    assert_eq!(buildings_field_map.get("building_type"), Some(&"string"));
    assert_eq!(buildings_field_map.get("address"), Some(&"string"));
}

#[tokio::test]
async fn test_mbtiles_schema_with_malformed_json() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles_with_malformed_json(temp.path());
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"malformed_json.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Get schema - should return empty layers due to parse error
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_item.id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.oneshot(schema_request).await.unwrap();
    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);

    let schema_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&schema_bytes).unwrap();

    // Should return empty layers array (graceful degradation)
    assert!(schema["layers"].is_array());
    assert_eq!(schema["layers"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_mbtiles_publish_and_public_tiles() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_tiles");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_tiles.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Publish the file
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "my-tiles"}"#))
        .unwrap();

    let publish_response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(publish_response.status(), axum::http::StatusCode::OK);

    // Request public tile
    let tile_request = Request::builder()
        .method("GET")
        .uri("/tiles/my-tiles/0/0/0")
        .body(Body::empty())
        .unwrap();

    let tile_response = app.oneshot(tile_request).await.unwrap();

    // Should return 200 with MVT content type
    assert_eq!(tile_response.status(), axum::http::StatusCode::OK);
    let content_type = tile_response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok());
    assert_eq!(content_type, Some("application/vnd.mapbox-vector-tile"));
}

#[tokio::test]
async fn test_health_check() {
    let (app, _temp) = setup_app().await;

    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["status"], "ok");
}

fn create_test_pmtiles(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    use std::io::Write;
    let path = dir.join(format!("{}.pmtiles", name));
    let mut file = std::fs::File::create(&path).unwrap();
    // PMTiles header magic bytes + minimal data
    file.write_all(b"PM").unwrap();
    file.write_all(&[0x00, 0x01]).unwrap(); // version
    file.write_all(&[0u8; 100]).unwrap(); // padding to make it look like a real file
    path
}

#[tokio::test]
async fn test_pmtiles_upload_and_publish() {
    let (app, temp) = setup_app().await;

    let pmtiles_path = create_test_pmtiles(temp.path(), "test_pmtiles");
    let pmtiles_bytes = std::fs::read(&pmtiles_path).expect("Failed to read test PMTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_pmtiles.pmtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&pmtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    assert_eq!(upload_response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(file_item.file_type, "pmtiles");

    wait_until_ready(&app, &file_item.id).await;

    // Publish the file
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"my-pmtiles"}"#))
        .unwrap();

    let publish_response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(publish_response.status(), axum::http::StatusCode::OK);

    let publish_body = publish_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let publish_json: serde_json::Value = serde_json::from_slice(&publish_body).unwrap();
    assert_eq!(publish_json["slug"], "my-pmtiles");
    assert_eq!(publish_json["url"], "/tiles/my-pmtiles");
}

#[tokio::test]
async fn test_pmtiles_public_url_endpoint() {
    let (app, temp) = setup_app().await;

    let pmtiles_path = create_test_pmtiles(temp.path(), "public_url_pmtiles");
    let pmtiles_bytes = std::fs::read(&pmtiles_path).expect("Failed to read test PMTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"public_url_pmtiles.pmtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&pmtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    assert_eq!(upload_response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"pmtiles-public-url"}"#))
        .unwrap();

    let publish_response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(publish_response.status(), axum::http::StatusCode::OK);

    let public_url_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/public-url", file_item.id))
        .body(Body::empty())
        .unwrap();

    let public_url_response = app.oneshot(public_url_request).await.unwrap();
    assert_eq!(public_url_response.status(), axum::http::StatusCode::OK);

    let public_url_bytes = public_url_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let public_url_json: serde_json::Value = serde_json::from_slice(&public_url_bytes).unwrap();
    assert_eq!(public_url_json["slug"], "pmtiles-public-url");
    assert_eq!(public_url_json["url"], "/tiles/pmtiles-public-url");
}

#[tokio::test]
async fn test_pmtiles_meta_endpoint() {
    let (app, temp) = setup_app().await;

    let pmtiles_path = create_test_pmtiles(temp.path(), "meta_test");
    let pmtiles_bytes = std::fs::read(&pmtiles_path).expect("Failed to read test PMTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"meta_test.pmtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&pmtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Publish
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"meta-test"}"#))
        .unwrap();

    let _ = app.clone().oneshot(publish_request).await.unwrap();

    // Get meta
    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/meta-test/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.clone().oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["slug"], "meta-test");
    assert_eq!(meta_json["name"], "meta_test");
    assert_eq!(meta_json["tileSource"], "pmtiles");
    assert_eq!(meta_json["tileUrl"], "/tiles/meta-test");
    // Test builds do not bundle the frontend viewer, so public meta omits viewerUrl here.
    assert!(meta_json.get("viewerUrl").is_none());
}

#[tokio::test]
async fn test_pmtiles_range_request() {
    let (app, temp) = setup_app().await;

    let pmtiles_path = create_test_pmtiles(temp.path(), "range_test");
    let pmtiles_bytes = std::fs::read(&pmtiles_path).expect("Failed to read test PMTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"range_test.pmtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&pmtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Publish
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"range-test"}"#))
        .unwrap();

    let _ = app.clone().oneshot(publish_request).await.unwrap();

    // HEAD request
    let head_request = Request::builder()
        .method("HEAD")
        .uri("/tiles/range-test")
        .body(Body::empty())
        .unwrap();

    let head_response = app.clone().oneshot(head_request).await.unwrap();
    assert_eq!(head_response.status(), axum::http::StatusCode::OK);
    assert!(head_response.headers().contains_key("content-length"));
    assert!(head_response.headers().contains_key("accept-ranges"));

    // Range request
    let range_request = Request::builder()
        .method("GET")
        .uri("/tiles/range-test")
        .header("range", "bytes=0-3")
        .body(Body::empty())
        .unwrap();

    let range_response = app.oneshot(range_request).await.unwrap();
    assert_eq!(
        range_response.status(),
        axum::http::StatusCode::PARTIAL_CONTENT
    );
    assert!(range_response.headers().contains_key("content-range"));
}

#[tokio::test]
async fn test_pmtiles_range_request_with_relative_upload_dir() {
    let (app, temp, _upload_temp) = setup_app_with_relative_upload_dir().await;

    let pmtiles_path = create_test_pmtiles(temp.path(), "range_relative_test");
    let pmtiles_bytes = std::fs::read(&pmtiles_path).expect("Failed to read test PMTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"range_relative_test.pmtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&pmtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"range-relative-test"}"#))
        .unwrap();

    let _ = app.clone().oneshot(publish_request).await.unwrap();

    let head_request = Request::builder()
        .method("HEAD")
        .uri("/tiles/range-relative-test")
        .body(Body::empty())
        .unwrap();

    let head_response = app.clone().oneshot(head_request).await.unwrap();
    assert_eq!(head_response.status(), axum::http::StatusCode::OK);
    assert!(head_response.headers().contains_key("content-length"));
    assert!(head_response.headers().contains_key("accept-ranges"));

    let range_request = Request::builder()
        .method("GET")
        .uri("/tiles/range-relative-test")
        .header("range", "bytes=0-3")
        .body(Body::empty())
        .unwrap();

    let range_response = app.oneshot(range_request).await.unwrap();
    assert_eq!(
        range_response.status(),
        axum::http::StatusCode::PARTIAL_CONTENT
    );
    assert!(range_response.headers().contains_key("content-range"));
}

#[tokio::test]
async fn test_pmtiles_meta_for_duckdb_file() {
    let (app, _temp) = setup_app().await;

    let geojson_bytes = read_fixture_bytes("frontend/tests/fixtures/sample.geojson");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"duckdb_meta_test.geojson\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&geojson_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    // Publish
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"duckdb-meta-test"}"#))
        .unwrap();

    let _ = app.clone().oneshot(publish_request).await.unwrap();

    // Get meta - should show duckdb tile source
    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/duckdb-meta-test/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.clone().oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["tileSource"], "duckdb");
    assert_eq!(meta_json["tileUrl"], "/tiles/duckdb-meta-test/{z}/{x}/{y}");
    assert_eq!(meta_json["bbox"], serde_json::json!([0.0, 0.0, 0.1, 0.1]));
}

#[tokio::test]
async fn test_publish_dynamic_data_with_zoom() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "zoom-test", "minZoom": 2, "maxZoom": 10}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Zoom values for dynamic data are stored in published_files table,
    // not in files table. Verify via public tile meta endpoint.
    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/zoom-test/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.clone().oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["minZoom"], 2);
    assert_eq!(meta_json["maxZoom"], 10);
}

#[tokio::test]
async fn test_publish_mbtiles_with_zoom_fails() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_zoom");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_zoom.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "mbtiles-zoom-test", "minZoom": 0, "maxZoom": 5}"#,
        ))
        .unwrap();

    let response = app.oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("Zoom levels can only be set for dynamic vector data"));
}

#[tokio::test]
async fn test_update_tile_zoom_success() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "update-zoom-test"}"#))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let update_zoom_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/zoom", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"minZoom": 5, "maxZoom": 15}"#))
        .unwrap();

    let response = app.clone().oneshot(update_zoom_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Zoom values for dynamic data are stored in published_files table,
    // verify via public tile meta endpoint
    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/update-zoom-test/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.clone().oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["minZoom"], 5);
    assert_eq!(meta_json["maxZoom"], 15);
}

#[tokio::test]
async fn test_update_tile_zoom_partial_update() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "partial-zoom-test", "minZoom": 2, "maxZoom": 10}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let update_zoom_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/zoom", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"maxZoom": 18}"#))
        .unwrap();

    let response = app.clone().oneshot(update_zoom_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Zoom values for dynamic data are stored in published_files table,
    // verify via public tile meta endpoint
    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/partial-zoom-test/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["minZoom"], 2);
    assert_eq!(meta_json["maxZoom"], 18);
}

#[tokio::test]
async fn test_update_tile_zoom_partial_invalid_fails() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "invalid-partial-test", "minZoom": 2, "maxZoom": 10}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let update_zoom_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/zoom", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"minZoom": 15}"#))
        .unwrap();

    let response = app.oneshot(update_zoom_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("minZoom must be less than or equal to maxZoom"));
}

#[tokio::test]
async fn test_update_tile_zoom_not_published_fails() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let update_zoom_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/zoom", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"minZoom": 5, "maxZoom": 15}"#))
        .unwrap();

    let response = app.oneshot(update_zoom_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("must be published"));
}

#[tokio::test]
async fn test_update_tile_zoom_mbtiles_fails() {
    let (app, temp) = setup_app().await;

    let mbtiles_path = create_test_mbtiles(temp.path(), "test_update_zoom");
    let mbtiles_bytes = std::fs::read(&mbtiles_path).expect("Failed to read test MBTiles");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test_update_zoom.mbtiles\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&mbtiles_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    wait_until_ready(&app, &file_item.id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "mbtiles-update-zoom"}"#))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let update_zoom_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/zoom", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"minZoom": 0, "maxZoom": 5}"#))
        .unwrap();

    let response = app.oneshot(update_zoom_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("Zoom levels can only be updated for dynamic vector data"));
}

#[tokio::test]
async fn test_public_tile_respects_zoom_limits() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "zoom-limit-test", "minZoom": 5, "maxZoom": 10}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let tile_request_within = Request::builder()
        .method("GET")
        .uri("/tiles/zoom-limit-test/7/0/0")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(tile_request_within).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let tile_request_below = Request::builder()
        .method("GET")
        .uri("/tiles/zoom-limit-test/3/0/0")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(tile_request_below).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);

    let tile_request_above = Request::builder()
        .method("GET")
        .uri("/tiles/zoom-limit-test/12/0/0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(tile_request_above).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_public_tile_respects_minzoom_only() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "minzoom-only-test", "minZoom": 5}"#))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let tile_request_within = Request::builder()
        .method("GET")
        .uri("/tiles/minzoom-only-test/7/0/0")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(tile_request_within).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let tile_request_below = Request::builder()
        .method("GET")
        .uri("/tiles/minzoom-only-test/3/0/0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(tile_request_below).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_public_tile_respects_maxzoom_only() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "maxzoom-only-test", "maxZoom": 10}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let tile_request_within = Request::builder()
        .method("GET")
        .uri("/tiles/maxzoom-only-test/7/0/0")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(tile_request_within).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let tile_request_above = Request::builder()
        .method("GET")
        .uri("/tiles/maxzoom-only-test/12/0/0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(tile_request_above).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_public_tile_meta_includes_zoom() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "meta-zoom-test", "minZoom": 3, "maxZoom": 12}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/meta-zoom-test/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["minZoom"], 3);
    assert_eq!(meta_json["maxZoom"], 12);
}

#[tokio::test]
async fn test_public_tile_meta_includes_extended_fields() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "meta-extended-test"}"#))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/meta-extended-test/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["slug"], "meta-extended-test");
    assert!(meta_json["name"].as_str().is_some());
    assert_eq!(meta_json["tileSource"], "duckdb");
    assert!(meta_json["tileUrl"]
        .as_str()
        .unwrap()
        .contains("/tiles/meta-extended-test/"));
    // Test builds do not bundle the frontend viewer, so public meta omits viewerUrl here.
    assert!(meta_json.get("viewerUrl").is_none());
    assert_eq!(meta_json["crsType"], "standard");
}

#[tokio::test]
async fn test_publish_zoom_below_range() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "zoom-below-range", "minZoom": -1, "maxZoom": 10}"#,
        ))
        .unwrap();

    let response = app.oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("minZoom must be between 0 and 22"));
}

#[tokio::test]
async fn test_publish_zoom_above_range() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "zoom-above-range", "minZoom": 0, "maxZoom": 23}"#,
        ))
        .unwrap();

    let response = app.oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("maxZoom must be between 0 and 22"));
}

#[tokio::test]
async fn test_update_tile_zoom_out_of_range() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug": "update-out-of-range"}"#))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let update_zoom_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/zoom", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"minZoom": 25, "maxZoom": 30}"#))
        .unwrap();

    let response = app.oneshot(update_zoom_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("minZoom must be between 0 and 22"));
}

#[tokio::test]
async fn test_unpublish_clears_zoom() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "unpublish-clears-zoom", "minZoom": 5, "maxZoom": 15}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Verify zoom values are stored in published_files via public tile meta endpoint
    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/unpublish-clears-zoom/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.clone().oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["minZoom"], 5);
    assert_eq!(meta_json["maxZoom"], 15);

    let unpublish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/unpublish", file_id))
        .body(Body::empty())
        .unwrap();

    let unpublish_response = app.clone().oneshot(unpublish_request).await.unwrap();
    assert_eq!(unpublish_response.status(), axum::http::StatusCode::OK);

    // After unpublish, the public tile meta endpoint should return 404
    // because published_files record is deleted
    let meta_request2 = Request::builder()
        .method("GET")
        .uri("/tiles/unpublish-clears-zoom/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response2 = app.oneshot(meta_request2).await.unwrap();
    assert_eq!(meta_response2.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_update_crs_success() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let update_crs_request = Request::builder()
        .method("PUT")
        .uri(format!("/api/files/{}/crs", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"crs": "EPSG:3857"}"#))
        .unwrap();

    let response = app.clone().oneshot(update_crs_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["id"], file_id);
    assert_eq!(result["crs"], "EPSG:3857");
    assert_eq!(result["crsType"], "standard");

    let list_request = Request::builder()
        .method("GET")
        .uri("/api/files")
        .body(Body::empty())
        .unwrap();

    let list_response = app.oneshot(list_request).await.unwrap();
    let body_bytes = list_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let files: Vec<FileItem> = serde_json::from_slice(&body_bytes).unwrap();

    let updated_file = files.iter().find(|f| f.id == file_id).unwrap();
    assert_eq!(updated_file.crs, Some("EPSG:3857".to_string()));
    assert_eq!(updated_file.crs_type, Some("standard".to_string()));
}

#[tokio::test]
async fn test_update_crs_with_epsg_urn_normalizes_to_standard() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let update_crs_request = Request::builder()
        .method("PUT")
        .uri(format!("/api/files/{}/crs", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"crs": "urn:ogc:def:crs:EPSG::4490"}"#))
        .unwrap();

    let response = app.clone().oneshot(update_crs_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["id"], file_id);
    assert_eq!(result["crs"], "EPSG:4490");
    assert_eq!(result["crsType"], "standard");
}

#[tokio::test]
async fn test_public_tile_meta_includes_bbox_for_epsg4490() {
    let (app, _temp) = setup_app().await;

    let geojson_bytes = read_fixture_bytes("frontend/tests/fixtures/sample.geojson");

    let boundary = "------------------------boundaryXYZ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"epsg4490_test.geojson\"\r\n\r\n",
    );

    let mut body = body_data.into_bytes();
    body.extend_from_slice(&geojson_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    assert_eq!(upload_response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id;
    wait_until_ready(&app, &file_id).await;

    let update_crs_request = Request::builder()
        .method("PUT")
        .uri(format!("/api/files/{}/crs", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"crs": "EPSG:4490"}"#))
        .unwrap();

    let update_response = app.clone().oneshot(update_crs_request).await.unwrap();
    assert_eq!(update_response.status(), axum::http::StatusCode::OK);

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"epsg4490-docs-test"}"#))
        .unwrap();

    let publish_response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(publish_response.status(), axum::http::StatusCode::OK);

    let meta_request = Request::builder()
        .method("GET")
        .uri("/tiles/epsg4490-docs-test/meta")
        .body(Body::empty())
        .unwrap();

    let meta_response = app.clone().oneshot(meta_request).await.unwrap();
    assert_eq!(meta_response.status(), axum::http::StatusCode::OK);

    let meta_body = meta_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let meta_json: serde_json::Value = serde_json::from_slice(&meta_body).unwrap();

    assert_eq!(meta_json["crs"], "EPSG:4490");
    assert_eq!(meta_json["crsType"], "standard");

    let bbox = meta_json["bbox"]
        .as_array()
        .expect("bbox should be an array");
    assert_eq!(bbox.len(), 4);

    let minx = bbox[0].as_f64().unwrap();
    let miny = bbox[1].as_f64().unwrap();
    let maxx = bbox[2].as_f64().unwrap();
    let maxy = bbox[3].as_f64().unwrap();

    assert!(maxx > minx);
    assert!(maxy > miny);
    assert!(minx >= -180.0 && maxx <= 180.0);
    assert!(miny >= -90.0 && maxy <= 90.0);

    let tile_request = Request::builder()
        .method("GET")
        .uri("/tiles/epsg4490-docs-test/0/0/0")
        .body(Body::empty())
        .unwrap();

    let tile_response = app.clone().oneshot(tile_request).await.unwrap();
    assert_eq!(tile_response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn test_update_crs_to_custom() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let update_crs_request = Request::builder()
        .method("PUT")
        .uri(format!("/api/files/{}/crs", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"crs": "LOCAL_GRID_2024"}"#))
        .unwrap();

    let response = app.clone().oneshot(update_crs_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["crs"], "LOCAL_GRID_2024");
    assert_eq!(result["crsType"], "custom");
}

#[tokio::test]
async fn test_update_crs_file_not_found() {
    let (app, _temp) = setup_app().await;

    let update_crs_request = Request::builder()
        .method("PUT")
        .uri("/api/files/non-existent-id/crs")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"crs": "EPSG:4326"}"#))
        .unwrap();

    let response = app.oneshot(update_crs_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_update_crs_missing_field() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let update_crs_request = Request::builder()
        .method("PUT")
        .uri(format!("/api/files/{}/crs", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{}"#))
        .unwrap();

    let response = app.oneshot(update_crs_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_update_crs_empty_string_sets_null() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let update_crs_request = Request::builder()
        .method("PUT")
        .uri(format!("/api/files/{}/crs", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"crs": ""}"#))
        .unwrap();

    let response = app.clone().oneshot(update_crs_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(result["crs"].is_null());
    assert_eq!(result["crsType"], "custom");
}

#[tokio::test]
async fn test_update_crs_not_ready_returns_409() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;

    let update_crs_request = Request::builder()
        .method("PUT")
        .uri(format!("/api/files/{}/crs", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"crs": "EPSG:4326"}"#))
        .unwrap();

    let response = app.oneshot(update_crs_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
}

// ============================================================================
// Field Aliases Tests
// ============================================================================

#[tokio::test]
async fn test_update_field_aliases_success() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    // First, get the schema to find a field's normalized name
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    assert_eq!(schema_response.status(), axum::http::StatusCode::OK);

    let body_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let first_field = &schema["layers"][0]["fields"][0];
    let normalized_name = first_field["normalized"]
        .as_str()
        .unwrap_or_else(|| first_field["name"].as_str().unwrap());

    // Update field alias
    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"fields": [{{"normalized_name": "{}", "alias": "显示名称"}}]}}"#,
            normalized_name
        )))
        .unwrap();

    let response = app.oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["success"], true);
}

#[tokio::test]
async fn test_update_field_aliases_schema_returns_alias() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    // Get initial schema
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    let body_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let first_field = &schema["layers"][0]["fields"][0];
    let normalized_name = first_field["normalized"]
        .as_str()
        .unwrap_or_else(|| first_field["name"].as_str().unwrap());

    // Verify alias is null initially
    assert!(first_field["alias"].is_null() || first_field.get("alias").is_none());

    // Update field alias
    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"fields": [{{"normalized_name": "{}", "alias": "MyAlias"}}]}}"#,
            normalized_name
        )))
        .unwrap();

    let response = app.clone().oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Get schema again and verify alias is set
    let schema_request2 = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let schema_response2 = app.oneshot(schema_request2).await.unwrap();
    let body_bytes2 = schema_response2
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema2: serde_json::Value = serde_json::from_slice(&body_bytes2).unwrap();

    let updated_field = schema2["layers"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| {
            f["normalized"]
                .as_str()
                .unwrap_or_else(|| f["name"].as_str().unwrap())
                == normalized_name
        })
        .unwrap();

    assert_eq!(updated_field["alias"], "MyAlias");
}

#[tokio::test]
async fn test_update_field_aliases_empty_rejected() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"fields": [{"normalized_name": "name", "alias": "   "}]}"#,
        ))
        .unwrap();

    let response = app.oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"]
        .as_str()
        .unwrap()
        .contains("empty string"));
}

#[tokio::test]
async fn test_update_field_aliases_too_long() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let long_alias = "x".repeat(256);
    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"fields": [{{"normalized_name": "name", "alias": "{}"}}]}}"#,
            long_alias
        )))
        .unwrap();

    let response = app.oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"].as_str().unwrap().contains("255"));
}

#[tokio::test]
async fn test_update_field_aliases_field_not_found() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"fields": [{"normalized_name": "nonexistent_field", "alias": "test"}]}"#,
        ))
        .unwrap();

    let response = app.oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(body_json["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_update_field_aliases_file_not_ready() {
    let (app, _temp) = setup_app().await;

    // Upload but don't wait for ready
    let boundary = "------------------------boundaryNotReady";
    let geojson_content = r#"{
        "type": "FeatureCollection",
        "features": [{"type": "Feature", "properties": {"name": "test"}, "geometry": {"type": "Point", "coordinates": [0, 0]}}]
    }"#;
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.geojson\"\r\n\r\n{geojson_content}\r\n--{boundary}--\r\n"
    );

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(upload_request).await.unwrap();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();

    // Try to update aliases before ready
    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_item.id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"fields": [{"normalized_name": "name", "alias": "test"}]}"#,
        ))
        .unwrap();

    let response = app.oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_update_field_aliases_file_not_found() {
    let (app, _temp) = setup_app().await;

    let update_request = Request::builder()
        .method("PATCH")
        .uri("/api/files/nonexistent-file-id/field-aliases")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"fields": [{"normalized_name": "name", "alias": "test"}]}"#,
        ))
        .unwrap();

    let response = app.oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_update_field_aliases_clear_alias() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    // Get schema to find field
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    let body_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let first_field = &schema["layers"][0]["fields"][0];
    let normalized_name = first_field["normalized"]
        .as_str()
        .unwrap_or_else(|| first_field["name"].as_str().unwrap());

    // Set alias
    let set_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"fields": [{{"normalized_name": "{}", "alias": "TestAlias"}}]}}"#,
            normalized_name
        )))
        .unwrap();

    let response = app.clone().oneshot(set_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Clear alias by setting to null
    let clear_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"fields": [{{"normalized_name": "{}", "alias": null}}]}}"#,
            normalized_name
        )))
        .unwrap();

    let response = app.clone().oneshot(clear_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Verify alias is cleared
    let schema_request2 = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let schema_response2 = app.oneshot(schema_request2).await.unwrap();
    let body_bytes2 = schema_response2
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema2: serde_json::Value = serde_json::from_slice(&body_bytes2).unwrap();

    let updated_field = schema2["layers"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| {
            f["normalized"]
                .as_str()
                .unwrap_or_else(|| f["name"].as_str().unwrap())
                == normalized_name
        })
        .unwrap();

    assert!(updated_field["alias"].is_null() || updated_field.get("alias").is_none());
}

#[tokio::test]
async fn test_feature_properties_returns_alias() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    // Get schema to find field
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    let body_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let first_field = &schema["layers"][0]["fields"][0];
    let normalized_name = first_field["normalized"]
        .as_str()
        .unwrap_or_else(|| first_field["name"].as_str().unwrap());
    let original_name = first_field["name"].as_str().unwrap();

    // Set alias
    let alias_value = "自定义别名";
    let set_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"fields": [{{"normalized_name": "{}", "alias": "{}"}}]}}"#,
            normalized_name, alias_value
        )))
        .unwrap();

    let set_response = app.clone().oneshot(set_request).await.unwrap();
    assert_eq!(set_response.status(), axum::http::StatusCode::OK);

    // Get feature properties and verify alias is returned
    let props_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/features/1", file_id))
        .body(Body::empty())
        .unwrap();

    let props_response = app.oneshot(props_request).await.unwrap();
    assert_eq!(props_response.status(), axum::http::StatusCode::OK);

    let body_bytes = props_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let props_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(props_json["fid"], 1);
    let props = props_json["properties"]
        .as_array()
        .expect("properties array");

    // Find the field with alias
    let field_with_alias = props
        .iter()
        .find(|p| p["key"].as_str() == Some(original_name))
        .expect("field should exist");

    assert_eq!(field_with_alias["alias"].as_str(), Some(alias_value));
}

#[tokio::test]
async fn test_update_publish_settings_success() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    // Publish with useAliases=true
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "settings-test", "useAliases": true}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Update useAliases to false
    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/publish-settings", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"useAliases": false}"#))
        .unwrap();

    let response = app.clone().oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["id"], file_id);
    assert_eq!(result["useAliases"], false);

    // Verify the setting persists via file list
    let list_request = Request::builder()
        .method("GET")
        .uri("/api/files")
        .body(Body::empty())
        .unwrap();

    let list_response = app.oneshot(list_request).await.unwrap();
    let list_bytes = list_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let files: Vec<FileItem> = serde_json::from_slice(&list_bytes).unwrap();

    let file = files.iter().find(|f| f.id == file_id).unwrap();
    assert_eq!(file.use_aliases, Some(false));
}

#[tokio::test]
async fn test_update_publish_settings_not_published() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    // Try to update settings without publishing first
    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/publish-settings", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"useAliases": false}"#))
        .unwrap();

    let response = app.oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(error["error"]
        .as_str()
        .unwrap()
        .contains("must be published"));
}

#[tokio::test]
async fn test_update_publish_settings_file_not_found() {
    let (app, _temp) = setup_app().await;

    let update_request = Request::builder()
        .method("PATCH")
        .uri("/api/files/nonexistent-id/publish-settings")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"useAliases": false}"#))
        .unwrap();

    let response = app.oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_republish_with_different_use_aliases() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    // First publish with useAliases=true
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "republish-test", "useAliases": true}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["useAliases"], true);

    // Verify file list shows use_aliases=true
    let list_request = Request::builder()
        .method("GET")
        .uri("/api/files")
        .body(Body::empty())
        .unwrap();

    let list_response = app.clone().oneshot(list_request).await.unwrap();
    let list_bytes = list_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let files: Vec<FileItem> = serde_json::from_slice(&list_bytes).unwrap();
    let file = files.iter().find(|f| f.id == file_id).unwrap();
    assert_eq!(file.use_aliases, Some(true));

    // Unpublish
    let unpublish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/unpublish", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(unpublish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Republish with useAliases=false
    let republish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "republish-test-2", "useAliases": false}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(republish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["useAliases"], false);

    // Verify file list shows use_aliases=false
    let list_request = Request::builder()
        .method("GET")
        .uri("/api/files")
        .body(Body::empty())
        .unwrap();

    let list_response = app.oneshot(list_request).await.unwrap();
    let list_bytes = list_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let files: Vec<FileItem> = serde_json::from_slice(&list_bytes).unwrap();
    let file = files.iter().find(|f| f.id == file_id).unwrap();
    assert_eq!(file.use_aliases, Some(false));
}

#[tokio::test]
async fn test_public_tile_uses_alias() {
    let (app, _temp) = setup_app().await;

    let file_id = upload_geojson_file(&app).await;
    wait_until_ready(&app, &file_id).await;

    // Set field alias
    let schema_request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let schema_response = app.clone().oneshot(schema_request).await.unwrap();
    let schema_bytes = schema_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let schema: serde_json::Value = serde_json::from_slice(&schema_bytes).unwrap();

    let first_field = &schema["layers"][0]["fields"][0];
    let normalized_name = first_field["normalized"]
        .as_str()
        .unwrap_or_else(|| first_field["name"].as_str().unwrap());
    let original_name = first_field["name"].as_str().expect("original field name");

    let set_alias_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/field-aliases", file_id))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"fields": [{{"normalized_name": "{}", "alias": "MyAlias"}}]}}"#,
            normalized_name
        )))
        .unwrap();

    let set_alias_response = app.clone().oneshot(set_alias_request).await.unwrap();
    assert_eq!(set_alias_response.status(), axum::http::StatusCode::OK);

    // Publish with useAliases=true
    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/files/{}/publish", file_id))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"slug": "alias-test-true", "useAliases": true}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Request public tile - should use alias
    let tile_request = Request::builder()
        .method("GET")
        .uri("/tiles/alias-test-true/0/0/0")
        .body(Body::empty())
        .unwrap();

    let tile_response = app.clone().oneshot(tile_request).await.unwrap();
    assert_eq!(tile_response.status(), axum::http::StatusCode::OK);

    let tile_bytes = tile_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();

    // Parse MVT and check for alias in property keys
    let reader = MvtReader::new(tile_bytes.to_vec()).expect("Valid MVT tile");
    let features = reader.get_features(0).expect("Features");

    let has_alias_key = features.iter().any(|f| {
        f.properties
            .as_ref()
            .map(|props| props.contains_key("MyAlias"))
            .unwrap_or(false)
    });
    assert!(
        has_alias_key,
        "Tile should have 'MyAlias' as property key when useAliases=true"
    );

    // Update to useAliases=false
    let update_request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/files/{}/publish-settings", file_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"useAliases": false}"#))
        .unwrap();

    let response = app.clone().oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Request public tile again - should use original name
    let tile_request2 = Request::builder()
        .method("GET")
        .uri("/tiles/alias-test-true/0/0/0")
        .body(Body::empty())
        .unwrap();

    let tile_response2 = app.oneshot(tile_request2).await.unwrap();
    assert_eq!(tile_response2.status(), axum::http::StatusCode::OK);

    let tile_bytes2 = tile_response2
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();

    // Parse MVT and check for original name in property keys (not alias)
    let reader2 = MvtReader::new(tile_bytes2.to_vec()).expect("Valid MVT tile");
    let features2 = reader2.get_features(0).expect("Features");

    let has_original_key = features2.iter().any(|f| {
        f.properties
            .as_ref()
            .map(|props| props.contains_key(original_name))
            .unwrap_or(false)
    });
    let has_alias_key2 = features2.iter().any(|f| {
        f.properties
            .as_ref()
            .map(|props| props.contains_key("MyAlias"))
            .unwrap_or(false)
    });
    assert!(
        has_original_key,
        "Tile should have original name as property key when useAliases=false"
    );
    assert!(
        !has_alias_key2,
        "Tile should NOT have alias as property key when useAliases=false"
    );
}

async fn create_user_and_session(
    app: &axum::Router,
    db: Arc<tokio::sync::Mutex<duckdb::Connection>>,
    user_id: &str,
    username: &str,
    role: &str,
) -> String {
    use time;
    use tower_sessions::session::{Id, Record};

    let password_hash = "$2b$12$EixZaYVK1fsbw1ZfbX3OXePaWxn96p36IgQE0VrqQ6EJdNpO5mLY";

    {
        let conn = db.lock().await;
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at) VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP) ON CONFLICT (id) DO UPDATE SET username = excluded.username, password_hash = excluded.password_hash, role = excluded.role",
            duckdb::params![user_id, username, password_hash, role],
        ).unwrap();
    }

    let id = Id::default();
    let expiry_date = time::OffsetDateTime::now_utc() + time::Duration::hours(24);

    let auth_hash = password_hash.as_bytes().to_vec();

    let mut auth_data = HashMap::new();
    auth_data.insert("user_id".to_string(), serde_json::json!(user_id));
    auth_data.insert("auth_hash".to_string(), serde_json::json!(auth_hash));

    let mut session_data = HashMap::new();
    session_data.insert("axum-login.data".to_string(), serde_json::json!(auth_data));

    let record = Record {
        id,
        data: session_data,
        expiry_date,
    };

    let session_id = record.id.to_string();
    let data_json = serde_json::to_string(&record.data).unwrap();
    let expiry_date_str = chrono::DateTime::from_timestamp(record.expiry_date.unix_timestamp(), 0)
        .unwrap()
        .to_rfc3339();

    let request = Request::builder()
        .method("POST")
        .uri("/api/test/session")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "session_id": session_id,
                "data": data_json,
                "expiry_date": expiry_date_str
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    format!("id={}", session_id)
}

#[tokio::test]
async fn test_get_settings_returns_current_max_size() {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };
    let app = build_api_router(state);

    let cookie = create_user_and_session(&app, db, "user-1", "testuser", "admin").await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/settings")
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let settings: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(settings["maxSizeMb"], 10);
}

#[tokio::test]
async fn test_get_settings_requires_auth() {
    let (app, _temp) = setup_app_with_auth().await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/settings")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_update_settings_requires_admin() {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };
    let app = build_api_router(state);

    let cookie = create_user_and_session(&app, db, "user-1", "testuser", "user").await;

    let request = Request::builder()
        .method("PATCH")
        .uri("/api/settings")
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"maxSizeMb": 20}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_postgis_connection_test_requires_admin() {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };
    let app = build_api_router(state);

    let cookie = create_user_and_session(&app, db, "user-1", "testuser", "user").await;

    let request = Request::builder()
        .method("POST")
        .uri("/api/postgis/connections/test")
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "connection": {
                    "host": "127.0.0.1",
                    "port": 5432,
                    "database": "postgres",
                    "username": "postgres",
                    "password": "postgres",
                    "sslMode": "disable"
                }
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_postgis_register_requires_admin() {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };
    let app = build_api_router(state);

    let cookie = create_user_and_session(&app, db, "user-1", "testuser", "user").await;

    let request = Request::builder()
        .method("POST")
        .uri("/api/postgis/sources/register")
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "connectionName": "demo",
                "connection": {
                    "host": "127.0.0.1",
                    "port": 5432,
                    "database": "postgres",
                    "username": "postgres",
                    "password": "postgres",
                    "sslMode": "disable"
                },
                "schema": "public",
                "object": "roads",
                "geometryColumn": "geom",
                "fidColumn": "id",
                "displayName": "Demo"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_update_settings_success() {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };
    let app = build_api_router(state.clone());

    let cookie = create_user_and_session(&app, db.clone(), "admin-1", "admin", "admin").await;

    let request = Request::builder()
        .method("PATCH")
        .uri("/api/settings")
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"maxSizeMb": 20}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let settings: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(settings["maxSizeMb"], 20);

    assert_eq!(*state.max_size.read().await, 20 * 1024 * 1024);
    assert_eq!(*state.max_size_label.read().await, "20MB");
}

#[tokio::test]
async fn test_update_settings_rejects_zero() {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };
    let app = build_api_router(state);

    let cookie = create_user_and_session(&app, db, "admin-1", "admin", "admin").await;

    let request = Request::builder()
        .method("PATCH")
        .uri("/api/settings")
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"maxSizeMb": 0}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(error["error"].as_str().unwrap().contains("at least"));
}

#[tokio::test]
async fn test_update_settings_rejects_exceeds_max() {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };
    let app = build_api_router(state);

    let cookie = create_user_and_session(&app, db, "admin-1", "admin", "admin").await;

    let request = Request::builder()
        .method("PATCH")
        .uri("/api/settings")
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"maxSizeMb": 200000}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(error["error"].as_str().unwrap().contains("at most"));
}

#[tokio::test]
async fn test_settings_persisted_to_database() {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("persist.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(10 * 1024 * 1024)),
        max_size_label: Arc::new(RwLock::new("10MB".to_string())),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db.clone()),
    };
    let app = build_api_router(state);

    let cookie = create_user_and_session(&app, db.clone(), "admin-1", "admin", "admin").await;

    let request = Request::builder()
        .method("PATCH")
        .uri("/api/settings")
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"maxSizeMb": 50}"#))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let conn = db.lock().await;
    let value: String = conn
        .query_row(
            "SELECT value FROM system_settings WHERE key = 'upload_max_size_mb'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, "50");
}
