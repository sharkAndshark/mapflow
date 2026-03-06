use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use backend::{build_test_router, init_database, AppState, AuthBackend, DuckDBStore};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use std::sync::Once;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::RwLock;
use tower::ServiceExt;

static TRACING_INIT: Once = Once::new();

#[derive(Debug, Clone)]
struct PostgisEnv {
    host: String,
    port: u16,
    database: String,
    username: String,
    password: String,
}

impl PostgisEnv {
    fn maybe_from_env() -> Option<Self> {
        let run_enabled = std::env::var("MAPFLOW_RUN_POSTGIS_TESTS")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(false);

        if !run_enabled {
            eprintln!("skipping postgis integration tests (MAPFLOW_RUN_POSTGIS_TESTS != 1)");
            return None;
        }

        let port = std::env::var("POSTGIS_TEST_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(55432);

        Some(Self {
            host: std::env::var("POSTGIS_TEST_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port,
            database: std::env::var("POSTGIS_TEST_DB").unwrap_or_else(|_| "mapflow".to_string()),
            username: std::env::var("POSTGIS_TEST_USER").unwrap_or_else(|_| "mapflow".to_string()),
            password: std::env::var("POSTGIS_TEST_PASSWORD")
                .unwrap_or_else(|_| "mapflow".to_string()),
        })
    }
}

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

fn init_tracing() {
    TRACING_INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error")),
            )
            .with_test_writer()
            .try_init();
    });
}

async fn send_json(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);

    let request = if let Some(payload) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request")
    } else {
        builder.body(Body::empty()).expect("request")
    };

    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let value = if bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice::<Value>(&bytes).unwrap_or_else(|_| {
            panic!(
                "non-json response body: {}",
                String::from_utf8_lossy(&bytes)
            )
        })
    };
    (status, value)
}

async fn send_json_retry_postgis_connectivity(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    const MAX_ATTEMPTS: usize = 8;
    const RETRY_DELAY_MS: u64 = 500;

    let mut attempt = 0;
    loop {
        let result = send_json(app, method.clone(), uri, body.clone()).await;
        let should_retry = result.0 == StatusCode::BAD_REQUEST
            && result.1["error"]
                .as_str()
                .map(|msg| msg.contains("error communicating with the server"))
                .unwrap_or(false)
            && attempt + 1 < MAX_ATTEMPTS;
        if !should_retry {
            return result;
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
    }
}

async fn send_bytes(app: &axum::Router, method: Method, uri: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("request");

    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes()
        .to_vec();
    (status, bytes)
}

fn postgis_connection_payload(cfg: &PostgisEnv) -> Value {
    json!({
        "host": cfg.host,
        "port": cfg.port,
        "database": cfg.database,
        "username": cfg.username,
        "password": cfg.password,
        "sslMode": "disable"
    })
}

fn register_source_payload(
    cfg: &PostgisEnv,
    connection_name: &str,
    object: &str,
    display_name: &str,
) -> Value {
    json!({
        "connectionName": connection_name,
        "connection": postgis_connection_payload(cfg),
        "schema": "public",
        "object": object,
        "geometryColumn": "geom",
        "fidColumn": "id",
        "displayName": display_name
    })
}

#[tokio::test]
async fn test_postgis_register_preview_publish_flow() {
    init_tracing();
    let Some(cfg) = PostgisEnv::maybe_from_env() else {
        return;
    };

    std::env::set_var("APP_SECRET", "postgis-integration-secret");

    let (app, _tmp) = setup_app().await;

    let test_payload = json!({
        "connection": postgis_connection_payload(&cfg)
    });

    let (status, body) = send_json_retry_postgis_connectivity(
        &app,
        Method::POST,
        "/api/postgis/connections/test",
        Some(test_payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    assert_eq!(body["success"], json!(true));

    let register_payload =
        register_source_payload(&cfg, "integration-local", "roads", "PostGIS Roads");

    let (status, body) = send_json_retry_postgis_connectivity(
        &app,
        Method::POST,
        "/api/postgis/sources/register",
        Some(register_payload),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{}", body);

    let file_id = body["fileId"].as_str().expect("fileId").to_string();

    let (status, files) = send_json(&app, Method::GET, "/api/files", None).await;
    assert_eq!(status, StatusCode::OK, "{}", files);
    let item = files
        .as_array()
        .and_then(|items| items.iter().find(|i| i["id"] == json!(file_id)))
        .expect("registered file in /api/files");
    assert_eq!(item["tileSource"], json!("postgis"));
    assert_eq!(item["status"], json!("ready"));

    let (status, preview) = send_json(
        &app,
        Method::GET,
        &format!("/api/files/{file_id}/preview"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", preview);
    assert!(
        preview["bbox"].is_array(),
        "preview bbox missing: {preview}"
    );

    let (status, schema) = send_json(
        &app,
        Method::GET,
        &format!("/api/files/{file_id}/schema"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", schema);
    let has_name_field = schema["layers"][0]["fields"]
        .as_array()
        .map(|fields| fields.iter().any(|f| f["name"] == json!("name")))
        .unwrap_or(false);
    assert!(has_name_field, "schema missing name field: {schema}");

    let (status, feature) = send_json(
        &app,
        Method::GET,
        &format!("/api/files/{file_id}/features/1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", feature);
    let has_main_street = feature["properties"]
        .as_array()
        .map(|props| {
            props
                .iter()
                .any(|p| p["key"] == json!("name") && p["value"] == json!("Main Street"))
        })
        .unwrap_or(false);
    assert!(
        has_main_street,
        "feature properties missing seeded value: {feature}"
    );

    let (status, private_tile) = send_bytes(
        &app,
        Method::GET,
        &format!("/api/files/{file_id}/tiles/0/0/0"),
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "private tile status {status}"
    );
    if status == StatusCode::OK {
        assert!(!private_tile.is_empty(), "private tile is empty");
    }

    let slug = format!("pg-roads-{}", &file_id[..8]);
    let (status, publish) = send_json(
        &app,
        Method::POST,
        &format!("/api/files/{file_id}/publish"),
        Some(json!({ "slug": slug })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", publish);

    let (status, public_tile) =
        send_bytes(&app, Method::GET, &format!("/tiles/{slug}/0/0/0")).await;
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "public tile status {status}"
    );
    if status == StatusCode::OK {
        assert!(!public_tile.is_empty(), "public tile is empty");
    }
}

#[tokio::test]
async fn test_postgis_view_registration_succeeds() {
    init_tracing();
    let Some(cfg) = PostgisEnv::maybe_from_env() else {
        return;
    };

    std::env::set_var("APP_SECRET", "postgis-integration-secret");

    let (app, _tmp) = setup_app().await;

    let register_payload =
        register_source_payload(&cfg, "integration-view", "roads_view", "PostGIS Roads View");

    let (status, body) = send_json_retry_postgis_connectivity(
        &app,
        Method::POST,
        "/api/postgis/sources/register",
        Some(register_payload),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{}", body);

    let file_id = body["fileId"].as_str().expect("fileId").to_string();

    let (status, files) = send_json(&app, Method::GET, "/api/files", None).await;
    assert_eq!(status, StatusCode::OK, "{}", files);
    let item = files
        .as_array()
        .and_then(|items| items.iter().find(|i| i["id"] == json!(file_id)))
        .expect("registered view file in /api/files");
    assert_eq!(item["tileSource"], json!("postgis"));
    assert_eq!(item["status"], json!("ready"));

    let (status, feature) = send_json(
        &app,
        Method::GET,
        &format!("/api/files/{file_id}/features/1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", feature);
}

#[tokio::test]
async fn test_postgis_rejects_composite_unique_fid_index() {
    init_tracing();
    let Some(cfg) = PostgisEnv::maybe_from_env() else {
        return;
    };

    std::env::set_var("APP_SECRET", "postgis-integration-secret");

    let (app, _tmp) = setup_app().await;

    let register_payload = register_source_payload(
        &cfg,
        "integration-composite",
        "roads_composite",
        "PostGIS Composite FID",
    );

    let (status, body) = send_json_retry_postgis_connectivity(
        &app,
        Method::POST,
        "/api/postgis/sources/register",
        Some(register_payload),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{}", body);

    let error = body["error"].as_str().unwrap_or("");
    assert!(
        error.contains("single-column UNIQUE/PRIMARY KEY index"),
        "unexpected error body: {body}"
    );
}

#[tokio::test]
async fn test_postgis_rejects_include_only_fid_index() {
    init_tracing();
    let Some(cfg) = PostgisEnv::maybe_from_env() else {
        return;
    };

    std::env::set_var("APP_SECRET", "postgis-integration-secret");

    let (app, _tmp) = setup_app().await;

    let register_payload = register_source_payload(
        &cfg,
        "integration-include",
        "roads_include",
        "PostGIS Include FID",
    );

    let (status, body) = send_json_retry_postgis_connectivity(
        &app,
        Method::POST,
        "/api/postgis/sources/register",
        Some(register_payload),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{}", body);

    let error = body["error"].as_str().unwrap_or("");
    assert!(
        error.contains("single-column UNIQUE/PRIMARY KEY index"),
        "unexpected error body: {body}"
    );
}

#[tokio::test]
async fn test_postgis_quoted_property_identifiers_and_aliases_work() {
    init_tracing();
    let Some(cfg) = PostgisEnv::maybe_from_env() else {
        return;
    };

    std::env::set_var("APP_SECRET", "postgis-integration-secret");

    let (app, _tmp) = setup_app().await;

    let register_payload = register_source_payload(
        &cfg,
        "integration-quoted",
        "roads_quoted",
        "PostGIS Quoted Columns",
    );
    let (status, body) = send_json_retry_postgis_connectivity(
        &app,
        Method::POST,
        "/api/postgis/sources/register",
        Some(register_payload),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{}", body);
    let file_id = body["fileId"].as_str().expect("fileId").to_string();

    let (status, schema) = send_json(
        &app,
        Method::GET,
        &format!("/api/files/{file_id}/schema"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", schema);
    let normalized_name = schema["layers"][0]["fields"]
        .as_array()
        .and_then(|fields| {
            fields
                .iter()
                .find(|field| field["name"] == json!("road name"))
                .and_then(|field| field["normalized"].as_str())
        })
        .expect("normalized name for quoted column")
        .to_string();

    let (status, updated) = send_json(
        &app,
        Method::PATCH,
        &format!("/api/files/{file_id}/field-aliases"),
        Some(json!({
            "fields": [
                {
                    "normalized_name": normalized_name,
                    "alias": "Road Name-1"
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", updated);

    let (status, feature) = send_json(
        &app,
        Method::GET,
        &format!("/api/files/{file_id}/features/1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", feature);
    let has_quoted_property = feature["properties"]
        .as_array()
        .map(|props| props.iter().any(|p| p["key"] == json!("road name")))
        .unwrap_or(false);
    assert!(
        has_quoted_property,
        "feature should contain quoted property key: {feature}"
    );

    let (status, tile) = send_bytes(
        &app,
        Method::GET,
        &format!("/api/files/{file_id}/tiles/0/0/0"),
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "tile status {status}"
    );
    if status == StatusCode::OK {
        assert!(!tile.is_empty(), "tile should not be empty");
    }
}
