use axum::{
    extract::{Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use std::path::PathBuf;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt},
};
use tracing::error;

use crate::{
    crs::{is_wgs84_compatible_crs, resolve_transform_source_crs, DataBounds},
    handlers::validate_tile_coords,
    http_errors::internal_error,
    mbtiles,
    models::{ErrorResponse, PublicTileMeta},
    tiles::{build_mvt_query_params, build_mvt_select_sql, TileParams},
    AppState,
};

struct PublicTileFileMeta {
    crs: Option<String>,
    crs_type: Option<String>,
    data_bounds_json: Option<String>,
    status: String,
    table_name: Option<String>,
    tile_format: Option<String>,
    file_path: String,
    minzoom: Option<i32>,
    maxzoom: Option<i32>,
    use_aliases: bool,
}

fn strip_dot_prefix(path: &str) -> &str {
    path.strip_prefix("./")
        .or_else(|| path.strip_prefix(".\\"))
        .unwrap_or(path)
}

fn candidate_uploaded_paths(state: &AppState, stored_path: &str) -> Vec<PathBuf> {
    let raw_path = PathBuf::from(stored_path);
    if raw_path.is_absolute() {
        return vec![raw_path];
    }

    let normalized_relative = PathBuf::from(strip_dot_prefix(stored_path));
    let mut candidates = vec![state.upload_dir.join(&normalized_relative)];
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(normalized_relative));
    }
    candidates
}

fn resolve_uploaded_file_canonical_path(
    state: &AppState,
    stored_path: &str,
    not_found_message: &str,
) -> Result<PathBuf, (StatusCode, Json<ErrorResponse>)> {
    let mut found_outside_upload_dir = false;

    for candidate in candidate_uploaded_paths(state, stored_path) {
        let Ok(canonical_path) = candidate.canonicalize() else {
            continue;
        };

        if canonical_path.starts_with(&state.upload_dir_canonical) {
            return Ok(canonical_path);
        }

        found_outside_upload_dir = true;
    }

    if found_outside_upload_dir {
        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Access denied".to_string(),
            }),
        ))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: not_found_message.to_string(),
            }),
        ))
    }
}

pub async fn get_public_tile(
    State(state): State<AppState>,
    AxumPath((slug, z, x, y)): AxumPath<(String, i32, i32, i32)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    validate_tile_coords(z, x, y)?;

    let conn = state.db.lock().await;

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

    let meta: PublicTileFileMeta = conn
        .query_row(
            "SELECT f.crs, f.crs_type, f.data_bounds, f.status, f.table_name, f.tile_format, f.path,
                    COALESCE(pf.minzoom, f.minzoom) as minzoom,
                    COALESCE(pf.maxzoom, f.maxzoom) as maxzoom,
                    COALESCE(pf.use_aliases, TRUE) as use_aliases
             FROM files f
             LEFT JOIN published_files pf ON f.id = pf.file_id
             WHERE f.id = ? AND f.is_public = TRUE",
            duckdb::params![&file_id],
            |row| Ok(PublicTileFileMeta {
                crs: row.get(0)?,
                crs_type: row.get(1)?,
                data_bounds_json: row.get(2)?,
                status: row.get(3)?,
                table_name: row.get(4)?,
                tile_format: row.get(5)?,
                file_path: row.get(6)?,
                minzoom: row.get(7)?,
                maxzoom: row.get(8)?,
                use_aliases: row.get(9)?,
            }),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "File not found".to_string(),
                }),
            )
        })?;

    if meta.status != "ready" {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready".to_string(),
            }),
        ));
    }

    if let Some(format) = meta.tile_format {
        let full_path = mbtiles::resolve_mbtiles_path(&meta.file_path);
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
                            (header::CACHE_CONTROL, "public, max-age=300"),
                        ],
                        data,
                    )
                        .into_response());
                } else {
                    return Ok((
                        [
                            (header::CONTENT_TYPE, ct),
                            (header::CACHE_CONTROL, "public, max-age=300"),
                        ],
                        data,
                    )
                        .into_response());
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

    if meta.minzoom.is_some_and(|min| z < min) || meta.maxzoom.is_some_and(|max| z > max) {
        drop(conn);
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let table_name = meta.table_name.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready".to_string(),
            }),
        )
    })?;

    let tile_params = TileParams {
        source_crs: resolve_transform_source_crs(meta.crs.as_deref()),
        crs_type: meta
            .crs_type
            .clone()
            .unwrap_or_else(|| "standard".to_string()),
        data_bounds: meta
            .data_bounds_json
            .as_ref()
            .and_then(|j| DataBounds::from_json(j)),
    };

    let select_sql = build_mvt_select_sql(
        &conn,
        &file_id,
        &table_name,
        &tile_params,
        z,
        x,
        y,
        meta.use_aliases,
    )
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
                error!(z, x, y, slug = %slug, error = %e, "Public tile generation failed");
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
        _ => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

pub async fn get_public_pmtiles(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (_file_id, file_path, tile_source): (String, String, String) = conn
        .query_row(
            "SELECT f.id, f.path, COALESCE(pf.tile_source, f.tile_source, 'duckdb')
             FROM files f
             JOIN published_files pf ON f.id = pf.file_id
             WHERE pf.slug = ? AND f.is_public = TRUE",
            duckdb::params![&slug],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Public tile not found".to_string(),
                }),
            )
        })?;

    if tile_source != "pmtiles" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "This endpoint is only for PMTiles files".to_string(),
            }),
        ));
    }

    let canonical_path =
        resolve_uploaded_file_canonical_path(&state, &file_path, "PMTiles file not found")?;

    let file = fs::File::open(&canonical_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to open file: {}", e),
            }),
        )
    })?;

    let metadata = file.metadata().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to read metadata: {}", e),
            }),
        )
    })?;
    let file_size = metadata.len();

    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header.to_str().map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid Range header".to_string(),
                }),
            )
        })?;

        if !range_str.starts_with("bytes=") {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid Range header format".to_string(),
                }),
            ));
        }

        let range_spec = &range_str[6..];
        let parts: Vec<&str> = range_spec.split('-').collect();
        if parts.len() != 2 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid Range header format".to_string(),
                }),
            ));
        }

        let start: u64 = if parts[0].is_empty() {
            0
        } else {
            parts[0].parse().map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Invalid Range start".to_string(),
                    }),
                )
            })?
        };

        let end: u64 = if parts[1].is_empty() {
            file_size - 1
        } else {
            parts[1].parse().map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Invalid Range end".to_string(),
                    }),
                )
            })?
        };

        if start > end || start >= file_size {
            return Err((
                StatusCode::RANGE_NOT_SATISFIABLE,
                Json(ErrorResponse {
                    error: "Range not satisfiable".to_string(),
                }),
            ));
        }

        let actual_end = end.min(file_size - 1);
        let content_length = actual_end - start + 1;

        let mut file = file;
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Seek failed: {}", e),
                    }),
                )
            })?;

        let mut buffer = vec![0u8; content_length as usize];
        file.read_exact(&mut buffer).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Read failed: {}", e),
                }),
            )
        })?;

        Ok((
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CONTENT_LENGTH, content_length.to_string().as_str()),
                (
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, actual_end, file_size).as_str(),
                ),
                (header::ACCEPT_RANGES, "bytes"),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            buffer,
        )
            .into_response())
    } else {
        let mut file = file;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Read failed: {}", e),
                }),
            )
        })?;

        Ok((
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CONTENT_LENGTH, file_size.to_string().as_str()),
                (header::ACCEPT_RANGES, "bytes"),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            buffer,
        )
            .into_response())
    }
}

pub async fn head_public_pmtiles(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (file_path, tile_source): (String, String) = conn
        .query_row(
            "SELECT f.path, COALESCE(pf.tile_source, f.tile_source, 'duckdb')
             FROM files f
             JOIN published_files pf ON f.id = pf.file_id
             WHERE pf.slug = ? AND f.is_public = TRUE",
            duckdb::params![&slug],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Public tile not found".to_string(),
                }),
            )
        })?;

    if tile_source != "pmtiles" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "This endpoint is only for PMTiles files".to_string(),
            }),
        ));
    }

    let canonical_path =
        resolve_uploaded_file_canonical_path(&state, &file_path, "PMTiles file not found")?;

    let metadata = fs::metadata(&canonical_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to read metadata: {}", e),
            }),
        )
    })?;
    let file_size = metadata.len();

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CONTENT_LENGTH, file_size.to_string().as_str()),
            (header::ACCEPT_RANGES, "bytes"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
    )
        .into_response())
}

struct PublicMetaRow {
    name: String,
    tile_source: String,
    crs: Option<String>,
    crs_type: Option<String>,
    data_bounds_json: Option<String>,
    table_name: Option<String>,
    tile_format: Option<String>,
    tile_bounds_json: Option<String>,
    minzoom: Option<i32>,
    maxzoom: Option<i32>,
}

pub async fn get_public_tile_meta(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let row: PublicMetaRow = conn
        .query_row(
            "SELECT f.name, COALESCE(pf.tile_source, f.tile_source, 'duckdb'),
                    f.crs, f.crs_type, f.data_bounds, f.table_name, f.tile_format, f.tile_bounds,
                    COALESCE(pf.minzoom, f.minzoom) as minzoom,
                    COALESCE(pf.maxzoom, f.maxzoom) as maxzoom
             FROM files f
             JOIN published_files pf ON f.id = pf.file_id
             WHERE pf.slug = ? AND f.is_public = TRUE",
            duckdb::params![&slug],
            |row| {
                Ok(PublicMetaRow {
                    name: row.get(0)?,
                    tile_source: row.get(1)?,
                    crs: row.get(2)?,
                    crs_type: row.get(3)?,
                    data_bounds_json: row.get(4)?,
                    table_name: row.get(5)?,
                    tile_format: row.get(6)?,
                    tile_bounds_json: row.get(7)?,
                    minzoom: row.get(8)?,
                    maxzoom: row.get(9)?,
                })
            },
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Public tile not found".to_string(),
                }),
            )
        })?;

    let tile_url = if row.tile_source == "pmtiles" {
        format!("/tiles/{}", slug)
    } else {
        format!("/tiles/{}/{{z}}/{{x}}/{{y}}", slug)
    };
    let viewer_url = format!("/tiles/{}", slug);

    let crs_type = row.crs_type.unwrap_or_else(|| "standard".to_string());
    let data_bounds: Option<DataBounds> = row
        .data_bounds_json
        .as_ref()
        .and_then(|j| DataBounds::from_json(j));
    let data_bounds_array = data_bounds.as_ref().map(|b| b.to_array());

    let bbox_values = if let Some(bounds_json) = row.tile_bounds_json {
        serde_json::from_str::<[f64; 4]>(&bounds_json).ok()
    } else {
        None
    }
    .or_else(|| {
        if crs_type == "custom" {
            return data_bounds_array;
        }

        if let Some(tbl) = row.table_name {
            let transform_source_crs = resolve_transform_source_crs(row.crs.as_deref());
            let bbox_components_query = format!(
                "SELECT ST_XMin(b), ST_YMin(b), ST_XMax(b), ST_YMax(b) FROM (
                    SELECT ST_Extent(ST_Transform(geom, '{}', 'EPSG:4326', always_xy := true)) as b
                    FROM \"{tbl}\"
                )",
                transform_source_crs
            );

            let transformed_bbox = conn
                .query_row(&bbox_components_query, [], |bbox_row| {
                    let minx: Option<f64> = bbox_row.get(0).ok();
                    let miny: Option<f64> = bbox_row.get(1).ok();
                    let maxx: Option<f64> = bbox_row.get(2).ok();
                    let maxy: Option<f64> = bbox_row.get(3).ok();

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

        if is_wgs84_compatible_crs(row.crs.as_deref()) {
            return data_bounds_array;
        }

        None
    });

    Ok(Json(PublicTileMeta {
        slug: slug.clone(),
        name: row.name,
        tile_source: row.tile_source,
        tile_url,
        viewer_url,
        crs: row.crs,
        crs_type: crs_type.clone(),
        bbox: bbox_values,
        data_bounds: data_bounds_array,
        tile_format: row.tile_format,
        minzoom: row.minzoom,
        maxzoom: row.maxzoom,
    }))
}
