use axum::{
    extract::{Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt},
};
use tracing::error;

use crate::{
    crs::DataBounds,
    handlers::{validate_tile_coords, TileFileMetadata},
    http_errors::internal_error,
    mbtiles,
    models::{ErrorResponse, PublicTileMeta},
    tiles::{build_mvt_query_params, build_mvt_select_sql, TileParams},
    AppState,
};

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

    let meta: TileFileMetadata = conn
        .query_row(
            "SELECT crs, crs_type, data_bounds, status, table_name, tile_format, path FROM files WHERE id = ? AND is_public = TRUE",
            duckdb::params![&file_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "File not found".to_string(),
                }),
            )
        })?;

    let (crs, crs_type, data_bounds_json, status, table_name, tile_format, file_path) = meta;

    if status != "ready" {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "File is not ready".to_string(),
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
                return Ok((
                    [
                        (header::CONTENT_TYPE, ct),
                        (header::CACHE_CONTROL, "public, max-age=300"),
                    ],
                    data,
                )
                    .into_response());
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
                error: "File is not ready".to_string(),
            }),
        )
    })?;

    let tile_params = TileParams {
        source_crs: crs.clone().unwrap_or_else(|| "EPSG:4326".to_string()),
        crs_type: crs_type.clone().unwrap_or_else(|| "standard".to_string()),
        data_bounds: data_bounds_json
            .as_ref()
            .and_then(|j| DataBounds::from_json(j)),
    };

    let select_sql = build_mvt_select_sql(&conn, &file_id, &table_name, &tile_params, z, x, y)
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

    let file_path = state
        .upload_dir
        .join(file_path.strip_prefix("./").unwrap_or(&file_path));
    let canonical_path = file_path.canonicalize().map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "PMTiles file not found".to_string(),
            }),
        )
    })?;

    if !canonical_path.starts_with(&state.upload_dir_canonical) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Access denied".to_string(),
            }),
        ));
    }

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

    let file_path = state
        .upload_dir
        .join(file_path.strip_prefix("./").unwrap_or(&file_path));
    let canonical_path = file_path.canonicalize().map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "PMTiles file not found".to_string(),
            }),
        )
    })?;

    if !canonical_path.starts_with(&state.upload_dir_canonical) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Access denied".to_string(),
            }),
        ));
    }

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

pub async fn get_public_tile_meta(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let (name, tile_source): (String, String) = conn
        .query_row(
            "SELECT f.name, COALESCE(pf.tile_source, f.tile_source, 'duckdb')
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

    let tile_url = if tile_source == "pmtiles" {
        format!("/tiles/{}", slug)
    } else {
        format!("/tiles/{}/{{z}}/{{x}}/{{y}}", slug)
    };
    let viewer_url = format!("/tiles/{}", slug);

    Ok(Json(PublicTileMeta {
        slug: slug.clone(),
        name,
        tile_source,
        tile_url,
        viewer_url,
    }))
}
