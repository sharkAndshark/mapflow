use axum::{
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use axum_login::AuthSession;
use duckdb::types::ValueRef;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::{format_bytes, save_max_size_to_db, validate_upload_size_mb, BYTES_PER_MB};
use crate::{
    crs::{
        is_wgs84_compatible_crs, normalize_crs, resolve_transform_source_crs, DataBounds,
        CRS_TYPE_CUSTOM,
    },
    http_errors::{bad_request, internal_error},
    mbtiles,
    models::{
        ErrorResponse, FeaturePropertiesResponse, FeatureProperty, FileItem, PreviewMeta,
        UpdateZoomRequest,
    },
    postgis::{
        build_feature_properties, build_property_columns_for_query, fetch_postgis_source_config,
        query_feature_properties_json, query_mvt_tile, TILE_SOURCE_POSTGIS,
    },
    read_preview_zoom_config,
    tiles::{build_mvt_query_params, build_mvt_select_sql, TileParams},
    AppState,
};

pub type FileMetadata = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<String>,
);

pub type TileFileMetadata = (
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
);

pub async fn list_files(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT f.id, f.name, f.type, f.size, f.uploaded_at, f.status, f.crs, f.crs_type, f.path, f.table_name, f.error, f.is_public, pf.slug, f.tile_format, f.minzoom, f.maxzoom, pf.use_aliases, f.tile_source
          FROM files f
          LEFT JOIN published_files pf ON f.id = pf.file_id
          ORDER BY f.uploaded_at DESC",
        )
        .map_err(internal_error)?;

    let items_iter = stmt
        .query_map([], |row| {
            let crs_type: Option<String> = row.get(7)?;
            let path: String = row.get(8)?;
            let table_name: Option<String> = row.get(9)?;
            let error: Option<String> = row.get(10)?;
            let is_public: bool = row.get(11).unwrap_or(false);
            let public_slug: Option<String> = row.get(12).ok();
            let tile_format: Option<String> = row.get(13)?;
            let minzoom: Option<i32> = row.get(14)?;
            let maxzoom: Option<i32> = row.get(15)?;
            let use_aliases: Option<bool> = row.get(16)?;
            let tile_source: Option<String> = row.get(17)?;
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
                crs_type,
                path,
                table_name,
                error,
                is_public: Some(is_public),
                public_slug,
                tile_format,
                minzoom,
                maxzoom,
                use_aliases,
                tile_source,
            })
        })
        .map_err(internal_error)?;

    let mut items: Vec<FileItem> = Vec::new();
    for item in items_iter {
        items.push(item.map_err(internal_error)?);
    }

    drop(conn);
    Ok(Json(items))
}

pub async fn get_preview_meta(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let mut stmt = conn
        .prepare("SELECT name, crs, crs_type, data_bounds, status, table_name, tile_format, tile_bounds, minzoom, maxzoom, tile_source FROM files WHERE id = ?")
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
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
            ))
        })
        .ok();

    let (
        name,
        crs,
        crs_type,
        data_bounds_json,
        status,
        table_name,
        tile_format,
        tile_bounds,
        minzoom,
        maxzoom,
        tile_source,
    ) = match meta {
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

    let crs_type = crs_type.unwrap_or_else(|| "standard".to_string());
    let data_bounds: Option<DataBounds> = data_bounds_json
        .as_ref()
        .and_then(|j| DataBounds::from_json(j));
    let data_bounds_array = data_bounds.as_ref().map(|b| b.to_array());

    let tile_source = tile_source.unwrap_or_else(|| "duckdb".to_string());
    let bbox_values = if let Some(bounds_json) = tile_bounds {
        serde_json::from_str::<[f64; 4]>(&bounds_json).ok()
    } else {
        None
    }
    .or_else(|| {
        if tile_source == TILE_SOURCE_POSTGIS {
            return data_bounds_array;
        }

        if crs_type == CRS_TYPE_CUSTOM {
            return data_bounds_array;
        }

        if let Some(tbl) = &table_name {
            let transform_source_crs = resolve_transform_source_crs(crs.as_deref());
            let bbox_components_query = format!(
                "SELECT ST_XMin(b), ST_YMin(b), ST_XMax(b), ST_YMax(b) FROM (
                    SELECT ST_Extent(ST_Transform(geom, '{}', 'EPSG:4326', always_xy := true)) as b
                    FROM \"{tbl}\"
                )",
                transform_source_crs
            );

            let transformed_bbox = conn
                .query_row(&bbox_components_query, [], |row| {
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
                .filter(|b| b != &[0.0, 0.0, 0.0, 0.0]);

            if transformed_bbox.is_some() {
                return transformed_bbox;
            }
        }

        if is_wgs84_compatible_crs(crs.as_deref()) {
            return data_bounds_array;
        }

        None
    });

    // For preview: tile files use file metadata zoom, dynamic data uses fixed range
    let (preview_minzoom, preview_maxzoom) = if tile_format.is_some() {
        // Tile file: use file metadata zoom
        (minzoom, maxzoom)
    } else {
        // Dynamic data: use fixed preview zoom range from config
        let (min, max) = read_preview_zoom_config();
        (Some(min), Some(max))
    };

    Ok(Json(PreviewMeta {
        id,
        name,
        crs,
        crs_type,
        bbox: bbox_values,
        data_bounds: data_bounds_array,
        tile_format,
        minzoom: preview_minzoom,
        maxzoom: preview_maxzoom,
    }))
}

pub async fn get_tile(
    State(state): State<AppState>,
    AxumPath((id, z, x, y)): AxumPath<(String, i32, i32, i32)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    validate_tile_coords(z, x, y)?;

    tracing::debug!(file_id = %id, z, x, y, "Tile request received");
    let conn = state.db.lock().await;

    let meta: TileFileMetadata = conn
        .query_row(
            "SELECT crs, crs_type, data_bounds, status, table_name, tile_format, path, tile_source FROM files WHERE id = ?",
            duckdb::params![id],
            |row| {
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

    let (crs, crs_type, data_bounds_json, status, table_name, tile_format, file_path, tile_source) =
        meta;

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

    let tile_source = tile_source.unwrap_or_else(|| "duckdb".to_string());
    if tile_source == TILE_SOURCE_POSTGIS {
        let source = fetch_postgis_source_config(&conn, &id)
            .map_err(|e| internal_error(format!("Failed to load PostGIS source: {e}")))?
            .ok_or_else(|| internal_error("PostGIS source metadata not found"))?;
        let properties = build_property_columns_for_query(&conn, &id)
            .map_err(|e| internal_error(format!("Failed to load PostGIS columns: {e}")))?;
        drop(conn);
        let mvt_blob = query_mvt_tile(&source, &properties, z, x, y, true)
            .await
            .map_err(|e| internal_error(format!("PostGIS tile generation failed: {e}")))?;
        return match mvt_blob {
            Some(blob) => Ok((
                [(header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile")],
                blob,
            )
                .into_response()),
            None => Ok(StatusCode::NO_CONTENT.into_response()),
        };
    }

    let table_name = table_name.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready for preview".to_string(),
            }),
        )
    })?;

    let tile_params = TileParams {
        source_crs: resolve_transform_source_crs(crs.as_deref()),
        crs_type: crs_type.clone().unwrap_or_else(|| "standard".to_string()),
        data_bounds: data_bounds_json
            .as_ref()
            .and_then(|j| DataBounds::from_json(j)),
    };

    let select_sql = build_mvt_select_sql(&conn, &id, &table_name, &tile_params, z, x, y, true)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Tile generation error: {}", e.0),
                }),
            )
        })?;

    let query_params = build_mvt_query_params(&tile_params, z, x, y);
    let params_slice: Vec<&dyn duckdb::ToSql> = query_params.iter().map(|p| p.as_ref()).collect();

    let mvt_blob: Option<Vec<u8>> =
        match conn.query_row(&select_sql, params_slice.as_slice(), |row| row.get(0)) {
            Ok(blob) => Some(blob),
            Err(e) => {
                tracing::error!(z, x, y, error = %e, sql = %select_sql, "Tile generation failed");
                return Err(internal_error(format!("Tile generation failed: {}", e)));
            }
        };

    tracing::debug!(
        z,
        x,
        y,
        size = mvt_blob.as_ref().map(|v| v.len()),
        "Tile request"
    );

    match mvt_blob {
        Some(blob) if !blob.is_empty() => Ok((
            [(header::CONTENT_TYPE, "application/vnd.mapbox-vector-tile")],
            blob,
        )
            .into_response()),
        _ => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

pub async fn get_feature_properties(
    State(state): State<AppState>,
    AxumPath((id, fid)): AxumPath<(String, i64)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (status, table_name, tile_format, tile_source): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT status, table_name, tile_format, tile_source FROM files WHERE id = ?",
            duckdb::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "File not found".to_string(),
                }),
            )
        })?;

    let tile_source = tile_source.unwrap_or_else(|| "duckdb".to_string());
    if tile_source == TILE_SOURCE_POSTGIS {
        if status != "ready" {
            return Err((
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "File is not ready for preview".to_string(),
                }),
            ));
        }

        let source = fetch_postgis_source_config(&conn, &id)
            .map_err(|e| internal_error(format!("Failed to load PostGIS source: {e}")))?
            .ok_or_else(|| internal_error("PostGIS source metadata not found"))?;
        let properties = build_property_columns_for_query(&conn, &id)
            .map_err(|e| internal_error(format!("Failed to load PostGIS columns: {e}")))?;
        drop(conn);

        let values = query_feature_properties_json(&source, &properties, fid)
            .await
            .map_err(|e| internal_error(format!("Failed to query PostGIS feature: {e}")))?;
        let Some(values) = values else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Feature not found".to_string(),
                }),
            ));
        };

        let properties = build_feature_properties(&properties, &values);
        return Ok(Json(FeaturePropertiesResponse { fid, properties }));
    }

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
            "SELECT normalized_name, original_name, alias\n         FROM dataset_columns\n         WHERE source_id = ?\n         ORDER BY ordinal",
        )
        .map_err(internal_error)?;

    let cols_iter = cols_stmt
        .query_map(duckdb::params![&id], |row| {
            let normalized: String = row.get(0)?;
            let original: String = row.get(1)?;
            let alias: Option<String> = row.get(2)?;
            Ok((normalized, original, alias))
        })
        .map_err(internal_error)?;

    let mut columns: Vec<(String, String, Option<String>)> = Vec::new();
    for c in cols_iter {
        columns.push(c.map_err(internal_error)?);
    }

    let mut select_exprs: Vec<String> = Vec::with_capacity(columns.len());
    for (normalized, _original, _alias) in &columns {
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
    for (index, (_normalized, original, alias)) in columns.iter().enumerate() {
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
            alias: alias.clone(),
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
                    tracing::warn!(file_id = %id, path = %full_path.display(), format = %format, error = %e, "Failed to extract MBTiles layers, returning empty");
                    return Ok(Json(crate::models::FileSchemaResponse { layers: vec![] }));
                }
            }
        } else {
            return Ok(Json(crate::models::FileSchemaResponse { layers: vec![] }));
        }
    }

    let mut cols_stmt = conn
        .prepare(
            "SELECT original_name, mvt_type, alias, normalized_name\n         FROM dataset_columns\n         WHERE source_id = ?\n         ORDER BY ordinal",
        )
        .map_err(internal_error)?;

    let cols_iter = cols_stmt
        .query_map(duckdb::params![&id], |row| {
            let original_name: String = row.get(0)?;
            let mvt_type: String = row.get(1)?;
            let alias: Option<String> = row.get(2)?;
            let normalized_name: Option<String> = row.get(3)?;
            Ok((original_name, mvt_type, alias, normalized_name))
        })
        .map_err(internal_error)?;

    let mut fields = Vec::new();
    for c in cols_iter {
        let (name, r#type, alias, normalized) = c.map_err(internal_error)?;
        fields.push(crate::models::FieldInfo {
            name,
            r#type,
            alias,
            normalized,
        });
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

fn validate_zoom_range(min: Option<i32>, max: Option<i32>) -> Result<(), String> {
    const MIN_ZOOM: i32 = 0;
    const MAX_ZOOM: i32 = 22;

    if let Some(z) = min {
        if !(MIN_ZOOM..=MAX_ZOOM).contains(&z) {
            return Err(format!(
                "minZoom must be between {} and {}",
                MIN_ZOOM, MAX_ZOOM
            ));
        }
    }
    if let Some(z) = max {
        if !(MIN_ZOOM..=MAX_ZOOM).contains(&z) {
            return Err(format!(
                "maxZoom must be between {} and {}",
                MIN_ZOOM, MAX_ZOOM
            ));
        }
    }
    if let (Some(mn), Some(mx)) = (min, max) {
        if mn > mx {
            return Err("minZoom must be less than or equal to maxZoom".to_string());
        }
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

    info!(file_id = %id, slug = %slug, "Publish request");

    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(internal_error)?;

    let (status, _name, tile_source, tile_format): (String, String, String, Option<String>) = conn
        .query_row(
            "SELECT status, name, COALESCE(tile_source, 'duckdb'), tile_format FROM files WHERE id = ?",
            duckdb::params![&id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
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

    let is_dynamic_data = tile_format.is_none();
    if !is_dynamic_data && (req.min_zoom.is_some() || req.max_zoom.is_some()) {
        conn.execute_batch("ROLLBACK").map_err(internal_error)?;
        drop(conn);
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error:
                    "Zoom levels can only be set for dynamic vector data (GeoJSON, Shapefile, etc.)"
                        .to_string(),
            }),
        ));
    }

    if is_dynamic_data {
        if let Err(e) = validate_zoom_range(req.min_zoom, req.max_zoom) {
            conn.execute_batch("ROLLBACK").map_err(internal_error)?;
            drop(conn);
            return Err(bad_request(&e));
        }
    }

    let insert_result = if is_dynamic_data {
        conn.execute(
            "INSERT INTO published_files (file_id, slug, tile_source, minzoom, maxzoom, use_aliases) VALUES (?, ?, ?, ?, ?, ?)",
            duckdb::params![&id, &slug, &tile_source, req.min_zoom, req.max_zoom, req.use_aliases],
        )
    } else {
        conn.execute(
            "INSERT INTO published_files (file_id, slug, tile_source, use_aliases) VALUES (?, ?, ?, ?)",
            duckdb::params![&id, &slug, &tile_source, req.use_aliases],
        )
    };

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
            info!(file_id = %id, slug = %slug, tile_source = %tile_source, "File published");
            let url = if tile_source == "pmtiles" {
                format!("/tiles/{slug}")
            } else {
                format!("/tiles/{slug}/{{z}}/{{x}}/{{y}}")
            };
            Ok(Json(crate::models::PublishResponse {
                url,
                slug,
                is_public: true,
                use_aliases: Some(req.use_aliases),
            }))
        }
        Err(err_msg) => {
            conn.execute_batch("ROLLBACK").map_err(internal_error)?;
            drop(conn);
            warn!(file_id = %id, error = %err_msg, "Publish failed");
            Err(bad_request(&err_msg))
        }
    }
}

pub async fn unpublish_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    info!(file_id = %id, "Unpublish request");
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
            info!(file_id = %id, "File unpublished");
            Ok(Json(serde_json::json!({ "message": "File unpublished" })))
        }
        Err(err_msg) => {
            conn.execute_batch("ROLLBACK").map_err(internal_error)?;
            drop(conn);
            warn!(file_id = %id, error = %err_msg, "Unpublish failed");
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
            "SELECT pf.slug, COALESCE(pf.tile_source, f.tile_source, 'duckdb')
             FROM published_files pf
             JOIN files f ON pf.file_id = f.id
             WHERE f.id = ? AND f.is_public = TRUE",
            duckdb::params![&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    drop(conn);

    match result {
        Some((slug, tile_source)) => Ok(Json(crate::models::PublicTileUrl {
            slug: slug.clone(),
            url: if tile_source == "pmtiles" {
                format!("/tiles/{slug}")
            } else {
                format!("/tiles/{slug}/{{z}}/{{x}}/{{y}}")
            },
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "File not published".to_string(),
            }),
        )),
    }
}

pub async fn update_crs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<crate::models::UpdateCrsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (status, old_crs): (String, Option<String>) = conn
        .query_row(
            "SELECT status, crs FROM files WHERE id = ?",
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
                error: "File is not ready".to_string(),
            }),
        ));
    }

    // Only update if crs is provided and different from current
    let new_crs = match req.crs {
        Some(crs) if crs.trim().is_empty() => None,
        Some(crs) => Some(crs.trim().to_string()),
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "crs field is required".to_string(),
                }),
            ));
        }
    };

    // Skip update if value unchanged
    if new_crs == old_crs {
        let current_crs_type: Option<String> = conn
            .query_row(
                "SELECT crs_type FROM files WHERE id = ?",
                duckdb::params![&id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        drop(conn);

        return Ok(Json(serde_json::json!({
            "id": id,
            "crs": old_crs,
            "crsType": current_crs_type.unwrap_or_else(|| "standard".to_string())
        })));
    }

    let normalized = normalize_crs(new_crs.as_deref());

    conn.execute(
        "UPDATE files SET crs = ?, crs_type = ? WHERE id = ?",
        duckdb::params![normalized.crs, normalized.crs_type, &id],
    )
    .map_err(internal_error)?;

    drop(conn);

    Ok(Json(serde_json::json!({
        "id": id,
        "crs": normalized.crs,
        "crsType": normalized.crs_type
    })))
}

pub async fn update_tile_zoom(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<UpdateZoomRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (tile_format, is_public): (Option<String>, bool) = conn
        .query_row(
            "SELECT tile_format, COALESCE(is_public, FALSE) FROM files WHERE id = ?",
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

    let is_dynamic_data = tile_format.is_none();
    if !is_dynamic_data {
        drop(conn);
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Zoom levels can only be updated for dynamic vector data (GeoJSON, Shapefile, etc.)".to_string(),
            }),
        ));
    }

    if !is_public {
        drop(conn);
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "File must be published before updating zoom levels".to_string(),
            }),
        ));
    }

    let (current_min, current_max): (Option<i32>, Option<i32>) = conn
        .query_row(
            "SELECT minzoom, maxzoom FROM published_files WHERE file_id = ?",
            duckdb::params![&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(internal_error)?;

    let effective_min = req.min_zoom.or(current_min);
    let effective_max = req.max_zoom.or(current_max);

    if let Err(e) = validate_zoom_range(effective_min, effective_max) {
        drop(conn);
        return Err(bad_request(&e));
    }

    let mut update_sql = "UPDATE published_files SET ".to_string();
    let mut params: Vec<Box<dyn duckdb::ToSql>> = Vec::new();
    let mut has_update = false;

    if let Some(min) = req.min_zoom {
        if has_update {
            update_sql.push_str(", ");
        }
        update_sql.push_str("minzoom = ?");
        params.push(Box::new(min));
        has_update = true;
    }
    if let Some(max) = req.max_zoom {
        if has_update {
            update_sql.push_str(", ");
        }
        update_sql.push_str("maxzoom = ?");
        params.push(Box::new(max));
        has_update = true;
    }

    if !has_update {
        drop(conn);
        return Ok(Json(serde_json::json!({
            "id": id,
            "minZoom": effective_min,
            "maxZoom": effective_max
        })));
    }

    update_sql.push_str(" WHERE file_id = ?");
    params.push(Box::new(id.clone()));

    let params_refs: Vec<&dyn duckdb::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&update_sql, params_refs.as_slice())
        .map_err(internal_error)?;

    drop(conn);

    Ok(Json(serde_json::json!({
        "id": id,
        "minZoom": effective_min,
        "maxZoom": effective_max
    })))
}

pub async fn update_field_aliases(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<crate::models::UpdateFieldAliasesRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let status: String = conn
        .query_row(
            "SELECT status FROM files WHERE id = ?",
            duckdb::params![&id],
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

    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(internal_error)?;

    for field in &req.fields {
        if let Some(ref alias) = field.alias {
            if alias.trim().is_empty() {
                conn.execute_batch("ROLLBACK").map_err(internal_error)?;
                drop(conn);
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Alias cannot be empty string".to_string(),
                    }),
                ));
            }
            if alias.len() > 255 {
                conn.execute_batch("ROLLBACK").map_err(internal_error)?;
                drop(conn);
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Alias must be 255 characters or less".to_string(),
                    }),
                ));
            }
        }

        let rows_affected = conn
            .execute(
                "UPDATE dataset_columns SET alias = ? WHERE source_id = ? AND normalized_name = ?",
                duckdb::params![&field.alias, &id, &field.normalized_name],
            )
            .map_err(internal_error)?;

        if rows_affected == 0 {
            conn.execute_batch("ROLLBACK").map_err(internal_error)?;
            drop(conn);
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Field '{}' not found", field.normalized_name),
                }),
            ));
        }
    }

    conn.execute_batch("COMMIT").map_err(internal_error)?;
    drop(conn);

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn update_publish_settings(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<crate::models::UpdatePublishSettingsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    // Check if file is published
    let is_public: bool = conn
        .query_row(
            "SELECT COALESCE(is_public, FALSE) FROM files WHERE id = ?",
            duckdb::params![&id],
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

    if !is_public {
        drop(conn);
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "File must be published before updating settings".to_string(),
            }),
        ));
    }

    // Update use_aliases if provided
    if let Some(use_aliases) = req.use_aliases {
        let rows_affected = conn
            .execute(
                "UPDATE published_files SET use_aliases = ? WHERE file_id = ?",
                duckdb::params![use_aliases, &id],
            )
            .map_err(internal_error)?;
        if rows_affected == 0 {
            drop(conn);
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Published file settings not found".to_string(),
                }),
            ));
        }
    }

    // Fetch current settings
    let use_aliases: bool = conn
        .query_row(
            "SELECT COALESCE(use_aliases, TRUE) FROM published_files WHERE file_id = ?",
            duckdb::params![&id],
            |row| row.get(0),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Published file settings not found".to_string(),
                }),
            )
        })?;

    drop(conn);

    Ok(Json(serde_json::json!({
        "id": id,
        "useAliases": use_aliases
    })))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsResponse {
    #[serde(rename = "maxSizeMb")]
    pub upload_max_size_mb: u64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    #[serde(rename = "maxSizeMb")]
    pub upload_max_size_mb: u64,
}

pub async fn get_settings(
    auth_session: AuthSession<crate::AuthBackend>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    if auth_session.user.is_none() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Not authenticated".to_string(),
            }),
        ));
    }

    let max_size = *state.max_size.read().await;
    let max_size_mb = max_size / BYTES_PER_MB;

    Ok(Json(SettingsResponse {
        upload_max_size_mb: max_size_mb,
    }))
}

pub async fn update_settings(
    auth_session: AuthSession<crate::AuthBackend>,
    State(state): State<AppState>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let user = auth_session.user.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Not authenticated".to_string(),
            }),
        )
    })?;

    if user.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Only administrators can update settings".to_string(),
            }),
        ));
    }

    if let Err(e) = validate_upload_size_mb(req.upload_max_size_mb) {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
    }

    let conn = state.db.lock().await;
    if let Err(e) = save_max_size_to_db(&conn, req.upload_max_size_mb) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to save settings: {}", e),
            }),
        ));
    }
    drop(conn);

    let new_bytes = req.upload_max_size_mb.saturating_mul(BYTES_PER_MB);
    let new_label = format_bytes(new_bytes);

    {
        let mut max_size = state.max_size.write().await;
        let mut max_size_label = state.max_size_label.write().await;
        *max_size = new_bytes;
        *max_size_label = new_label;
    }

    info!(
        upload_max_size_mb = req.upload_max_size_mb,
        updated_by = %user.username,
        "Upload size limit updated"
    );

    Ok(Json(SettingsResponse {
        upload_max_size_mb: req.upload_max_size_mb,
    }))
}
