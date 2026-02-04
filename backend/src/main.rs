use axum::{
    extract::{Multipart, State},
    http::{Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
    sync::Mutex,
};
use tower_http::{cors::{Any, CorsLayer}, services::ServeDir};
use zip::ZipArchive;

const MAX_SIZE: u64 = 200 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    upload_dir: PathBuf,
    index_path: PathBuf,
    index_lock: Arc<Mutex<()>>,
    max_size: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct FileItem {
    id: String,
    name: String,
    #[serde(rename = "type")]
    file_type: String,
    size: u64,
    #[serde(rename = "uploadedAt")]
    uploaded_at: String,
    status: String,
    crs: Option<String>,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ErrorResponse {
    error: String,
}

#[tokio::main]
async fn main() {
    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string());
    let upload_dir = PathBuf::from(upload_dir);
    let index_path = upload_dir.join("index.json");

    ensure_upload_dir(&upload_dir, &index_path).await;
    normalize_index_on_start(&index_path).await;

    let state = AppState {
        upload_dir,
        index_path,
        index_lock: Arc::new(Mutex::new(())),
        max_size: MAX_SIZE,
    };

    let mut app = build_api_router(state.clone());

    let web_dist = std::env::var("WEB_DIST").unwrap_or_else(|_| "frontend/dist".to_string());
    let web_dist_path = PathBuf::from(web_dist);
    if web_dist_path.exists() {
        app = app.fallback_service(ServeDir::new(web_dist_path));
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{port}");
    println!("MapFlow server running at http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server failed");
}

fn build_api_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    Router::new()
        .route("/api/files", get(list_files))
        .route("/api/uploads", post(upload_file))
        .with_state(state)
        .layer(cors)
}

async fn list_files(State(state): State<AppState>) -> impl IntoResponse {
    let _guard = with_index_lock(&state).await;
    let items = load_index(&state.index_path).await;
    drop(_guard);
    Json(items)
}

async fn upload_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let mut field = loop {
        let next = multipart.next_field().await.map_err(internal_error)?;
        match next {
            Some(field) if field.name() == Some("file") => break field,
            Some(_) => continue,
            None => return Err(bad_request("No file uploaded")),
        }
    };

    let original_name = field
        .file_name()
        .map(|name| name.to_string())
        .ok_or_else(|| bad_request("Missing file name"))?;
    let safe_name = Path::new(&original_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| bad_request("Invalid file name"))?
        .to_string();

    let ext = Path::new(&safe_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext.to_lowercase()))
        .ok_or_else(|| bad_request("Unsupported file type. Use .zip or .geojson"))?;

    let file_type = match ext.as_str() {
        ".zip" => "shapefile",
        ".geojson" => "geojson",
        _ => return Err(bad_request("Unsupported file type. Use .zip or .geojson")),
    };

    let upload_id = create_id();
    let dir = state.upload_dir.join(&upload_id);
    fs::create_dir_all(&dir).await.map_err(internal_error)?;

    let file_path = dir.join(&safe_name);
    let mut file = BufWriter::new(
        fs::File::create(&file_path)
            .await
            .map_err(internal_error)?,
    );

    let mut size: u64 = 0;
    while let Some(chunk) = field.chunk().await.map_err(internal_error)? {
        size = size.saturating_add(chunk.len() as u64);
        if size > state.max_size {
            drop(file);
            let _ = fs::remove_file(&file_path).await;
            return Err(payload_too_large("File too large (max 200MB)"));
        }
        file.write_all(&chunk).await.map_err(internal_error)?;
    }
    file.flush().await.map_err(internal_error)?;

    let base_name = Path::new(&safe_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&safe_name)
        .to_string();

    let validation = match file_type {
        "shapefile" => validate_shapefile_zip(&file_path).await,
        "geojson" => validate_geojson(&file_path).await,
        _ => Ok(()),
    };

    let uploaded_at = Utc::now().to_rfc3339();

    let _guard = with_index_lock(&state).await;
    let mut index = load_index(&state.index_path).await;

    if let Err(message) = validation {
        let failed = build_metadata(
            &upload_id,
            &base_name,
            file_type,
            size,
            &uploaded_at,
            "failed",
            None,
            &file_path,
            Some(message.clone()),
        );
        index.push(failed);
        save_index(&state.index_path, &index).await;
        drop(_guard);
        return Err(bad_request(&message));
    }

    let meta = build_metadata(
        &upload_id,
        &base_name,
        file_type,
        size,
        &uploaded_at,
        "uploaded",
        None,
        &file_path,
        None,
    );

    index.push(meta.clone());
    save_index(&state.index_path, &index).await;
    drop(_guard);

    Ok((StatusCode::CREATED, Json(meta)))
}

async fn ensure_upload_dir(upload_dir: &Path, index_path: &Path) {
    let _ = fs::create_dir_all(upload_dir).await;
    if fs::metadata(index_path).await.is_err() {
        let _ = fs::write(index_path, "[]").await;
    }
}

async fn normalize_index_on_start(index_path: &Path) {
    let mut items = load_index(index_path).await;
    let mut changed = false;
    for item in &mut items {
        if item.status == "uploading" {
            item.status = "failed".to_string();
            item.error.get_or_insert_with(|| {
                "Upload interrupted by server restart".to_string()
            });
            changed = true;
        }
    }
    if changed {
        save_index(index_path, &items).await;
    }
}

async fn load_index(index_path: &Path) -> Vec<FileItem> {
    match fs::read_to_string(index_path).await {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

async fn save_index(index_path: &Path, items: &[FileItem]) {
    if let Ok(json) = serde_json::to_string_pretty(items) {
        let _ = fs::write(index_path, json).await;
    }
}

async fn with_index_lock(state: &AppState) -> tokio::sync::MutexGuard<'_, ()> {
    state.index_lock.lock().await
}

fn build_metadata(
    id: &str,
    name: &str,
    file_type: &str,
    size: u64,
    uploaded_at: &str,
    status: &str,
    crs: Option<String>,
    file_path: &Path,
    error: Option<String>,
) -> FileItem {
    let relative = file_path
        .strip_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .unwrap_or(file_path)
        .to_path_buf();
    let mut rel_string = relative.to_string_lossy().replace('\\', "/");
    if !rel_string.starts_with('.') {
        rel_string = format!("./{rel_string}");
    }

    FileItem {
        id: id.to_string(),
        name: name.to_string(),
        file_type: file_type.to_string(),
        size,
        uploaded_at: uploaded_at.to_string(),
        status: status.to_string(),
        crs,
        path: rel_string,
        error,
    }
}

fn create_id() -> String {
    let mut bytes = [0u8; 3];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

async fn validate_shapefile_zip(file_path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(file_path).map_err(|_| "Unable to read zip file".to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "Unable to read zip file".to_string())?;

    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|_| "Unable to read zip file".to_string())?;
        if entry.is_file() {
            if let Some(name) = Path::new(entry.name()).file_name() {
                entries.push(name.to_string_lossy().to_lowercase());
            }
        }
    }

    if entries.iter().all(|name| !name.ends_with(".shp")) {
        return Err("Missing .shp file in zip".to_string());
    }

    let shp_bases: Vec<String> = entries
        .iter()
        .filter_map(|name| name.strip_suffix(".shp").map(|base| base.to_string()))
        .collect();

    for base in shp_bases {
        let has_shx = entries.iter().any(|name| name == &format!("{base}.shx"));
        let has_dbf = entries.iter().any(|name| name == &format!("{base}.dbf"));
        if has_shx && has_dbf {
            return Ok(());
        }
    }

    Err("Shapefile zip must include .shp/.shx/.dbf with the same name".to_string())
}

async fn validate_geojson(file_path: &Path) -> Result<(), String> {
    let data = fs::read_to_string(file_path)
        .await
        .map_err(|_| "Invalid GeoJSON".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&data).map_err(|_| "Invalid GeoJSON".to_string())?;
    if !value.is_object() {
        return Err("Invalid GeoJSON".to_string());
    }
    Ok(())
}

fn bad_request(message: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
}

fn payload_too_large(message: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
}

fn internal_error<E: std::fmt::Debug>(_error: E) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "Upload failed".to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request};
    use http_body_util::BodyExt;
    use std::io::{Cursor, Write};
    use tempfile::TempDir;
    use tower::util::ServiceExt;
    use zip::write::FileOptions;
    use zip::ZipWriter;

    async fn setup_state(max_size: u64) -> (AppState, TempDir) {
        let temp_dir = TempDir::new().expect("temp dir");
        let upload_dir = temp_dir.path().join("uploads");
        let index_path = upload_dir.join("index.json");

        ensure_upload_dir(&upload_dir, &index_path).await;

        let state = AppState {
            upload_dir,
            index_path,
            index_lock: Arc::new(Mutex::new(())),
            max_size,
        };

        (state, temp_dir)
    }

    fn multipart_body(
        field_name: &str,
        filename: &str,
        content_type: &str,
        data: &[u8],
    ) -> (String, Vec<u8>) {
        let boundary = "BOUNDARY123456";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{field_name}\"; filename=\"{filename}\"\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        (boundary.to_string(), body)
    }

    fn zip_bytes(entries: &[&str]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let options = FileOptions::default();
            for name in entries {
                zip.start_file(*name, options).expect("start file");
                zip.write_all(b"").expect("write file");
            }
            zip.finish().expect("finish zip");
        }
        cursor.into_inner()
    }

    async fn response_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
        let body = response.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&body).expect("json")
    }

    #[tokio::test]
    async fn list_returns_seeded_items() {
        let (state, _temp_dir) = setup_state(1024).await;
        let uploaded_at = "2026-02-04T10:00:00Z";
        let file_path = state.upload_dir.join("seed-1").join("existing.geojson");
        let item = build_metadata(
            "seed-1",
            "existing",
            "geojson",
            42,
            uploaded_at,
            "uploaded",
            None,
            &file_path,
            None,
        );
        save_index(&state.index_path, &[item]).await;

        let app = build_api_router(state);
        let response = app
            .oneshot(Request::builder().uri("/api/files").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let items: Vec<FileItem> = response_json(response).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "existing");
        assert_eq!(items[0].status, "uploaded");
    }

    #[tokio::test]
    async fn upload_geojson_success() {
        let (state, _temp_dir) = setup_state(1024).await;
        let app = build_api_router(state.clone());
        let payload = br#"{"type":"FeatureCollection","features":[]}"#;
        let (boundary, body) = multipart_body(
            "file",
            "sample.geojson",
            "application/geo+json",
            payload,
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/uploads")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let item: FileItem = response_json(response).await;
        assert_eq!(item.file_type, "geojson");
        assert_eq!(item.status, "uploaded");

        let index = load_index(&state.index_path).await;
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].status, "uploaded");
    }

    #[tokio::test]
    async fn upload_geojson_invalid() {
        let (state, _temp_dir) = setup_state(1024).await;
        let app = build_api_router(state.clone());
        let payload = br#"{"type": "#;
        let (boundary, body) = multipart_body("file", "bad.geojson", "application/json", payload);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/uploads")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: ErrorResponse = response_json(response).await;
        assert!(error.error.contains("Invalid GeoJSON"));

        let index = load_index(&state.index_path).await;
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].status, "failed");
    }

    #[tokio::test]
    async fn upload_shapefile_missing_parts() {
        let (state, _temp_dir) = setup_state(1024).await;
        let app = build_api_router(state.clone());
        let zip_data = zip_bytes(&["roads.shp", "roads.shx"]);
        let (boundary, body) = multipart_body("file", "roads.zip", "application/zip", &zip_data);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/uploads")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: ErrorResponse = response_json(response).await;
        assert!(error.error.contains(".shp/.shx/.dbf"));

        let index = load_index(&state.index_path).await;
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].status, "failed");
    }

    #[tokio::test]
    async fn upload_unsupported_extension() {
        let (state, _temp_dir) = setup_state(1024).await;
        let app = build_api_router(state);
        let payload = b"hello";
        let (boundary, body) = multipart_body("file", "note.txt", "text/plain", payload);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/uploads")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: ErrorResponse = response_json(response).await;
        assert!(error.error.contains("Unsupported file type"));
    }

    #[tokio::test]
    async fn upload_too_large() {
        let (state, _temp_dir) = setup_state(10).await;
        let app = build_api_router(state);
        let payload = b"0123456789abcdef";
        let (boundary, body) = multipart_body("file", "big.geojson", "application/json", payload);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/uploads")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let error: ErrorResponse = response_json(response).await;
        assert!(error.error.contains("File too large"));
    }

    #[tokio::test]
    async fn restart_marks_uploading_failed_and_keeps_uploaded() {
        let (state, _temp_dir) = setup_state(1024).await;
        let uploaded_at = "2026-02-04T10:00:00Z";
        let uploaded_path = state.upload_dir.join("ok-1").join("ok.geojson");
        let uploading_path = state.upload_dir.join("stuck-1").join("stuck.geojson");
        let uploaded = build_metadata(
            "ok-1",
            "ok",
            "geojson",
            1,
            uploaded_at,
            "uploaded",
            None,
            &uploaded_path,
            None,
        );
        let uploading = build_metadata(
            "stuck-1",
            "stuck",
            "geojson",
            1,
            uploaded_at,
            "uploading",
            None,
            &uploading_path,
            None,
        );
        save_index(&state.index_path, &[uploaded, uploading]).await;

        normalize_index_on_start(&state.index_path).await;

        let app = build_api_router(state);
        let response = app
            .oneshot(Request::builder().uri("/api/files").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let items: Vec<FileItem> = response_json(response).await;
        assert_eq!(items.len(), 2);
        let stuck = items.iter().find(|item| item.id == "stuck-1").unwrap();
        assert_eq!(stuck.status, "failed");
        assert!(stuck
            .error
            .as_ref()
            .map(|msg| msg.contains("Upload interrupted"))
            .unwrap_or(false));
    }
}
