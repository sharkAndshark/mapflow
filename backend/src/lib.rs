use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_login::AuthManagerLayerBuilder;
use chrono::Utc;
use rand::RngCore;
use std::path::{Path, PathBuf};
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};
use tower_http::cors::CorsLayer;
use tower_sessions::SessionManagerLayer;

mod auth;
mod auth_routes;
mod config;
mod db;
mod http_errors;
mod import;
mod models;
mod password;
mod session_store;
mod test_routes;
mod tiles;
mod validation;

pub use auth::{AuthBackend, User};
pub use auth_routes::build_auth_router;
pub use config::{format_bytes, read_cookie_secure, read_max_size_config};
pub use db::{
    init_database, is_initialized, reconcile_processing_files, set_initialized, DEFAULT_DB_PATH,
    PROCESSING_RECONCILIATION_ERROR,
};
use duckdb::types::ValueRef;
use http_errors::{bad_request, internal_error, payload_too_large};
use import::import_spatial_data;
pub use models::{
    AppState, ErrorResponse, FileItem, FileSchemaResponse, PreviewMeta, PublicTileUrl,
    PublishRequest, PublishResponse,
};
use models::{FeaturePropertiesResponse, FeatureProperty};
pub use password::{hash_password, validate_password_complexity, verify_password, PasswordError};
pub use session_store::DuckDBStore;
use test_routes::add_test_routes;
use tiles::build_mvt_select_sql;
pub use validation::{validate_geojson, validate_shapefile_zip};

pub fn build_api_router(state: AppState) -> Router {
    build_api_router_with_auth(state, true)
}

pub fn build_test_router(state: AppState) -> Router {
    build_api_router_with_auth(state, false)
}

fn build_api_router_with_auth(state: AppState, with_auth: bool) -> Router {
    // Read allowed origins from environment or use defaults
    let allowed_origins = config::read_cors_origins();

    // Build CORS layer with specific origins
    // Note: When using credentials, we cannot use wildcards for headers
    let mut cors = CorsLayer::new()
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::ACCEPT,
            axum::http::header::AUTHORIZATION,
        ])
        .allow_credentials(true);

    // Add each allowed origin
    for origin in allowed_origins {
        if let Ok(parsed) = origin.parse::<axum::http::HeaderValue>() {
            cors = cors.allow_origin(parsed);
        } else {
            eprintln!("Warning: Failed to parse CORS origin '{}', skipping. Check CORS_ALLOWED_ORIGINS environment variable.", origin);
        }
    }

    let session_layer = SessionManagerLayer::new(state.session_store.clone())
        .with_secure(config::read_cookie_secure())
        .with_same_site(tower_cookies::cookie::SameSite::Lax);

    let auth_layer =
        AuthManagerLayerBuilder::new(state.auth_backend.clone(), session_layer).build();

    let auth_router = build_auth_router();
    let public_router = Router::new()
        .route("/api/test/is-initialized", get(check_is_initialized))
        .route("/tiles/{slug}/{z}/{x}/{y}", get(get_public_tile));

    let mut api_router = Router::new()
        .route("/api/files", get(list_files))
        .route("/api/uploads", post(upload_file))
        .route("/api/files/{id}/preview", get(get_preview_meta))
        .route("/api/files/{id}/tiles/{z}/{x}/{y}", get(get_tile))
        .route(
            "/api/files/{id}/features/{fid}",
            get(get_feature_properties),
        )
        .route("/api/files/{id}/schema", get(get_file_schema))
        .route("/api/files/{id}/publish", post(publish_file))
        .route("/api/files/{id}/unpublish", post(unpublish_file))
        .route("/api/files/{id}/public-url", get(get_public_url));

    // Add authentication middleware if required
    if with_auth {
        api_router = api_router.route_layer(axum_login::login_required!(crate::AuthBackend));
    }

    // Combine all routes
    let router = auth_router
        .merge(public_router)
        .merge(api_router)
        .merge(add_test_routes(Router::new()));

    router
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
        .layer(auth_layer)
        .layer(cors)
}

async fn list_files(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT f.id, f.name, f.type, f.size, f.uploaded_at, f.status, f.crs, f.path, f.table_name, f.error, f.is_public, pf.slug
          FROM files f
          LEFT JOIN published_files pf ON f.id = pf.file_id
          ORDER BY f.uploaded_at DESC",
        )
        .unwrap();

    let items: Vec<FileItem> = stmt
        .query_map([], |row| {
            let table_name: Option<String> = row.get(8)?;
            let error: Option<String> = row.get(9)?;
            let is_public: bool = row.get(10).unwrap_or(false);
            let public_slug: Option<String> = row.get(11).ok();
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
                is_public: Some(is_public),
                public_slug,
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
        .prepare("SELECT name, crs, status, table_name FROM files WHERE id = ?")
        .map_err(internal_error)?;

    let meta: Option<(String, Option<String>, String, Option<String>)> = stmt
        .query_row(duckdb::params![id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .ok();

    let (name, crs, status, table_name) = match meta {
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

    let table_name = table_name.filter(|_| status == "ready").ok_or_else(|| {
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
    validate_tile_coords(z, x, y)?;

    println!(
        "Received tile request: id={}, z={}, x={}, y={}",
        id, z, x, y
    );
    let conn = state.db.lock().await;

    // 1. Get CRS and table for the file
    let (crs, status, table_name): (Option<String>, String, Option<String>) = conn
        .query_row(
            "SELECT crs, status, table_name FROM files WHERE id = ?",
            duckdb::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "File not found".to_string(),
                }),
            )
        })?;

    let table_name = table_name.filter(|_| status == "ready").ok_or_else(|| {
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
    let select_sql =
        build_mvt_select_sql(&conn, &id, &table_name, source_crs).map_err(internal_error)?;

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

async fn get_feature_properties(
    State(state): State<AppState>,
    AxumPath((id, fid)): AxumPath<(String, i64)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (status, table_name): (String, Option<String>) = conn
        .query_row(
            "SELECT status, table_name FROM files WHERE id = ?",
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

    let table_name = table_name.filter(|_| status == "ready").ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready for preview".to_string(),
            }),
        )
    })?;

    let mut cols_stmt = conn
        .prepare(
            "SELECT normalized_name, original_name\n         FROM dataset_columns\n         WHERE source_id = ?\n         ORDER BY ordinal",
        )
        .map_err(internal_error)?;

    let cols_iter = cols_stmt
        .query_map(duckdb::params![&id], |row| {
            let normalized: String = row.get(0)?;
            let original: String = row.get(1)?;
            Ok((normalized, original))
        })
        .map_err(internal_error)?;

    let mut columns: Vec<(String, String)> = Vec::new();
    for c in cols_iter {
        columns.push(c.map_err(internal_error)?);
    }

    // Build a projection that preserves ordering and uses safe identifiers.
    let mut select_exprs: Vec<String> = Vec::with_capacity(columns.len());
    for (normalized, _original) in &columns {
        select_exprs.push(format!("\"{normalized}\""));
    }

    let sql = format!(
        "SELECT {} FROM \"{}\" WHERE fid = ?",
        select_exprs.join(", "),
        table_name
    );

    let mut stmt = conn.prepare(&sql).map_err(internal_error)?;
    let mut rows = stmt.query(duckdb::params![fid]).map_err(internal_error)?;

    let Some(row) = rows.next().map_err(internal_error)? else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Feature not found".to_string(),
            }),
        ));
    };

    let mut properties: Vec<FeatureProperty> = Vec::with_capacity(columns.len());
    for (index, (_normalized, original)) in columns.iter().enumerate() {
        let raw = match row.get_ref(index).map_err(internal_error)? {
            ValueRef::Null => serde_json::Value::Null,
            ValueRef::Boolean(v) => serde_json::Value::Bool(v),
            ValueRef::TinyInt(v) => serde_json::Value::Number(v.into()),
            ValueRef::SmallInt(v) => serde_json::Value::Number(v.into()),
            ValueRef::Int(v) => serde_json::Value::Number(v.into()),
            ValueRef::BigInt(v) => serde_json::Value::Number(v.into()),
            ValueRef::UTinyInt(v) => serde_json::Value::Number(v.into()),
            ValueRef::USmallInt(v) => serde_json::Value::Number(v.into()),
            ValueRef::UInt(v) => serde_json::Value::Number(v.into()),
            ValueRef::UBigInt(v) => serde_json::Value::Number(v.into()),
            ValueRef::Float(v) => serde_json::Number::from_f64(v as f64)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            ValueRef::Double(v) => serde_json::Number::from_f64(v)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            ValueRef::Text(bytes) => {
                serde_json::Value::String(String::from_utf8_lossy(bytes).to_string())
            }
            ValueRef::Blob(bytes) => serde_json::Value::String(format!("0x{}", hex::encode(bytes))),
            other => serde_json::Value::String(format!("{other:?}")),
        };
        properties.push(FeatureProperty {
            key: original.clone(),
            value: raw,
        });
    }

    Ok(Json(FeaturePropertiesResponse { fid, properties }))
}

async fn get_file_schema(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let status: String = conn
        .query_row(
            "SELECT status FROM files WHERE id = ?",
            duckdb::params![id],
            |row| row.get(0),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "File not found".to_string(),
                }),
            )
        })?;

    if status != "ready" {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready".to_string(),
            }),
        ));
    }

    let mut cols_stmt = conn
        .prepare(
            "SELECT original_name, mvt_type\n         FROM dataset_columns\n         WHERE source_id = ?\n         ORDER BY ordinal",
        )
        .map_err(internal_error)?;

    let cols_iter = cols_stmt
        .query_map(duckdb::params![&id], |row| {
            let original_name: String = row.get(0)?;
            let mvt_type: String = row.get(1)?;
            Ok((original_name, mvt_type))
        })
        .map_err(internal_error)?;

    let mut fields = Vec::new();
    for c in cols_iter {
        let (name, r#type) = c.map_err(internal_error)?;
        fields.push(models::FieldInfo { name, r#type });
    }

    drop(conn);
    Ok(Json(models::FileSchemaResponse { fields }))
}

fn validate_tile_coords(z: i32, x: i32, y: i32) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    // Practical cap. This is plenty for web maps and keeps bounds math simple.
    const MAX_Z: i32 = 22;

    if z < 0 || x < 0 || y < 0 || z > MAX_Z {
        return Err(bad_request("Invalid tile coordinates"));
    }

    let max_xy: i32 = 1_i32 << z;
    if x >= max_xy || y >= max_xy {
        return Err(bad_request("Invalid tile coordinates"));
    }

    Ok(())
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
        .ok_or_else(|| bad_request("Unsupported file type. Use .zip, .geojson, .json, .geojsonl, .kml, .gpx, or .topojson"))?;

    let file_type = match ext.as_str() {
        ".zip" => "shapefile",
        ".geojson" | ".json" => "geojson",
        ".geojsonl" | ".geojsons" => "geojsonl",
        ".kml" => "kml",
        ".gpx" => "gpx",
        ".topojson" => "topojson",
        _ => return Err(bad_request(
            "Unsupported file type. Use .zip, .geojson, .json, .geojsonl, .kml, .gpx, or .topojson",
        )),
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
        "geojsonl" | "kml" | "gpx" | "topojson" => Ok(()), // Trust GDAL to validate
        _ => Ok(()), // Unreachable due to earlier validation, but required for type safety
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
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                false,
            ],
        )
        .map_err(internal_error)?;

        drop(conn);
        return Err(bad_request(&message));
    }

    let size_i64 = size as i64;
    conn.execute(
        "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        duckdb::params![
            &upload_id,
            &base_name,
            file_type,
            size_i64,
            &uploaded_at,
            "uploaded",
            &None::<String>,
            &rel_string,
            &None::<String>,
            &None::<String>,
            false,
        ],
    )
    .map_err(internal_error)?;

    drop(conn);

    let db = state.db.clone();
    let upload_id_clone = upload_id.clone();
    let file_path_clone = file_path.clone();
    tokio::spawn(async move {
        // Set status to processing
        {
            let conn = db.lock().await;
            let _ = conn.execute(
                "UPDATE files SET status = 'processing' WHERE id = ?",
                duckdb::params![upload_id_clone],
            );
        }

        match import_spatial_data(&db, &upload_id_clone, &file_path_clone).await {
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
        status: "uploaded".to_string(),
        crs: None,
        path: rel_string,
        table_name: None,
        error: None,
        is_public: Some(false),
        public_slug: None,
    };

    Ok((StatusCode::CREATED, Json(meta)))
}

async fn check_is_initialized(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    match is_initialized(&conn) {
        Ok(initialized) => (
            StatusCode::OK,
            Json(serde_json::json!({ "initialized": initialized })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to check initialization status: {}", e) })),
        )
            .into_response(),
    }
}

async fn publish_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PublishRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (status, _name): (String, String) = conn
        .query_row(
            "SELECT status, name FROM files WHERE id = ?",
            duckdb::params![&id],
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

    if status != "ready" {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready for publishing".to_string(),
            }),
        ));
    }

    let slug = match req.slug {
        Some(s) => validate_slug(&s).map_err(|e| bad_request(&e))?,
        None => validate_slug(&id).map_err(|e| bad_request(&e))?,
    };

    // Use transaction to ensure atomicity: insert into published_files first (enforces uniqueness),
    // then update files table. This eliminates race conditions for concurrent publish requests.
    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(internal_error)?;

    // Check file status within transaction to provide better error messages
    let (status, _name): (String, String) = conn
        .query_row(
            "SELECT status, name FROM files WHERE id = ?",
            duckdb::params![&id],
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

    if status != "ready" {
        conn.execute_batch("ROLLBACK").map_err(internal_error)?;
        drop(conn);
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!("File is not ready for publishing (status: {})", status),
            }),
        ));
    }

    let insert_result = conn.execute(
        "INSERT INTO published_files (file_id, slug) VALUES (?, ?)",
        duckdb::params![&id, &slug],
    );

    let publish_result: Result<(), String> = match insert_result {
        Ok(_) => conn
            .execute(
                "UPDATE files SET is_public = TRUE WHERE id = ?",
                duckdb::params![&id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Err(e) => {
            let err_msg = e.to_string();
            // Check for PRIMARY KEY constraint on file_id (file already published) vs UNIQUE constraint on slug
            // More specific detection: PRIMARY KEY on file_id, or constraint error mentioning file
            let is_file_already_published = err_msg.contains("PRIMARY KEY")
                || (err_msg.contains("Constraint Error") && err_msg.contains("file_id"));

            if is_file_already_published {
                // Immediately rollback the failed transaction
                // After ROLLBACK, the connection returns to autocommit mode, allowing us to query
                // without an active transaction. This is safe because:
                // - The INSERT failed, so the transaction is aborted
                // - We need to query for the existing slug to provide a helpful error message
                // - The subsequent query runs in autocommit mode and does not affect database state
                conn.execute_batch("ROLLBACK").map_err(internal_error)?;

                // Query for existing slug (connection now in autocommit mode)
                // We use .ok() here to convert "not found" to None. Any other error is also
                // converted to None, which is acceptable given:
                // - PRIMARY KEY constraint just confirmed the file exists
                // - Query is simple and likely to succeed
                // - If query fails, we return a generic but accurate error message
                let existing_slug: Option<String> = conn
                    .query_row(
                        "SELECT slug FROM published_files WHERE file_id = ?",
                        duckdb::params![&id],
                        |row| row.get(0),
                    )
                    .ok();

                drop(conn);

                let error_msg = if let Some(existing) = existing_slug {
                    format!(
                        "File already published with slug '{existing}'. Unpublish first to change slug."
                    )
                } else {
                    "File already published. Unpublish first to change slug.".to_string()
                };

                // Return early with the error (skip the outer match's ROLLBACK)
                return Err((
                    StatusCode::CONFLICT,
                    Json(ErrorResponse { error: error_msg }),
                ));
            } else if err_msg.contains("UNIQUE")
                || (err_msg.contains("slug") && err_msg.contains("unique"))
            {
                Err("Slug already in use".to_string())
            } else {
                Err(err_msg)
            }
        }
    };

    match publish_result {
        Ok(()) => {
            conn.execute_batch("COMMIT").map_err(internal_error)?;
            drop(conn);
            Ok(Json(PublishResponse {
                url: format!("/tiles/{slug}/{{z}}/{{x}}/{{y}}"),
                slug,
                is_public: true,
            }))
        }
        Err(err_msg) => {
            conn.execute_batch("ROLLBACK").map_err(internal_error)?;
            drop(conn);
            Err(bad_request(&err_msg))
        }
    }
}

async fn unpublish_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    // Use transaction to ensure atomicity: delete from published_files and update files table
    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(internal_error)?;

    // Delete from published_files and verify file is actually published (is_public=TRUE)
    // This ensures we don't leave orphaned published_files entries if files.is_public is FALSE
    let rows_affected = conn
        .execute(
            "DELETE FROM published_files pf
            JOIN files f ON pf.file_id = f.id
            WHERE pf.file_id = ? AND f.is_public = TRUE",
            duckdb::params![&id],
        )
        .map_err(internal_error)?;

    if rows_affected == 0 {
        conn.execute_batch("ROLLBACK").map_err(internal_error)?;
        drop(conn);
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "File not published".to_string(),
            }),
        ));
    }

    let update_result = conn
        .execute(
            "UPDATE files SET is_public = FALSE WHERE id = ?",
            duckdb::params![&id],
        )
        .map_err(|e| e.to_string());

    match update_result {
        Ok(_) => {
            conn.execute_batch("COMMIT").map_err(internal_error)?;
            drop(conn);
            Ok(Json(serde_json::json!({ "message": "File unpublished" })))
        }
        Err(err_msg) => {
            conn.execute_batch("ROLLBACK").map_err(internal_error)?;
            drop(conn);
            Err(internal_error(err_msg.as_str()))
        }
    }
}

async fn get_public_url(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let result: Option<(String, String)> = conn
        .query_row(
            "SELECT pf.slug, pf.published_at FROM published_files pf JOIN files f ON pf.file_id = f.id WHERE f.id = ? AND f.is_public = TRUE",
            duckdb::params![&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    drop(conn);

    match result {
        Some((slug, _published_at)) => Ok(Json(PublicTileUrl {
            slug: slug.clone(),
            url: format!("/tiles/{slug}/{{z}}/{{x}}/{{y}}"),
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "File not published".to_string(),
            }),
        )),
    }
}

async fn get_public_tile(
    State(state): State<AppState>,
    AxumPath((slug, z, x, y)): AxumPath<(String, i32, i32, i32)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    validate_tile_coords(z, x, y)?;

    let conn = state.db.lock().await;

    // Step 1: Get file_id from published_files using slug (enforces uniqueness)
    let file_id: String = conn
        .query_row(
            "SELECT file_id FROM published_files WHERE slug = ?",
            duckdb::params![&slug],
            |row| row.get(0),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Public tile not found".to_string(),
                }),
            )
        })?;

    // Step 2: Get file metadata from files table, verifying is_public flag
    let (crs, status, table_name): (Option<String>, String, Option<String>) = conn
        .query_row(
            "SELECT crs, status, table_name FROM files WHERE id = ? AND is_public = TRUE",
            duckdb::params![&file_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "File not found".to_string(),
                }),
            )
        })?;

    let table_name = table_name.filter(|_| status == "ready").ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready".to_string(),
            }),
        )
    })?;

    let source_crs = crs.as_deref().unwrap_or("EPSG:4326");

    let select_sql =
        build_mvt_select_sql(&conn, &file_id, &table_name, source_crs).map_err(internal_error)?;

    let mvt_blob: Option<Vec<u8>> =
        match conn.query_row(&select_sql, duckdb::params![z, x, y, z, x, y], |row| {
            row.get(0)
        }) {
            Ok(blob) => Some(blob),
            Err(e) => {
                eprintln!("Tile Error (z={z}, x={x}, y={y}): {:?}", e);
                return Err(internal_error(format!("Tile generation failed: {}", e)));
            }
        };

    match mvt_blob {
        Some(blob) if !blob.is_empty() => Ok((
            [
                (header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile"),
                (header::CACHE_CONTROL, "public, max-age=300"),
            ],
            blob,
        )
            .into_response()),
        _ => Ok((
            [
                (header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile"),
                (header::CACHE_CONTROL, "public, max-age=300"),
            ],
            Vec::new(),
        )
            .into_response()),
    }
}

fn validate_slug(slug: &str) -> Result<String, String> {
    let slug = slug.trim().to_string();

    if slug.is_empty() {
        return Err("Slug cannot be empty".to_string());
    }

    if slug.len() > 100 {
        return Err("Slug must be 100 characters or less".to_string());
    }

    if !slug
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Slug can only contain letters, numbers, hyphens, and underscores".to_string());
    }

    Ok(slug)
}

fn create_id() -> String {
    let mut bytes = [0u8; 3];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
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
    use tokio::sync::Mutex;
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
            error VARCHAR,
            is_public BOOLEAN DEFAULT FALSE
        );

        CREATE TABLE IF NOT EXISTS published_files (
            file_id VARCHAR PRIMARY KEY,
            slug VARCHAR UNIQUE NOT NULL,
            published_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
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
            db: conn.clone(),
            max_size,
            max_size_label: format_bytes(max_size),
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
    #[ignore = "needs authentication"]
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
            is_public: Some(false),
            public_slug: None,
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
    fn read_cookie_secure_from_env() {
        let _guard = ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env lock");

        // Default to false
        std::env::remove_var("COOKIE_SECURE");
        assert!(!read_cookie_secure());

        // Explicitly set to false
        std::env::set_var("COOKIE_SECURE", "false");
        assert!(!read_cookie_secure());

        std::env::set_var("COOKIE_SECURE", "true");
        assert!(read_cookie_secure());

        // Invalid value falls back to default false
        std::env::set_var("COOKIE_SECURE", "invalid");
        assert!(!read_cookie_secure());

        std::env::remove_var("COOKIE_SECURE");
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
