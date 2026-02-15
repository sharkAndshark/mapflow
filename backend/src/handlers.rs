use axum::{
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use duckdb::types::ValueRef;

use crate::{
    http_errors::{bad_request, internal_error},
    mbtiles,
    models::{ErrorResponse, FeaturePropertiesResponse, FeatureProperty, FileItem, PreviewMeta},
    tiles::build_mvt_select_sql,
    AppState,
};

pub type FileMetadata = (
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
);

pub async fn list_files(State(state): State<AppState>) -> impl IntoResponse {
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

pub async fn get_preview_meta(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let mut stmt = conn
        .prepare("SELECT name, crs, status, table_name, tile_format, tile_bounds, minzoom, maxzoom FROM files WHERE id = ?")
        .map_err(internal_error)?;

    let meta: Option<FileMetadata> = stmt
        .query_row(duckdb::params![id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })
        .ok();

    let (name, crs, status, table_name, tile_format, tile_bounds, minzoom, maxzoom) = match meta {
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

    if status != "ready" {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready for preview".to_string(),
            }),
        ));
    }

    let bbox_values = if let Some(bounds_json) = tile_bounds {
        serde_json::from_str::<[f64; 4]>(&bounds_json).ok()
    } else if let Some(tbl) = table_name {
        let bbox_components_query = format!(
            "SELECT ST_XMin(b), ST_YMin(b), ST_XMax(b), ST_YMax(b) FROM (
                SELECT ST_Extent(ST_Transform(geom, '{}', 'EPSG:4326', always_xy := true)) as b
                FROM \"{tbl}\"
            )",
            crs.as_deref().unwrap_or("EPSG:4326")
        );

        conn.query_row(&bbox_components_query, [], |row| {
            let minx: Option<f64> = row.get(0).ok();
            let miny: Option<f64> = row.get(1).ok();
            let maxx: Option<f64> = row.get(2).ok();
            let maxy: Option<f64> = row.get(3).ok();

            if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (minx, miny, maxx, maxy) {
                Ok([x1, y1, x2, y2])
            } else {
                Ok([0.0, 0.0, 0.0, 0.0])
            }
        })
        .ok()
        .filter(|b| b != &[0.0, 0.0, 0.0, 0.0])
    } else {
        None
    };

    Ok(Json(PreviewMeta {
        id,
        name,
        crs,
        bbox: bbox_values,
        tile_format,
        minzoom,
        maxzoom,
    }))
}

pub async fn get_tile(
    State(state): State<AppState>,
    AxumPath((id, z, x, y)): AxumPath<(String, i32, i32, i32)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    validate_tile_coords(z, x, y)?;

    println!(
        "Received tile request: id={}, z={}, x={}, y={}",
        id, z, x, y
    );
    let conn = state.db.lock().await;

    let (crs, status, table_name, tile_format, file_path): (
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT crs, status, table_name, tile_format, path FROM files WHERE id = ?",
            duckdb::params![id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
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
                error: "File is not ready for preview".to_string(),
            }),
        ));
    }

    if let Some(format) = tile_format {
        let full_path = mbtiles::resolve_mbtiles_path(&file_path);
        drop(conn);
        match mbtiles::get_tile_from_mbtiles(&full_path, z, x, y).await {
            Ok(Some(data)) => {
                let ct = match format.as_str() {
                    "mvt" => "application/vnd.mapbox-vector-tile",
                    "png" => "image/png",
                    _ => "application/octet-stream",
                };

                let is_gzipped = data.starts_with(&[0x1f, 0x8b]);

                if is_gzipped {
                    return Ok((
                        [
                            (header::CONTENT_TYPE, ct),
                            (header::CONTENT_ENCODING, "gzip"),
                        ],
                        data,
                    )
                        .into_response());
                } else {
                    return Ok(([(header::CONTENT_TYPE, ct)], data).into_response());
                }
            }
            Ok(None) => {
                return Ok(StatusCode::NO_CONTENT.into_response());
            }
            Err(e) => {
                return Err(internal_error(format!("Failed to read MBTiles: {}", e)));
            }
        }
    }

    let table_name = table_name.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready for preview".to_string(),
            }),
        )
    })?;

    let source_crs = crs.as_deref().unwrap_or("EPSG:4326");

    let select_sql =
        build_mvt_select_sql(&conn, &id, &table_name, source_crs).map_err(internal_error)?;

    println!("Executing SQL for tile z={z} x={x} y={y} id={id}");

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
        _ => Ok((
            [(header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile")],
            Vec::new(),
        )
            .into_response()),
    }
}

pub async fn get_feature_properties(
    State(state): State<AppState>,
    AxumPath((id, fid)): AxumPath<(String, i64)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (status, table_name, tile_format): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, table_name, tile_format FROM files WHERE id = ?",
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

    if tile_format.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Feature properties not available for MBTiles files".to_string(),
            }),
        ));
    }

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

pub async fn get_file_schema(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (status, tile_format, file_path): (String, Option<String>, String) = conn
        .query_row(
            "SELECT status, tile_format, path FROM files WHERE id = ?",
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

    if status != "ready" {
        drop(conn);
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready".to_string(),
            }),
        ));
    }

    if let Some(format) = tile_format {
        drop(conn);
        if format == "mvt" {
            let full_path = mbtiles::resolve_mbtiles_path(&file_path);
            match mbtiles::extract_mbtiles_layers_async(&full_path).await {
                Ok(layers) => {
                    return Ok(Json(crate::models::FileSchemaResponse { layers }));
                }
                Err(e) => {
                    eprintln!("Failed to extract MBTiles layers for {}: {}", id, e);
                    eprintln!("  File path: {}", full_path.display());
                    eprintln!("  Tile format: {}", format);
                    return Ok(Json(crate::models::FileSchemaResponse { layers: vec![] }));
                }
            }
        } else {
            return Ok(Json(crate::models::FileSchemaResponse { layers: vec![] }));
        }
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
        fields.push(crate::models::FieldInfo { name, r#type });
    }

    drop(conn);

    let default_layer = crate::models::LayerInfo {
        id: "default".to_string(),
        description: None,
        fields,
    };
    Ok(Json(crate::models::FileSchemaResponse {
        layers: vec![default_layer],
    }))
}

pub fn validate_tile_coords(
    z: i32,
    x: i32,
    y: i32,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
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

pub async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

pub async fn check_is_initialized(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    match crate::db::is_initialized(&conn) {
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

pub fn validate_slug(slug: &str) -> Result<String, String> {
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

pub async fn publish_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<crate::models::PublishRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let slug = match req.slug {
        Some(s) => validate_slug(&s).map_err(|e| bad_request(&e))?,
        None => validate_slug(&id).map_err(|e| bad_request(&e))?,
    };

    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(internal_error)?;

    let (status, _name, tile_source): (String, String, String) = conn
        .query_row(
            "SELECT status, name, COALESCE(tile_source, 'duckdb') FROM files WHERE id = ?",
            duckdb::params![&id],
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
        "INSERT INTO published_files (file_id, slug, tile_source) VALUES (?, ?, ?)",
        duckdb::params![&id, &slug, &tile_source],
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
            let is_file_already_published = err_msg.contains("PRIMARY KEY")
                || (err_msg.contains("Constraint Error") && err_msg.contains("file_id"));

            if is_file_already_published {
                conn.execute_batch("ROLLBACK").map_err(internal_error)?;

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
            let url = if tile_source == "pmtiles" {
                format!("/tiles/{slug}")
            } else {
                format!("/tiles/{slug}/{{z}}/{{x}}/{{y}}")
            };
            Ok(Json(crate::models::PublishResponse {
                url,
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

pub async fn unpublish_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(internal_error)?;

    let rows_affected = conn
        .execute(
            "DELETE FROM published_files 
            WHERE file_id = ? AND file_id IN (SELECT id FROM files WHERE is_public = TRUE)",
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

pub async fn get_public_url(
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
        Some((slug, _published_at)) => Ok(Json(crate::models::PublicTileUrl {
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
