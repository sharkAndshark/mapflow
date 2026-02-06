use axum::body::Body;
use axum::http::Request;
use backend::{
    build_api_router, init_database, reconcile_processing_files, AppState, FileItem,
    PROCESSING_RECONCILIATION_ERROR,
};
use http_body_util::BodyExt; // for collect()
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

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    let mut shift = 0;
    while *pos < buf.len() && shift <= 63 {
        let b = buf[*pos];
        *pos += 1;
        result |= ((b & 0x7f) as u64) << shift;
        if (b & 0x80) == 0 {
            return Some(result);
        }
        shift += 7;
    }
    None
}

fn read_len_delimited<'a>(buf: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    let len = read_varint(buf, pos)? as usize;
    if *pos + len > buf.len() {
        return None;
    }
    let out = &buf[*pos..*pos + len];
    *pos += len;
    Some(out)
}

fn skip_field(buf: &[u8], pos: &mut usize, wire_type: u8) -> Option<()> {
    match wire_type {
        0 => {
            let _ = read_varint(buf, pos)?;
            Some(())
        }
        1 => {
            *pos = pos.checked_add(8)?;
            if *pos > buf.len() {
                return None;
            }
            Some(())
        }
        2 => {
            let _ = read_len_delimited(buf, pos)?;
            Some(())
        }
        5 => {
            *pos = pos.checked_add(4)?;
            if *pos > buf.len() {
                return None;
            }
            Some(())
        }
        _ => None,
    }
}

fn parse_packed_u32(buf: &[u8]) -> Option<Vec<u32>> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < buf.len() {
        let v = read_varint(buf, &mut pos)?;
        out.push(v as u32);
    }
    Some(out)
}

fn layer_has_string_tag(layer: &[u8], want_key: &str, want_value: &str) -> bool {
    // Collect keys and values; then scan features for tags.
    let mut keys: Vec<String> = Vec::new();
    let mut values: Vec<Option<String>> = Vec::new();
    let mut features: Vec<Vec<u32>> = Vec::new();

    let mut pos = 0;
    while pos < layer.len() {
        let key = match read_varint(layer, &mut pos) {
            Some(v) => v,
            None => return false,
        };
        let field = (key >> 3) as u32;
        let wire = (key & 0x07) as u8;

        match (field, wire) {
            // keys: repeated string
            (3, 2) => {
                let s = match read_len_delimited(layer, &mut pos) {
                    Some(b) => b,
                    None => return false,
                };
                let s = match std::str::from_utf8(s) {
                    Ok(v) => v.to_string(),
                    Err(_) => return false,
                };
                keys.push(s);
            }
            // values: repeated message
            (4, 2) => {
                let msg = match read_len_delimited(layer, &mut pos) {
                    Some(b) => b,
                    None => return false,
                };
                values.push(parse_value_string(msg));
            }
            // features: repeated message
            (2, 2) => {
                let msg = match read_len_delimited(layer, &mut pos) {
                    Some(b) => b,
                    None => return false,
                };
                if let Some(tags) = parse_feature_tags(msg) {
                    features.push(tags);
                }
            }
            _ => {
                if skip_field(layer, &mut pos, wire).is_none() {
                    return false;
                }
            }
        }
    }

    let Some(want_key_index) = keys.iter().position(|k| k == want_key) else {
        return false;
    };

    for tags in features {
        // tags are pairs: [key_index, value_index, ...]
        for pair in tags.chunks_exact(2) {
            let k = pair[0] as usize;
            let v = pair[1] as usize;
            if k == want_key_index {
                if let Some(Some(s)) = values.get(v) {
                    if s == want_value {
                        return true;
                    }
                }
            }
        }
    }

    false
}

fn parse_feature_tags(feature: &[u8]) -> Option<Vec<u32>> {
    let mut pos = 0;
    while pos < feature.len() {
        let key = read_varint(feature, &mut pos)?;
        let field = (key >> 3) as u32;
        let wire = (key & 0x07) as u8;
        match (field, wire) {
            // tags: packed uint32
            (2, 2) => {
                let packed = read_len_delimited(feature, &mut pos)?;
                return parse_packed_u32(packed);
            }
            _ => {
                skip_field(feature, &mut pos, wire)?;
            }
        }
    }
    None
}

fn parse_value_string(value: &[u8]) -> Option<String> {
    let mut pos = 0;
    while pos < value.len() {
        let key = read_varint(value, &mut pos)?;
        let field = (key >> 3) as u32;
        let wire = (key & 0x07) as u8;
        match (field, wire) {
            // string_value
            (1, 2) => {
                let s = read_len_delimited(value, &mut pos)?;
                let s = std::str::from_utf8(s).ok()?.to_string();
                return Some(s);
            }
            _ => {
                skip_field(value, &mut pos, wire)?;
            }
        }
    }
    None
}

fn mvt_has_string_tag(tile: &[u8], want_key: &str, want_value: &str) -> bool {
    let mut pos = 0;
    while pos < tile.len() {
        let key = match read_varint(tile, &mut pos) {
            Some(v) => v,
            None => return false,
        };
        let field = (key >> 3) as u32;
        let wire = (key & 0x07) as u8;
        match (field, wire) {
            // layers: repeated message
            (3, 2) => {
                let layer = match read_len_delimited(tile, &mut pos) {
                    Some(b) => b,
                    None => return false,
                };
                if layer_has_string_tag(layer, want_key, want_value) {
                    return true;
                }
            }
            _ => {
                if skip_field(tile, &mut pos, wire).is_none() {
                    return false;
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
        "Unsupported file type. Use .zip or .geojson"
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
