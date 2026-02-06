use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State},
    http::{header, Method, StatusCode},
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
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
};
use zip::ZipArchive;

const DEFAULT_MAX_SIZE_MB: u64 = 200;
const BYTES_PER_MB: u64 = 1024 * 1024;
const DEFAULT_DB_PATH: &str = "./data/mapflow.duckdb";

fn init_database(db_path: &Path) -> duckdb::Connection {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create database directory");
    }

    let conn = duckdb::Connection::open(db_path).expect("Failed to open database");

    conn.execute_batch("INSTALL spatial; LOAD spatial;")
        .expect("Failed to install and load spatial extension");

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS files (
            id VARCHAR PRIMARY KEY,
            name VARCHAR NOT NULL,
            type VARCHAR NOT NULL,
            size BIGINT NOT NULL,
            uploaded_at TIMESTAMP NOT NULL,
            status VARCHAR NOT NULL,
            crs VARCHAR,
            path VARCHAR NOT NULL,
            error VARCHAR
        );
        ",
    )
    .expect("Failed to create files table");

    conn.execute_batch(
        r"
        CREATE SEQUENCE IF NOT EXISTS spatial_data_seq;
        
        CREATE TABLE IF NOT EXISTS spatial_data (
            id INTEGER PRIMARY KEY DEFAULT nextval('spatial_data_seq'),
            source_id VARCHAR NOT NULL,
            geom GEOMETRY,
            properties JSON,
            FOREIGN KEY (source_id) REFERENCES files(id)
        );
        
        CREATE INDEX IF NOT EXISTS idx_spatial_data_source 
            ON spatial_data(source_id);
        ",
    )
    .expect("Failed to create spatial_data table");

    conn
}

#[derive(Clone)]
struct AppState {
    upload_dir: PathBuf,
    db: Arc<Mutex<duckdb::Connection>>,
    max_size: u64,
    max_size_label: String,
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

#[derive(Serialize)]
struct PreviewMeta {
    id: String,
    name: String,
    crs: Option<String>,
    bbox: Option<[f64; 4]>, // minx, miny, maxx, maxy in WGS84
}

#[tokio::main]
async fn main() {
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_string());
    let db_path = PathBuf::from(db_path);
    let conn = init_database(&db_path);

    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string());
    let upload_dir = PathBuf::from(upload_dir);
    let _ = fs::create_dir_all(&upload_dir).await;

    let (max_size, max_size_label) = read_max_size_config();

    let state = AppState {
        upload_dir,
        db: Arc::new(Mutex::new(conn)),
        max_size,
        max_size_label,
    };

    let mut app = build_api_router(state.clone());

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

fn build_api_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    Router::new()
        .route("/api/files", get(list_files))
        .route("/api/uploads", post(upload_file))
        .route("/api/files/:id/preview", get(get_preview_meta))
        .route("/api/files/:id/tiles/:z/:x/:y", get(get_tile))
        .layer(DefaultBodyLimit::disable()) // Disable default 2MB limit
        .with_state(state)
        .layer(cors)
}

async fn list_files(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, type, size, uploaded_at, status, crs, path, error 
         FROM files ORDER BY uploaded_at DESC",
        )
        .unwrap();

    let items: Vec<FileItem> = stmt
        .query_map([], |row| {
            let error: Option<String> = row.get(8)?;
            Ok(FileItem {
                id: row.get(0)?,
                name: row.get(1)?,
                file_type: row.get(2)?,
                size: row.get(3)?,
                uploaded_at: {
                    let ts: chrono::NaiveDateTime = row.get(4)?;
                    ts.and_utc().to_rfc3339()
                },
                status: row.get(5)?,
                crs: row.get(6)?,
                path: row.get(7)?,
                error,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    drop(conn);
    Json(items)
}

async fn get_preview_meta(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    // Check if file exists and get meta
    let mut stmt = conn
        .prepare("SELECT name, crs, path FROM files WHERE id = ?")
        .map_err(internal_error)?;
    
    let meta: Option<(String, Option<String>, String)> = stmt
        .query_row(duckdb::params![id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .ok();

    let (name, crs, _path) = match meta {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, Json(ErrorResponse { error: "File not found".to_string() }))),
    };

    // Calculate BBOX in WGS84
    // Note: If CRS is missing/null, we assume EPSG:4326 and always_xy=true (lon/lat) for simplicity
    
    // Query for bbox components directly
    let bbox_components_query = format!(
        "SELECT ST_XMin(b), ST_YMin(b), ST_XMax(b), ST_YMax(b) FROM (
            SELECT ST_Extent(ST_Transform(geom, '{}', 'EPSG:4326', always_xy := true)) as b
            FROM spatial_data WHERE source_id = ?
        )",
        crs.as_deref().unwrap_or("EPSG:4326")
    );

    let bbox_values: Option<[f64; 4]> = conn
        .query_row(&bbox_components_query, duckdb::params![id], |row| {
             let minx: Option<f64> = row.get(0).ok();
             let miny: Option<f64> = row.get(1).ok();
             let maxx: Option<f64> = row.get(2).ok();
             let maxy: Option<f64> = row.get(3).ok();
             
             if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (minx, miny, maxx, maxy) {
                 Ok([x1, y1, x2, y2])
             } else {
                 Ok([0.0, 0.0, 0.0, 0.0]) // Handle empty result
             }
        })
        .ok()
        .filter(|b| b != &[0.0, 0.0, 0.0, 0.0]);

    Ok(Json(PreviewMeta {
        id,
        name,
        crs,
        bbox: bbox_values,
    }))
}

async fn get_tile(
    State(state): State<AppState>,
    AxumPath((id, z, x, y)): AxumPath<(String, i32, i32, i32)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    println!("Received tile request: id={}, z={}, x={}, y={}", id, z, x, y);
    let conn = state.db.lock().await;

    // 1. Get CRS for the file
    let crs: Option<String> = conn
        .query_row("SELECT crs FROM files WHERE id = ?", duckdb::params![id], |row| row.get(0))
        .map_err(|_| (StatusCode::NOT_FOUND, Json(ErrorResponse { error: "File not found".to_string() })))?;

    let source_crs = crs.as_deref().unwrap_or("EPSG:4326");

    // 2. Generate MVT
    // logic:
    //  - filter by source_id
    //  - ST_Transform(geom, source_crs, 'EPSG:3857', always_xy := true)
    //  - ST_TileEnvelope(z, x, y) to get tile bounds in 3857
    //  - ST_AsMVTGeom(geom_3857, tile_env) to clip/transform to tile coords
    //  - ST_AsMVT(...) to encode
    
    let select_sql = format!(
        "SELECT ST_AsMVT(feature) FROM (
            SELECT {{
                'geom': ST_AsMVTGeom(
                    ST_Transform(geom, '{source_crs}', 'EPSG:3857', always_xy := true),
                    ST_Extent(ST_TileEnvelope(?, ?, ?)),
                    4096, 256, true
                )
            }} as feature
            FROM spatial_data
            WHERE source_id = ? 
              AND ST_Intersects(
                  ST_Transform(geom, '{source_crs}', 'EPSG:3857', always_xy := true),
                  ST_TileEnvelope(?, ?, ?)
              )
        )"
    );

    println!("Executing SQL for tile z={z} x={x} y={y} id={id}");

    // Params: z, x, y, source_id, z, x, y
    let mvt_blob: Option<Vec<u8>> = match conn.query_row(
        &select_sql,
        duckdb::params![z, x, y, id, z, x, y],
        |row| row.get(0)
    ) {
        Ok(blob) => Some(blob),
        Err(e) => {
            eprintln!("Tile Error (z={z}, x={x}, y={y}): {:?}", e);
            eprintln!("SQL that failed: {}", select_sql);
            return Err(internal_error(format!("Tile generation failed: {}", e)));
        }
    };

    println!("Tile Request: z={z}, x={x}, y={y}, Blob Size: {:?}", mvt_blob.as_ref().map(|v| v.len()));

    match mvt_blob {
        Some(blob) if !blob.is_empty() => {
             Ok((
                [(header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile")],
                blob
            ).into_response())
        },
        _ => {
            // Return empty response or 204? Mapbox clients usually expect 200 with empty body or valid PBF.
            // An empty blob is a valid MVT (empty).
             Ok((
                [(header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile")],
                Vec::new()
            ).into_response())
        }
    }
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
    let mut file = BufWriter::new(fs::File::create(&file_path).await.map_err(internal_error)?);

    let mut size: u64 = 0;
    while let Some(chunk) = field.chunk().await.map_err(internal_error)? {
        size = size.saturating_add(chunk.len() as u64);
        if size > state.max_size {
            drop(file);
            let _ = fs::remove_file(&file_path).await;
            let message = format!("File too large (max {})", state.max_size_label);
            return Err(payload_too_large(&message));
        }
        file.write_all(&chunk).await.map_err(internal_error)?;
    }
    file.flush().await.map_err(internal_error)?;
    drop(file); // Explicitly close file to release lock


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

    // Calculate relative path for storage
    let relative = file_path
        .strip_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .unwrap_or(&file_path)
        .to_path_buf();
    let mut rel_string = relative.to_string_lossy().replace('\\', "/");
    if !rel_string.starts_with('.') {
        rel_string = format!("./{rel_string}");
    }

    let conn = state.db.lock().await;

    if let Err(message) = validation {
        let size_i64 = size as i64;
        conn.execute(
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            duckdb::params![
                &upload_id,
                &base_name,
                file_type,
                size_i64,
                &uploaded_at,
                "failed",
                &None::<String>,
                &rel_string,
                &Some(message.clone()),
            ],
        )
        .map_err(internal_error)?;

        drop(conn);
        return Err(bad_request(&message));
    }

    let size_i64 = size as i64;
    conn.execute(
        "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        duckdb::params![
            &upload_id,
            &base_name,
            file_type,
            size_i64,
            &uploaded_at,
            "uploaded", // Initial status: uploaded (processing happens in background)
            &None::<String>,
            &rel_string,
            &None::<String>,
        ],
    )
    .map_err(internal_error)?;

    drop(conn);

    let db = state.db.clone();
    let upload_id_clone = upload_id.clone();
    let file_path_clone = file_path.clone();
    let file_type_str = file_type.to_string();
    tokio::spawn(async move {
        // Set status to processing
        {
            let conn = db.lock().await;
            let _ = conn.execute(
                "UPDATE files SET status = 'processing' WHERE id = ?",
                duckdb::params![upload_id_clone],
            );
        }

        match import_spatial_data(&db, &upload_id_clone, &file_path_clone, &file_type_str).await {
            Ok(_) => {
                println!("Successfully imported spatial data for {}", upload_id_clone);
                let conn = db.lock().await;
                let _ = conn.execute(
                    "UPDATE files SET status = 'ready' WHERE id = ?",
                    duckdb::params![upload_id_clone],
                );
            }
            Err(e) => {
                eprintln!("Failed to import spatial data for {}: {}", upload_id_clone, e);
                // Update status to failed
                let conn = db.lock().await;
                let _ = conn.execute(
                    "UPDATE files SET status = 'failed', error = ? WHERE id = ?",
                    duckdb::params![e, upload_id_clone],
                );
            }
        }
    });

    let meta = FileItem {
        id: upload_id,
        name: base_name,
        file_type: file_type.to_string(),
        size,
        uploaded_at,
        status: "uploaded".to_string(), // Keep consistent with DB initial state
        crs: None,
        path: rel_string,
        error: None,
    };

    Ok((StatusCode::CREATED, Json(meta)))
}

async fn import_spatial_data(
    db: &Arc<Mutex<duckdb::Connection>>,
    source_id: &str,
    file_path: &Path,
    _file_type: &str,
) -> Result<(), String> {
    let abs_path = std::fs::canonicalize(file_path)
        .map_err(|e| format!("Cannot resolve file path {:?}: {}", file_path, e))?
        .to_string_lossy()
        .to_string();

    let abs_path = if file_path.extension().and_then(|e| e.to_str()) == Some("zip") {
        // Use /vsizip/ prefix for GDAL to read directly from zip
        format!("/vsizip/{}", abs_path)
    } else {
        abs_path
    };

    let conn = db.lock().await;

    // 1. Detect CRS using ST_Read_Meta
    // layers[1].geometry_fields[1].crs.auth_name / auth_code
    // Note: ST_Read_Meta return structure depends on the file. 
    // We try to get the first layer's CRS.
    // List indexing in DuckDB is 1-based.
    let crs_query = format!(
        "SELECT 
            layers[1].geometry_fields[1].crs.auth_name || ':' || layers[1].geometry_fields[1].crs.auth_code 
         FROM ST_Read_Meta('{abs_path}')"
    );

    let detected_crs: Option<String> = conn.query_row(&crs_query, [], |row| row.get(0)).ok();
    
    // Update files table with detected CRS
    if let Some(crs) = &detected_crs {
        let _ = conn.execute("UPDATE files SET crs = ? WHERE id = ?", duckdb::params![crs, source_id]);
    }

    // 2. Import Data
    let sql = format!(
        "INSERT INTO spatial_data (source_id, geom, properties) 
         SELECT '{source_id}', geom, NULL 
         FROM ST_Read('{abs_path}')"
    );

    conn.execute(&sql, [])
        .map_err(|e| format!("Spatial import failed: {}", e))?;

    Ok(())
}

fn read_max_size_config() -> (u64, String) {
    let max_size_mb = std::env::var("UPLOAD_MAX_SIZE_MB")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SIZE_MB);
    let bytes = max_size_mb.saturating_mul(BYTES_PER_MB);
    (bytes, format_bytes(bytes))
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes >= GB && bytes % GB == 0 {
        format!("{}GB", bytes / GB)
    } else if bytes >= MB && bytes % MB == 0 {
        format!("{}MB", bytes / MB)
    } else if bytes >= KB && bytes % KB == 0 {
        format!("{}KB", bytes / KB)
    } else {
        format!("{}B", bytes)
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

fn internal_error<E: std::fmt::Debug>(error: E) -> (StatusCode, Json<ErrorResponse>) {
    eprintln!("Internal Error: {:?}", error);
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
    use std::sync::OnceLock;
    use tempfile::TempDir;
    use tower::util::ServiceExt;
    use zip::write::FileOptions;
    use zip::ZipWriter;

    static ENV_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();

    async fn setup_state(max_size: u64) -> (AppState, TempDir) {
        let temp_dir = TempDir::new().expect("temp dir");
        let upload_dir = temp_dir.path().join("uploads");
        fs::create_dir_all(&upload_dir).await.ok();

        let conn = duckdb::Connection::open_in_memory().expect("Failed to create test database");
        conn.execute_batch("INSTALL spatial; LOAD spatial;").unwrap();

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
            path VARCHAR NOT NULL,
            error VARCHAR
        );

        CREATE TABLE spatial_data (
            id INTEGER PRIMARY KEY,
            source_id VARCHAR NOT NULL,
            geom GEOMETRY,
            properties JSON,
            FOREIGN KEY (source_id) REFERENCES files(id)
        );
        ",
        )
        .unwrap();

        let state = AppState {
            upload_dir,
            db: Arc::new(Mutex::new(conn)),
            max_size,
            max_size_label: format_bytes(max_size),
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
            path: file_path.to_string_lossy().to_string(),
            error: None,
        };

        let conn = state.db.lock().await;
        let size = item.size as i64;
        conn.execute(
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            duckdb::params![
                &item.id,
                &item.name,
                &item.file_type,
                size,
                &item.uploaded_at,
                &item.status,
                &item.crs,
                &item.path,
                &item.error,
            ],
        )
        .unwrap();
        drop(conn);

        let app = build_api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                .uri("/api/files")
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let items: Vec<FileItem> = response_json(response).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "existing");
        assert_eq!(items[0].status, "uploaded");
    }

    #[test]
    fn read_max_size_config_default_and_custom() {
        let _guard = ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env lock");

        std::env::remove_var("UPLOAD_MAX_SIZE_MB");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, DEFAULT_MAX_SIZE_MB * BYTES_PER_MB);
        assert_eq!(label, "200MB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "12");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, 12 * BYTES_PER_MB);
        assert_eq!(label, "12MB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "0");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, DEFAULT_MAX_SIZE_MB * BYTES_PER_MB);
        assert_eq!(label, "200MB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "nope");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, DEFAULT_MAX_SIZE_MB * BYTES_PER_MB);
        assert_eq!(label, "200MB");
        std::env::remove_var("UPLOAD_MAX_SIZE_MB");
    }
}
