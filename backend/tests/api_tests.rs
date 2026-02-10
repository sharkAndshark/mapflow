use axum::body::Body;
use axum::http::Request;
use backend::{
    build_test_router, init_database, reconcile_processing_files, AppState, AuthBackend,
    DuckDBStore, FileItem, PROCESSING_RECONCILIATION_ERROR,
};
use http_body_util::BodyExt; // for collect()
use mvt_reader::{feature::Value as MvtValue, Reader as MvtReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt; // for oneshot

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

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        db: db.clone(),
        max_size: 10 * 1024 * 1024, // 10MB
        max_size_label: "10MB".to_string(),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db),
    };

    let router = build_test_router(state);
    (router, temp_dir)
}

async fn setup_app_with_large_max_size() -> (axum::Router, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        db: db.clone(),
        max_size: 100 * 1024 * 1024, // 100MB for OSM datasets
        max_size_label: "100MB".to_string(),
        auth_backend: AuthBackend::new(db.clone()),
        session_store: DuckDBStore::new(db),
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

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        db: db.clone(),
        max_size: 1024, // 1KB
        max_size_label: "1KB".to_string(),
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
        let options = zip::write::FileOptions::default();
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

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        db: db.clone(),
        max_size: 10 * 1024 * 1024,
        max_size_label: "10MB".to_string(),
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
        "Unsupported file type. Use .zip, .geojson, .json, .geojsonl, .kml, .gpx, or .topojson"
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
    assert!(body_json["fields"].is_array());

    let fields = body_json["fields"]
        .as_array()
        .expect("fields should be an array");

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
async fn test_schema_endpoint_returns_409_for_non_ready_file() {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);
    let db = Arc::new(tokio::sync::Mutex::new(conn));

    let state = AppState {
        upload_dir,
        db: db.clone(),
        max_size: 10 * 1024 * 1024,
        max_size_label: "10MB".to_string(),
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
    assert!(body_json["fields"].is_array());

    let fields = body_json["fields"]
        .as_array()
        .expect("fields should be an array");

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
    assert!(body_json["fields"].is_array());

    let fields = body_json["fields"]
        .as_array()
        .expect("fields should be an array");

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
async fn test_persistence_across_restart_keeps_ready_dataset() {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");

    let db_path = temp_dir.path().join("persist.duckdb");
    let conn1 = init_database(&db_path);
    let db1 = Arc::new(tokio::sync::Mutex::new(conn1));
    let state1 = AppState {
        upload_dir: upload_dir.clone(),
        db: db1.clone(),
        max_size: 10 * 1024 * 1024,
        max_size_label: "10MB".to_string(),
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

    // Simulate restart: new DB connection and router, same DB file + upload dir.
    let conn2 = init_database(&db_path);
    let db2 = Arc::new(tokio::sync::Mutex::new(conn2));
    reconcile_processing_files(&db2).await.unwrap();

    let state2 = AppState {
        upload_dir,
        db: db2.clone(),
        max_size: 10 * 1024 * 1024,
        max_size_label: "10MB".to_string(),
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
