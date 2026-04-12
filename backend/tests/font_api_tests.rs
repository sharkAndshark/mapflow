#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::Request;
use backend::{build_test_router, init_database, AppState, AuthBackend, DuckDBStore};
use http_body_util::BodyExt;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;
use tower::ServiceExt;

static TEST_MODE_SET: std::sync::Once = std::sync::Once::new();

fn ensure_test_mode() {
    TEST_MODE_SET.call_once(|| {
        std::env::set_var("MAPFLOW_TEST_MODE", "1");
    });
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

async fn setup_app() -> (
    axum::Router,
    TempDir,
    Arc<tokio::sync::Mutex<duckdb::Connection>>,
) {
    ensure_test_mode();
    let temp_dir = TempDir::new().expect("temp dir");
    let upload_dir = temp_dir.path().join("uploads");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let db_path = temp_dir.path().join("font-api-tests.duckdb");
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

    let router = build_test_router(state);
    (router, temp_dir, db)
}

async fn upload_fixture_font(app: &axum::Router) -> String {
    let font_bytes = read_fixture_bytes("backend/tests/fixtures/fonts/PressStart2P-Regular.ttf");
    let boundary = "------------------------boundaryFONT";
    let body = multipart_body(boundary, "PressStart2P-Regular.ttf", &font_bytes);

    let upload_request = Request::builder()
        .method("POST")
        .uri("/api/fonts")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let upload_response = app.clone().oneshot(upload_request).await.unwrap();
    let upload_status = upload_response.status();
    if upload_status != axum::http::StatusCode::CREATED {
        let upload_body = upload_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        panic!(
            "expected upload 201 but got {} with body {}",
            upload_status,
            String::from_utf8_lossy(&upload_body)
        );
    }

    let upload_body = upload_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let uploaded: Value = serde_json::from_slice(&upload_body).unwrap();
    uploaded
        .get("id")
        .and_then(Value::as_str)
        .expect("font id")
        .to_string()
}

async fn wait_until_font_ready(app: &axum::Router, font_id: &str) -> Value {
    let mut last_status: Option<String> = None;
    let mut last_error: Option<String> = None;

    for _ in 0..120 {
        let request = Request::builder()
            .method("GET")
            .uri("/api/fonts")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let fonts: Vec<Value> = serde_json::from_slice(&body_bytes).unwrap();

        if let Some(item) = fonts
            .into_iter()
            .find(|f| f.get("id").and_then(Value::as_str) == Some(font_id))
        {
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            last_status = Some(status.clone());
            last_error = item
                .get("error")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            if status == "ready" {
                return item;
            }
            if status == "failed" {
                panic!("Font processing failed: {:?}", last_error);
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    panic!(
        "Timeout waiting for font ready (last_status={:?}, last_error={:?})",
        last_status, last_error
    );
}

#[tokio::test]
async fn test_font_upload_publish_and_public_glyph_lifecycle() {
    let (app, _temp_dir, _db) = setup_app().await;

    let font_id = upload_fixture_font(&app).await;

    let ready_font = wait_until_font_ready(&app, &font_id).await;
    let glyph_count = ready_font
        .get("glyphCount")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    assert!(glyph_count > 0);

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/fonts/{}/publish", font_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"pressstart2p-test"}"#))
        .unwrap();
    let publish_response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(publish_response.status(), axum::http::StatusCode::OK);
    let publish_body = publish_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let publish_payload: Value = serde_json::from_slice(&publish_body).unwrap();
    assert_eq!(publish_payload["workspaceSlug"], "test-workspace");
    assert_eq!(
        publish_payload["url"],
        "/fonts/test-workspace/{fontstack}/{range}.pbf"
    );

    let public_glyph_request = Request::builder()
        .method("GET")
        .uri("/fonts/test-workspace/Press%20Start%202P%20Regular/0-255.pbf")
        .body(Body::empty())
        .unwrap();
    let public_glyph_response = app.clone().oneshot(public_glyph_request).await.unwrap();
    assert_eq!(public_glyph_response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        public_glyph_response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/x-protobuf")
    );

    let fallback_fontstack_request = Request::builder()
        .method("GET")
        .uri("/fonts/test-workspace/Press%20Start%202P%20Regular,Missing%20Fallback/0-255.pbf")
        .body(Body::empty())
        .unwrap();
    let fallback_fontstack_response = app
        .clone()
        .oneshot(fallback_fontstack_request)
        .await
        .unwrap();
    assert_eq!(
        fallback_fontstack_response.status(),
        axum::http::StatusCode::OK
    );
    assert_eq!(
        fallback_fontstack_response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/x-protobuf")
    );

    let invalid_range_request = Request::builder()
        .method("GET")
        .uri("/fonts/test-workspace/Press%20Start%202P%20Regular/0-999.pbf")
        .body(Body::empty())
        .unwrap();
    let invalid_range_response = app.clone().oneshot(invalid_range_request).await.unwrap();
    assert_eq!(
        invalid_range_response.status(),
        axum::http::StatusCode::BAD_REQUEST
    );

    let unpublish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/fonts/{}/unpublish", font_id))
        .body(Body::empty())
        .unwrap();
    let unpublish_response = app.clone().oneshot(unpublish_request).await.unwrap();
    assert_eq!(
        unpublish_response.status(),
        axum::http::StatusCode::NO_CONTENT
    );

    let public_after_unpublish_request = Request::builder()
        .method("GET")
        .uri("/fonts/test-workspace/Press%20Start%202P%20Regular/0-255.pbf")
        .body(Body::empty())
        .unwrap();
    let public_after_unpublish_response = app
        .clone()
        .oneshot(public_after_unpublish_request)
        .await
        .unwrap();
    assert_eq!(
        public_after_unpublish_response.status(),
        axum::http::StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn test_publish_font_rejects_invalid_slug() {
    let (app, _temp_dir, _db) = setup_app().await;
    let font_id = upload_fixture_font(&app).await;
    let _ = wait_until_font_ready(&app, &font_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/fonts/{}/publish", font_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"bad slug!*"}"#))
        .unwrap();
    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_publish_font_rejects_slug_conflict() {
    let (app, _temp_dir, _db) = setup_app().await;
    let font_id_1 = upload_fixture_font(&app).await;
    let font_id_2 = upload_fixture_font(&app).await;
    let _ = wait_until_font_ready(&app, &font_id_1).await;
    let _ = wait_until_font_ready(&app, &font_id_2).await;

    let publish_1 = Request::builder()
        .method("POST")
        .uri(format!("/api/fonts/{}/publish", font_id_1))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"duplicate-font-slug"}"#))
        .unwrap();
    let response_1 = app.clone().oneshot(publish_1).await.unwrap();
    assert_eq!(response_1.status(), axum::http::StatusCode::OK);

    let publish_2 = Request::builder()
        .method("POST")
        .uri(format!("/api/fonts/{}/publish", font_id_2))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"duplicate-font-slug"}"#))
        .unwrap();
    let response_2 = app.clone().oneshot(publish_2).await.unwrap();
    assert_eq!(response_2.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_publish_font_rejects_non_ready_status() {
    let (app, _temp_dir, db) = setup_app().await;
    let font_id = "font-not-ready-1";

    let seed_request = Request::builder()
        .method("GET")
        .uri("/api/fonts")
        .body(Body::empty())
        .unwrap();
    let seed_response = app.clone().oneshot(seed_request).await.unwrap();
    assert_eq!(seed_response.status(), axum::http::StatusCode::OK);

    let workspace_id = {
        let conn = db.lock().await;
        conn.query_row(
            "SELECT id FROM workspaces WHERE is_personal = TRUE AND deleted_at IS NULL LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };

    {
        let conn = db.lock().await;
        conn.execute(
            "INSERT INTO fonts (id, workspace_id, name, fontstack, original_path, glyphs_path, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, 'processing', CURRENT_TIMESTAMP)",
            duckdb::params![
                font_id,
                &workspace_id,
                "Not Ready Font",
                "Not Ready Font",
                "./uploads/fonts/font-not-ready-1/original",
                "./uploads/fonts/font-not-ready-1/glyphs",
            ],
        )
        .unwrap();
    }

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/fonts/{}/publish", font_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"not-ready-font"}"#))
        .unwrap();
    let response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_delete_font_removes_public_access() {
    let (app, temp_dir, _db) = setup_app().await;
    let font_id = upload_fixture_font(&app).await;
    let _ = wait_until_font_ready(&app, &font_id).await;

    let publish_request = Request::builder()
        .method("POST")
        .uri(format!("/api/fonts/{}/publish", font_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"slug":"deletable-font"}"#))
        .unwrap();
    let publish_response = app.clone().oneshot(publish_request).await.unwrap();
    assert_eq!(publish_response.status(), axum::http::StatusCode::OK);

    let delete_request = Request::builder()
        .method("DELETE")
        .uri(format!("/api/fonts/{}", font_id))
        .body(Body::empty())
        .unwrap();
    let delete_response = app.clone().oneshot(delete_request).await.unwrap();
    assert_eq!(delete_response.status(), axum::http::StatusCode::NO_CONTENT);

    let public_glyph_request = Request::builder()
        .method("GET")
        .uri("/fonts/test-workspace/Press%20Start%202P%20Regular/0-255.pbf")
        .body(Body::empty())
        .unwrap();
    let public_glyph_response = app.clone().oneshot(public_glyph_request).await.unwrap();
    assert_eq!(
        public_glyph_response.status(),
        axum::http::StatusCode::NOT_FOUND
    );

    let font_dir = temp_dir.path().join("uploads").join("fonts").join(font_id);
    assert!(!font_dir.exists());
}

#[tokio::test]
async fn test_list_and_get_font_use_camel_case_contract() {
    let (app, _temp_dir, _db) = setup_app().await;
    let font_id = upload_fixture_font(&app).await;
    let _ = wait_until_font_ready(&app, &font_id).await;

    let list_request = Request::builder()
        .method("GET")
        .uri("/api/fonts")
        .body(Body::empty())
        .unwrap();
    let list_response = app.clone().oneshot(list_request).await.unwrap();
    assert_eq!(list_response.status(), axum::http::StatusCode::OK);
    let list_body = list_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let items: Vec<Value> = serde_json::from_slice(&list_body).unwrap();
    let item = items
        .into_iter()
        .find(|it| it.get("id").and_then(Value::as_str) == Some(font_id.as_str()))
        .expect("uploaded font in list");

    assert!(item.get("glyphCount").is_some());
    assert!(item.get("startCp").is_some());
    assert!(item.get("endCp").is_some());
    assert!(item.get("isPublic").is_some());
    assert!(item.get("workspaceSlug").is_some());
    assert!(item.get("createdAt").is_some());
    assert!(item.get("glyph_count").is_none());
    assert!(item.get("is_public").is_none());

    let get_request = Request::builder()
        .method("GET")
        .uri(format!("/api/fonts/{}", font_id))
        .body(Body::empty())
        .unwrap();
    let get_response = app.clone().oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), axum::http::StatusCode::OK);
    let get_body = get_response.into_body().collect().await.unwrap().to_bytes();
    let got: Value = serde_json::from_slice(&get_body).unwrap();
    assert!(got.get("glyphCount").is_some());
    assert!(got.get("isPublic").is_some());
    assert!(got.get("workspaceSlug").is_some());
    assert!(got.get("glyph_count").is_none());
    assert!(got.get("is_public").is_none());
}

#[tokio::test]
async fn test_upload_font_rejects_unsupported_extension() {
    let (app, _temp_dir, _db) = setup_app().await;
    let body = multipart_body("----boundaryTXT", "not-a-font.txt", b"hello");

    let request = Request::builder()
        .method("POST")
        .uri("/api/fonts")
        .header(
            "content-type",
            "multipart/form-data; boundary=----boundaryTXT",
        )
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}
