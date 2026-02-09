use axum::body::Body;
use axum::http::Request;
use backend::{
    build_api_router, init_database, reconcile_processing_files, AppState, FileItem,
    FileSchemaResponse, PROCESSING_RECONCILIATION_ERROR,
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

// Helper to setup the app for testing
async fn setup_app() -> (axum::Router, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);

    let state = AppState {
        upload_dir,
        db: Arc::new(tokio::sync::Mutex::new(conn)),
        max_size: 10 * 1024 * 1024, // 10MB
        max_size_label: "10MB".to_string(),
    };

    let router = build_api_router(state);
    (router, temp_dir)
}

async fn setup_app_with_large_max_size() -> (axum::Router, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);

    let state = AppState {
        upload_dir,
        db: Arc::new(tokio::sync::Mutex::new(conn)),
        max_size: 100 * 1024 * 1024, // 100MB for OSM datasets
        max_size_label: "100MB".to_string(),
    };

    let router = build_api_router(state);
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
}

#[tokio::test]
async fn test_upload_payload_too_large_returns_413() {
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");

    let db_path = temp_dir.path().join("test.duckdb");
    let conn = init_database(&db_path);

    let state = AppState {
        upload_dir,
        db: Arc::new(tokio::sync::Mutex::new(conn)),
        max_size: 1024, // 1KB
        max_size_label: "1KB".to_string(),
    };

    let app = build_api_router(state);

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

    let state = AppState {
        upload_dir,
        db: Arc::new(tokio::sync::Mutex::new(conn)),
        max_size: 10 * 1024 * 1024,
        max_size_label: "10MB".to_string(),
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

    let app = build_api_router(state);
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

    let state = AppState {
        upload_dir,
        db: Arc::new(tokio::sync::Mutex::new(conn)),
        max_size: 10 * 1024 * 1024,
        max_size_label: "10MB".to_string(),
    };

    let app = build_api_router(state.clone());

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
    let state1 = AppState {
        upload_dir: upload_dir.clone(),
        db: Arc::new(tokio::sync::Mutex::new(conn1)),
        max_size: 10 * 1024 * 1024,
        max_size_label: "10MB".to_string(),
    };
    let app1 = build_api_router(state1);

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
        db: db2,
        max_size: 10 * 1024 * 1024,
        max_size_label: "10MB".to_string(),
    };
    let app2 = build_api_router(state2);

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
async fn test_upload_geojsonseq_lifecycle() {
    let (app, _temp) = setup_app().await;

    // GeoJSONSeq content: one Feature per line
    let geojsonseq_content = r#"{"type":"Feature","properties":{"name":"Point1"},"geometry":{"type":"Point","coordinates":[0.0,0.0]}}
{"type":"Feature","properties":{"name":"Point2"},"geometry":{"type":"Point","coordinates":[1.0,1.0]}}
{"type":"Feature","properties":{"name":"Point3"},"geometry":{"type":"Point","coordinates":[2.0,2.0]}}"#;

    let boundary = "------------------------boundaryGEOJSONSEQ";
    let body_data = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.geojsonl\"\r\n\r\n{geojsonseq_content}\r\n--{boundary}--\r\n"
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
    let file_id = file_item.id.clone();

    // Verify file type
    assert_eq!(file_item.file_type, "geojsonl");
    assert_eq!(file_item.name, "test");

    // Wait for processing to complete
    let ready_item = wait_until_ready(&app, &file_id).await;
    assert_eq!(ready_item.status, "ready");

    // Verify we can query the schema
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let schema: FileSchemaResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert!(schema.fields.iter().any(|f| f.name == "name"));

    // Verify tile endpoint returns data (ensures features were actually imported)
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
        "Expected non-empty MVT tile body for GeoJSONSeq data"
    );

    // Verify at least one feature has the expected "name" property
    assert!(
        mvt_has_string_tag(&tile_body, "name", "Point1"),
        "Expected MVT to include tag name=Point1"
    );
}

#[tokio::test]
async fn test_upload_kml_lifecycle() {
    let (app, _temp) = setup_app().await;

    let kml_bytes = read_fixture_bytes("testdata/sample/formats/sample.kml");
    let boundary = "------------------------boundaryKML";
    let body_data = multipart_body(boundary, "test.kml", &kml_bytes);

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id.clone();

    assert_eq!(file_item.file_type, "kml");
    assert_eq!(file_item.name, "test");

    let ready_item = wait_until_ready(&app, &file_id).await;
    assert_eq!(ready_item.status, "ready");

    // Verify schema query works
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let schema: FileSchemaResponse = serde_json::from_slice(&body_bytes).unwrap();
    // KML may use different field names, just verify schema is populated
    assert!(!schema.fields.is_empty(), "KML schema should have fields");

    // Verify tile endpoint returns data (ensures features were actually imported)
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
        "Expected non-empty MVT tile body for KML data"
    );

    // Verify MVT tile has at least one feature with properties
    let reader = MvtReader::new(tile_body.to_vec()).expect("valid MVT");
    let layers = reader.get_layer_names().expect("layers");
    let mut found_feature_with_props = false;
    for (layer_idx, _) in layers.iter().enumerate() {
        if let Ok(features) = reader.get_features(layer_idx) {
            for f in features {
                if f.properties.is_some() && !f.properties.as_ref().unwrap().is_empty() {
                    found_feature_with_props = true;
                    break;
                }
            }
            if found_feature_with_props {
                break;
            }
        }
    }
    assert!(
        found_feature_with_props,
        "Expected MVT to have at least one feature with properties"
    );
}

#[tokio::test]
async fn test_upload_gpx_lifecycle() {
    let (app, _temp) = setup_app().await;

    let gpx_bytes = read_fixture_bytes("testdata/sample/formats/sample.gpx");
    let boundary = "------------------------boundaryGPX";
    let body_data = multipart_body(boundary, "test.gpx", &gpx_bytes);

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id.clone();

    assert_eq!(file_item.file_type, "gpx");
    assert_eq!(file_item.name, "test");

    let ready_item = wait_until_ready(&app, &file_id).await;
    assert_eq!(ready_item.status, "ready");

    // Verify schema query works
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let schema: FileSchemaResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert!(!schema.fields.is_empty());

    // Verify tile endpoint returns data (ensures features were actually imported)
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
        "Expected non-empty MVT tile body for GPX data"
    );

    // Verify MVT tile has at least one feature with properties
    let reader = MvtReader::new(tile_body.to_vec()).expect("valid MVT");
    let layers = reader.get_layer_names().expect("layers");
    let mut found_feature_with_props = false;
    for (layer_idx, _) in layers.iter().enumerate() {
        if let Ok(features) = reader.get_features(layer_idx) {
            for f in features {
                if f.properties.is_some() && !f.properties.as_ref().unwrap().is_empty() {
                    found_feature_with_props = true;
                    break;
                }
            }
            if found_feature_with_props {
                break;
            }
        }
    }
    assert!(
        found_feature_with_props,
        "Expected MVT to have at least one feature with properties"
    );
}

#[tokio::test]
async fn test_upload_topojson_lifecycle() {
    let (app, _temp) = setup_app().await;

    let topojson_bytes = read_fixture_bytes("testdata/sample/formats/sample.topojson");
    let boundary = "------------------------boundaryTOPOJSON";
    let body_data = multipart_body(boundary, "test.topojson", &topojson_bytes);

    let request = Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body_data))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let file_item: FileItem = serde_json::from_slice(&body_bytes).unwrap();
    let file_id = file_item.id.clone();

    assert_eq!(file_item.file_type, "topojson");
    assert_eq!(file_item.name, "test");

    let ready_item = wait_until_ready(&app, &file_id).await;
    assert_eq!(ready_item.status, "ready");

    // Verify schema query works
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/files/{}/schema", file_id))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let schema: FileSchemaResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert!(!schema.fields.is_empty());

    // Verify tile endpoint returns data (ensures features were actually imported)
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
        "Expected non-empty MVT tile body for TopoJSON data"
    );

    // Verify MVT tile has at least one feature with properties
    let reader = MvtReader::new(tile_body.to_vec()).expect("valid MVT");
    let layers = reader.get_layer_names().expect("layers");
    let mut found_feature_with_props = false;
    for (layer_idx, _) in layers.iter().enumerate() {
        if let Ok(features) = reader.get_features(layer_idx) {
            for f in features {
                if f.properties.is_some() && !f.properties.as_ref().unwrap().is_empty() {
                    found_feature_with_props = true;
                    break;
                }
            }
            if found_feature_with_props {
                break;
            }
        }
    }
    assert!(
        found_feature_with_props,
        "Expected MVT to have at least one feature with properties"
    );
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
