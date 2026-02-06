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
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
    sync::Mutex,
};
use tower_http::cors::{Any, CorsLayer};
use zip::ZipArchive;

mod config;
mod db;

pub use config::{format_bytes, read_max_size_config};
pub use db::{
    init_database, reconcile_processing_files, DEFAULT_DB_PATH, PROCESSING_RECONCILIATION_ERROR,
};

#[derive(Clone)]
pub struct AppState {
    pub upload_dir: PathBuf,
    pub db: Arc<Mutex<duckdb::Connection>>,
    pub max_size: u64,
    pub max_size_label: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub size: u64,
    #[serde(rename = "uploadedAt")]
    pub uploaded_at: String,
    pub status: String,
    pub crs: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize)]
pub struct PreviewMeta {
    pub id: String,
    pub name: String,
    pub crs: Option<String>,
    pub bbox: Option<[f64; 4]>, // minx, miny, maxx, maxy in WGS84
}

pub fn build_api_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers(Any);

    let router: Router<AppState> = Router::new()
        .route("/api/files", get(list_files))
        .route("/api/uploads", post(upload_file))
        .route("/api/files/:id/preview", get(get_preview_meta))
        .route("/api/files/:id/tiles/:z/:x/:y", get(get_tile));

    let router = add_test_routes(router);

    router
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
        .layer(cors)
}

#[cfg(debug_assertions)]
fn add_test_routes(router: Router<AppState>) -> Router<AppState> {
    if std::env::var("MAPFLOW_TEST_MODE").as_deref() == Ok("1") {
        println!("Test mode enabled (debug only): exposing POST /api/test/reset");
        router.route("/api/test/reset", post(reset_test_state))
    } else {
        router
    }
}

#[cfg(not(debug_assertions))]
fn add_test_routes(router: Router<AppState>) -> Router<AppState> {
    router
}

#[cfg(debug_assertions)]
async fn reset_test_state(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;

    // Drop per-dataset tables.
    // We use files.table_name as the source of truth.
    if let Ok(mut stmt) = conn.prepare("SELECT table_name FROM files WHERE table_name IS NOT NULL")
    {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, Option<String>>(0)) {
            for table in rows.flatten().flatten() {
                // table is normalized/safe, but quote anyway.
                let _ = conn.execute(&format!("DROP TABLE IF EXISTS \"{table}\""), []);
            }
        }
    }

    if let Err(e) = conn.execute_batch("DELETE FROM dataset_columns;\nDELETE FROM files;") {
        eprintln!("Test Reset DB Error: {:?}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "DB cleanup failed" })),
        );
    }

    match fs::read_dir(&state.upload_dir).await {
        Ok(mut entries) => {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    if let Err(e) = fs::remove_dir_all(path).await {
                        eprintln!("Test Reset FS Error: {:?}", e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": "Upload dir cleanup failed" })),
                        );
                    }
                } else if let Err(e) = fs::remove_file(path).await {
                    eprintln!("Test Reset FS Error: {:?}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": "Upload dir cleanup failed" })),
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Test Reset FS Error: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Upload dir read failed" })),
            );
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "reset_complete" })),
    )
}

async fn list_files(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, type, size, uploaded_at, status, crs, path, table_name, error 
         FROM files ORDER BY uploaded_at DESC",
        )
        .unwrap();

    let items: Vec<FileItem> = stmt
        .query_map([], |row| {
            let table_name: Option<String> = row.get(8)?;
            let error: Option<String> = row.get(9)?;
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
                table_name,
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
        .prepare("SELECT name, crs, path, table_name FROM files WHERE id = ?")
        .map_err(internal_error)?;

    let meta: Option<(String, Option<String>, String, Option<String>)> = stmt
        .query_row(duckdb::params![id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .ok();

    let (name, crs, _path, table_name) = match meta {
        Some(m) => m,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "File not found".to_string(),
                }),
            ))
        }
    };

    let table_name = table_name.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready for preview".to_string(),
            }),
        )
    })?;

    // Calculate BBOX in WGS84
    // Note: If CRS is missing/null, we assume EPSG:4326 and always_xy=true (lon/lat) for simplicity

    // Query for bbox components directly
    let bbox_components_query = format!(
        "SELECT ST_XMin(b), ST_YMin(b), ST_XMax(b), ST_YMax(b) FROM (
            SELECT ST_Extent(ST_Transform(geom, '{}', 'EPSG:4326', always_xy := true)) as b
            FROM \"{table_name}\"
        )",
        crs.as_deref().unwrap_or("EPSG:4326")
    );

    let bbox_values: Option<[f64; 4]> = conn
        .query_row(&bbox_components_query, [], |row| {
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
    println!(
        "Received tile request: id={}, z={}, x={}, y={}",
        id, z, x, y
    );
    let conn = state.db.lock().await;

    // 1. Get CRS and table for the file
    let (crs, table_name): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT crs, table_name FROM files WHERE id = ?",
            duckdb::params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "File not found".to_string(),
                }),
            )
        })?;

    let table_name = table_name.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready for preview".to_string(),
            }),
        )
    })?;

    let source_crs = crs.as_deref().unwrap_or("EPSG:4326");

    // 2. Generate MVT
    // logic:
    //  - filter by source_id
    //  - ST_Transform(geom, source_crs, 'EPSG:3857', always_xy := true)
    //  - ST_TileEnvelope(z, x, y) to get tile bounds in 3857
    //  - ST_AsMVTGeom(geom_3857, tile_env) to clip/transform to tile coords
    //  - ST_AsMVT(...) to encode

    // 2a. Build property struct keys based on captured column metadata.
    // We keep property keys as original names for UX.
    // Note: We exclude fid + geom.
    let mut props_stmt = conn
        .prepare(
            "SELECT normalized_name, original_name\n             FROM dataset_columns\n             WHERE source_id = ?\n             ORDER BY ordinal",
        )
        .map_err(internal_error)?;
    let props_iter = props_stmt
        .query_map(duckdb::params![id.clone()], |row| {
            let normalized: String = row.get(0)?;
            let original: String = row.get(1)?;
            Ok((normalized, original))
        })
        .map_err(internal_error)?;

    let mut struct_fields = Vec::new();
    struct_fields.push(format!(
        "geom := ST_AsMVTGeom(\n                    ST_Transform(geom, '{source_crs}', 'EPSG:3857', always_xy := true),\n                    ST_Extent(ST_TileEnvelope(?, ?, ?)),\n                    4096, 256, true\n                )"
    ));
    struct_fields.push("fid := fid".to_string());

    for entry in props_iter {
        let (normalized, original) = entry.map_err(internal_error)?;

        // Use the original column name as the MVT property key.
        // DuckDB `struct_pack` uses identifier keys; quoted identifiers allow spaces/symbols.
        // Escape embedded double quotes per SQL identifier rules.
        let key = original.replace('"', "\"\"");
        struct_fields.push(format!("\"{key}\" := \"{normalized}\""));
    }

    let struct_expr = format!(
        "struct_pack(\n                {}\n            )",
        struct_fields.join(",\n                ")
    );

    let select_sql = format!(
        "SELECT ST_AsMVT(feature, 'layer', 4096, 'geom', 'fid') FROM (\n            SELECT {struct_expr} as feature\n            FROM \"{table_name}\"\n            WHERE ST_Intersects(\n                ST_Transform(geom, '{source_crs}', 'EPSG:3857', always_xy := true),\n                ST_TileEnvelope(?, ?, ?)\n            )\n        )"
    );

    println!("Executing SQL for tile z={z} x={x} y={y} id={id}");

    // Params: z, x, y (for AsMVTGeom bounds), z, x, y (for intersects)
    let mvt_blob: Option<Vec<u8>> =
        match conn.query_row(&select_sql, duckdb::params![z, x, y, z, x, y], |row| {
            row.get(0)
        }) {
            Ok(blob) => Some(blob),
            Err(e) => {
                eprintln!("Tile Error (z={z}, x={x}, y={y}): {:?}", e);
                eprintln!("SQL that failed: {}", select_sql);
                return Err(internal_error(format!("Tile generation failed: {}", e)));
            }
        };

    println!(
        "Tile Request: z={z}, x={x}, y={y}, Blob Size: {:?}",
        mvt_blob.as_ref().map(|v| v.len())
    );

    match mvt_blob {
        Some(blob) if !blob.is_empty() => Ok((
            [(header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile")],
            blob,
        )
            .into_response()),
        _ => {
            // Return empty response or 204? Mapbox clients usually expect 200 with empty body or valid PBF.
            // An empty blob is a valid MVT (empty).
            Ok((
                [(header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile")],
                Vec::new(),
            )
                .into_response())
        }
    }
}

async fn upload_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let mut field = loop {
        let next = multipart.next_field().await.map_err(|e| {
            let message = format!("Invalid multipart form: {e}");
            bad_request(&message)
        })?;
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
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            duckdb::params![
                &upload_id,
                &base_name,
                file_type,
                size_i64,
                &uploaded_at,
                "failed",
                &None::<String>,
                &rel_string,
                &None::<String>,
                &Some(message.clone()),
            ],
        )
        .map_err(internal_error)?;

        drop(conn);
        return Err(bad_request(&message));
    }

    let size_i64 = size as i64;
    conn.execute(
        "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                eprintln!(
                    "Failed to import spatial data for {}: {}",
                    upload_id_clone, e
                );
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
        table_name: None,
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
        let _ = conn.execute(
            "UPDATE files SET crs = ? WHERE id = ?",
            duckdb::params![crs, source_id],
        );
    }

    // 2. Import Data into a per-dataset table (layer_<id>) so we can preserve columns.
    // We keep a stable feature id column (fid) for MVT feature ids.
    let table_name = format!("layer_{}", source_id);
    let safe_table_name =
        normalize_column_name(&table_name).unwrap_or_else(|| format!("layer_{}", source_id));

    // Drop if exists (id collision should be impossible, but keep idempotent).
    let _ = conn.execute(&format!("DROP TABLE IF EXISTS \"{safe_table_name}\""), []);

    let create_sql = format!(
        "CREATE TABLE \"{safe_table_name}\" AS\n         SELECT row_number() OVER ()::BIGINT AS fid, *\n         FROM ST_Read('{abs_path}')"
    );

    conn.execute(&create_sql, [])
        .map_err(|e| format!("Spatial import failed: {}", e))?;

    // Record table name on the file record.
    let _ = conn.execute(
        "UPDATE files SET table_name = ? WHERE id = ?",
        duckdb::params![safe_table_name.as_str(), source_id],
    );

    // 3. Normalize/rename columns when needed and capture metadata.
    // DuckDB is case-insensitive for identifiers, so we treat case-only differences as conflicts.
    // Strategy:
    // - Keep original name if it is already a safe identifier and unique (case-insensitive)
    // - Otherwise normalize (lowercase + non [a-z0-9_] -> '_' + trim)
    // - If still conflicts, suffix _2, _3...
    // - Ensure reserved columns fid + geom stay as-is.
    let mut columns_stmt = conn
        .prepare(
            "SELECT column_name, data_type, ordinal_position\n             FROM information_schema.columns\n             WHERE table_schema = 'main' AND table_name = ?\n             ORDER BY ordinal_position",
        )
        .map_err(|e| format!("Metadata query failed: {}", e))?;

    let columns_iter = columns_stmt
        .query_map(duckdb::params![safe_table_name.as_str()], |row| {
            let name: String = row.get(0)?;
            let data_type: String = row.get(1)?;
            let ordinal: i64 = row.get(2)?;
            Ok((name, data_type, ordinal))
        })
        .map_err(|e| format!("Metadata query failed: {}", e))?;

    let mut columns: Vec<(String, String, i64)> = Vec::new();
    for col in columns_iter {
        columns.push(col.map_err(|e| format!("Metadata query failed: {}", e))?);
    }

    // Clear any prior metadata.
    let _ = conn.execute(
        "DELETE FROM dataset_columns WHERE source_id = ?",
        duckdb::params![source_id],
    );

    let mut used: HashSet<String> = HashSet::new();
    used.insert("fid".to_string());

    // Ensure geometry column is named `geom` for downstream queries.
    // Most drivers already use `geom`, but don't rely on it.
    // If we find a GEOMETRY column that isn't named `geom`, rename it.
    for (name, data_type, _ordinal) in &columns {
        if data_type.eq_ignore_ascii_case("GEOMETRY") && name != "geom" {
            let alter =
                format!("ALTER TABLE \"{safe_table_name}\" RENAME COLUMN \"{name}\" TO geom");
            conn.execute(&alter, [])
                .map_err(|e| format!("Failed to normalize geometry column: {}", e))?;
        }
    }

    // Refresh columns after potential geom rename.
    let mut refresh_stmt = conn
        .prepare(
            "SELECT column_name, data_type, ordinal_position\n             FROM information_schema.columns\n             WHERE table_schema = 'main' AND table_name = ?\n             ORDER BY ordinal_position",
        )
        .map_err(|e| format!("Metadata query failed: {}", e))?;

    let columns_iter = refresh_stmt
        .query_map(duckdb::params![safe_table_name.as_str()], |row| {
            let name: String = row.get(0)?;
            let data_type: String = row.get(1)?;
            let ordinal: i64 = row.get(2)?;
            Ok((name, data_type, ordinal))
        })
        .map_err(|e| format!("Metadata query failed: {}", e))?;
    let mut columns: Vec<(String, String, i64)> = Vec::new();
    for col in columns_iter {
        columns.push(col.map_err(|e| format!("Metadata query failed: {}", e))?);
    }

    for (name, data_type, ordinal) in &columns {
        let lower = name.to_ascii_lowercase();
        let is_reserved = lower == "fid" || lower == "geom";

        // Determine normalized name.
        let mut normalized = if is_reserved {
            lower.clone()
        } else if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && (name
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false))
        {
            // Keep as-is but lowercase to match DuckDB identifier behavior.
            lower.clone()
        } else {
            normalize_column_name(name).unwrap_or_else(|| format!("col_{ordinal}"))
        };

        if is_reserved {
            used.insert(normalized.clone());
        } else {
            if normalized.is_empty() {
                normalized = format!("col_{ordinal}");
            }
            let mut candidate = normalized.clone();
            let mut suffix = 2;
            while used.contains(&candidate) {
                candidate = format!("{normalized}_{suffix}");
                suffix += 1;
            }
            normalized = candidate;
            used.insert(normalized.clone());

            if normalized != lower {
                let alter = format!(
                    "ALTER TABLE \"{safe_table_name}\" RENAME COLUMN \"{name}\" TO \"{normalized}\""
                );
                conn.execute(&alter, [])
                    .map_err(|e| format!("Failed to normalize column name: {}", e))?;
            }
        }

        // Coerce unsupported property types to VARCHAR so they can be included in MVT.
        // Keep GEOMETRY as-is.
        let mvt_type = if lower == "geom" {
            "GEOMETRY".to_string()
        } else if lower == "fid" {
            "BIGINT".to_string()
        } else {
            match data_type.as_str() {
                "VARCHAR" | "BOOLEAN" | "DOUBLE" | "FLOAT" | "BIGINT" | "INTEGER" => {
                    data_type.clone()
                }
                "SMALLINT" | "TINYINT" => {
                    let alter = format!(
                        "ALTER TABLE \"{safe_table_name}\" ALTER COLUMN \"{normalized}\" SET DATA TYPE INTEGER"
                    );
                    conn.execute(&alter, [])
                        .map_err(|e| format!("Failed to coerce column type: {}", e))?;
                    "INTEGER".to_string()
                }
                "UBIGINT" | "UINTEGER" | "USMALLINT" | "UTINYINT" => {
                    let alter = format!(
                        "ALTER TABLE \"{safe_table_name}\" ALTER COLUMN \"{normalized}\" SET DATA TYPE BIGINT"
                    );
                    conn.execute(&alter, [])
                        .map_err(|e| format!("Failed to coerce column type: {}", e))?;
                    "BIGINT".to_string()
                }
                _ => {
                    // Cast to VARCHAR in-place.
                    let alter = format!(
                        "ALTER TABLE \"{safe_table_name}\" ALTER COLUMN \"{normalized}\" SET DATA TYPE VARCHAR"
                    );
                    conn.execute(&alter, [])
                        .map_err(|e| format!("Failed to coerce column type: {}", e))?;
                    "VARCHAR".to_string()
                }
            }
        };

        if lower != "geom" && lower != "fid" {
            // Record property columns (exclude geom + fid).
            let _ = conn.execute(
                "INSERT INTO dataset_columns (source_id, normalized_name, original_name, ordinal, mvt_type)\n                 VALUES (?1, ?2, ?3, ?4, ?5)",
                duckdb::params![
                    source_id,
                    normalized.as_str(),
                    name.as_str(),
                    *ordinal,
                    mvt_type.as_str()
                ],
            );
        }
    }

    Ok(())
}

fn normalize_column_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }

    while out.contains("__") {
        out = out.replace("__", "_");
    }

    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        return None;
    }

    let first = out.chars().next().unwrap();
    let mut out = if first.is_ascii_alphabetic() || first == '_' {
        out
    } else {
        format!("col_{out}")
    };

    // Avoid a small set of very common keywords.
    // DuckDB has more, but we mainly want to dodge obvious foot-guns.
    const KEYWORDS: [&str; 10] = [
        "select", "from", "where", "group", "order", "by", "limit", "offset", "join", "table",
    ];
    if KEYWORDS.contains(&out.as_str()) {
        out = format!("col_{out}");
    }

    Some(out)
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
            error: "Internal Server Error".to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use std::sync::OnceLock;
    use tempfile::TempDir;
    use tower::util::ServiceExt;

    static ENV_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();

    async fn setup_state(max_size: u64) -> (AppState, TempDir) {
        let temp_dir = TempDir::new().expect("temp dir");
        let upload_dir = temp_dir.path().join("uploads");
        fs::create_dir_all(&upload_dir).await.ok();

        let conn = duckdb::Connection::open_in_memory().expect("Failed to create test database");
        conn.execute_batch("INSTALL spatial; LOAD spatial;")
            .unwrap();

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
            table_name VARCHAR,
            error VARCHAR
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

        let state = AppState {
            upload_dir,
            db: Arc::new(Mutex::new(conn)),
            max_size,
            max_size_label: format_bytes(max_size),
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
            path: file_path.to_string_lossy().to_string(),
            table_name: None,
            error: None,
        };

        let conn = state.db.lock().await;
        let size = item.size as i64;
        conn.execute(
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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

        let default_mb: u64 = 200;
        let bytes_per_mb: u64 = 1024 * 1024;

        std::env::remove_var("UPLOAD_MAX_SIZE_MB");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, default_mb * bytes_per_mb);
        assert_eq!(label, "200MB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "12");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, 12 * bytes_per_mb);
        assert_eq!(label, "12MB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "0");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, default_mb * bytes_per_mb);
        assert_eq!(label, "200MB");

        std::env::set_var("UPLOAD_MAX_SIZE_MB", "nope");
        let (bytes, label) = read_max_size_config();
        assert_eq!(bytes, default_mb * bytes_per_mb);
        assert_eq!(label, "200MB");
        std::env::remove_var("UPLOAD_MAX_SIZE_MB");
    }
}
