use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use axum_login::AuthSession;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, info_span, warn, Instrument};

use crate::{
    config::BYTES_PER_MB, handlers::get_workspace_id, import::import_spatial_data, mbtiles,
    models::ErrorResponse, AppState,
};

const MAX_IMPORT_FILES: usize = 20;
const MAX_FILE_SIZE_MB: u64 = 1024;

type ApiResult<T> = Result<T, Response>;

fn err(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
        .into_response()
}

fn bad_req(message: &str) -> Response {
    err(StatusCode::BAD_REQUEST, message)
}

fn forbidden(message: &str) -> Response {
    err(StatusCode::FORBIDDEN, message)
}

fn not_found(message: &str) -> Response {
    err(StatusCode::NOT_FOUND, message)
}

fn internal_err<E: std::fmt::Debug>(e: E) -> Response {
    tracing::error!(error = ?e, "Internal server error");
    err(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
}

/// Get allowed data directories from environment variable
fn get_allowed_directories() -> Vec<PathBuf> {
    std::env::var("SERVER_DATA_DIRS")
        .unwrap_or_else(|_| "./data".to_string())
        .split(',')
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.exists() && p.is_dir())
        .collect()
}

/// Get canonicalized allowed directories (cached for request lifetime)
fn get_allowed_directories_canonical() -> Vec<PathBuf> {
    get_allowed_directories()
        .into_iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect()
}

/// Check if a canonical path is within allowed directories
fn is_canonical_path_allowed(canonical: &PathBuf, allowed_dirs: &[PathBuf]) -> bool {
    allowed_dirs
        .iter()
        .any(|allowed| canonical.starts_with(allowed))
}

/// Validate file extension is supported
fn is_supported_extension(ext: &str) -> bool {
    let ext_lower = ext.to_lowercase();
    matches!(
        ext_lower.as_str(),
        "zip"
            | "geojson"
            | "json"
            | "geojsonl"
            | "geojsons"
            | "kml"
            | "gpx"
            | "topojson"
            | "mbtiles"
            | "pmtiles"
    )
}

#[derive(Debug, Serialize)]
pub struct DirectoryInfo {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct DirectoriesResponse {
    pub directories: Vec<DirectoryInfo>,
}

/// GET /api/server-files/directories
/// Returns list of allowed directories for server file import
pub async fn list_directories(
    _auth_session: AuthSession<crate::AuthBackend>,
) -> ApiResult<impl IntoResponse> {
    let dirs = get_allowed_directories();

    let directories: Vec<DirectoryInfo> = dirs
        .into_iter()
        .filter_map(|p| {
            let canonical = p.canonicalize().ok()?;
            let name = canonical.file_name()?.to_string_lossy().to_string();
            let path = canonical.to_string_lossy().to_string();
            Some(DirectoryInfo { path, name })
        })
        .collect();

    Ok(Json(DirectoriesResponse { directories }))
}

#[derive(Debug, Serialize)]
pub struct BrowseItem {
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BrowseResponse {
    #[serde(rename = "currentPath")]
    pub current_path: String,
    #[serde(rename = "parentPath", skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
    pub items: Vec<BrowseItem>,
}

#[derive(Debug, Deserialize)]
pub struct BrowseQuery {
    pub path: Option<String>,
}

/// GET /api/server-files/browse?path=...
/// Browse directory contents
pub async fn browse_directory(
    _auth_session: AuthSession<crate::AuthBackend>,
    Query(query): Query<BrowseQuery>,
) -> ApiResult<impl IntoResponse> {
    let allowed_dirs = get_allowed_directories_canonical();
    if allowed_dirs.is_empty() {
        return Err(bad_req("No data directories configured"));
    }

    // Resolve path: canonicalize first, then check if allowed
    let canonical = match query.path {
        Some(ref p) if !p.is_empty() => {
            let path = PathBuf::from(p);
            path.canonicalize()
                .map_err(|_| not_found("Directory not found"))?
        }
        _ => allowed_dirs.first().cloned().unwrap(),
    };

    // Security: verify canonical path is within allowed directories
    if !is_canonical_path_allowed(&canonical, &allowed_dirs) {
        return Err(forbidden("Access denied: path outside allowed directories"));
    }

    if !canonical.is_dir() {
        return Err(bad_req("Not a directory"));
    }

    let read_dir = std::fs::read_dir(&canonical).map_err(internal_err)?;

    let mut items: Vec<BrowseItem> = Vec::new();
    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = match entry.file_name().to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip hidden files
        if name.starts_with('.') {
            continue;
        }

        let full_path = canonical.join(&name);

        let entry_canonical = match full_path.canonicalize() {
            Ok(c) => c,
            Err(_) => continue,
        };

        if !is_canonical_path_allowed(&entry_canonical, &allowed_dirs) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if metadata.is_dir() {
            items.push(BrowseItem {
                name,
                item_type: "directory".to_string(),
                size: None,
                ext: None,
            });
        } else if metadata.is_file() {
            let ext = full_path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()));

            if let Some(ref e) = ext {
                let ext_without_dot = e.trim_start_matches('.');
                if !is_supported_extension(ext_without_dot) {
                    continue;
                }
            } else {
                continue;
            }

            items.push(BrowseItem {
                name,
                item_type: "file".to_string(),
                size: Some(metadata.len()),
                ext,
            });
        }
    }

    // Sort: directories first, then files, both alphabetically
    items.sort_by(|a, b| match (a.item_type.as_str(), b.item_type.as_str()) {
        ("directory", "file") => std::cmp::Ordering::Less,
        ("file", "directory") => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    // Calculate parent path if within allowed directories
    let parent_path = canonical.parent().and_then(|p| {
        if is_canonical_path_allowed(&p.to_path_buf(), &allowed_dirs) {
            Some(p.to_string_lossy().to_string())
        } else {
            None
        }
    });

    Ok(Json(BrowseResponse {
        current_path: canonical.to_string_lossy().to_string(),
        parent_path,
        items,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ImportFile {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub files: Vec<ImportFile>,
}

#[derive(Debug, Serialize)]
pub struct ImportedFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct FailedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub imported: Vec<ImportedFile>,
    pub failed: Vec<FailedFile>,
}

/// POST /api/server-files/import
/// Import files from server (reference mode - no copy)
pub async fn import_files(
    auth_session: AuthSession<crate::AuthBackend>,
    State(state): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> ApiResult<impl IntoResponse> {
    let workspace_id = get_workspace_id(&auth_session, &state)
        .await
        .map_err(|(status, json)| err(status, &json.error))?;

    if req.files.is_empty() {
        return Err(bad_req("No files specified"));
    }

    if req.files.len() > MAX_IMPORT_FILES {
        return Err(bad_req(&format!(
            "Maximum {} files per import",
            MAX_IMPORT_FILES
        )));
    }

    let allowed_dirs = get_allowed_directories_canonical();
    let max_size_bytes = *state.max_size.read().await;
    let mut imported = Vec::new();
    let mut failed = Vec::new();
    let mut files_to_process: Vec<(String, String, PathBuf)> = Vec::new();

    {
        let conn = state.db.lock().await;

        for file in &req.files {
            let path = PathBuf::from(&file.path);

            let canonical = match path.canonicalize() {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "Import failed: file not found");
                    failed.push(FailedFile {
                        path: file.path.clone(),
                        reason: "File not found".to_string(),
                    });
                    continue;
                }
            };

            if !is_canonical_path_allowed(&canonical, &allowed_dirs) {
                warn!("Import blocked: path outside allowed directories");
                failed.push(FailedFile {
                    path: file.path.clone(),
                    reason: "Path outside allowed directories".to_string(),
                });
                continue;
            }

            if !canonical.is_file() {
                warn!("Import failed: not a file");
                failed.push(FailedFile {
                    path: file.path.clone(),
                    reason: "Not a file".to_string(),
                });
                continue;
            }

            let ext = canonical
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase());

            if let Some(ref e) = ext {
                if !is_supported_extension(e) {
                    warn!(ext = %e, "Import failed: unsupported file type");
                    failed.push(FailedFile {
                        path: file.path.clone(),
                        reason: format!("Unsupported file type: {}", e),
                    });
                    continue;
                }
            } else {
                failed.push(FailedFile {
                    path: file.path.clone(),
                    reason: "File has no extension".to_string(),
                });
                continue;
            }

            let metadata = match std::fs::metadata(&canonical) {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, "Import failed: cannot read metadata");
                    failed.push(FailedFile {
                        path: file.path.clone(),
                        reason: "Cannot read file metadata".to_string(),
                    });
                    continue;
                }
            };

            let file_size = metadata.len();
            if file_size > max_size_bytes {
                let max_mb = max_size_bytes / BYTES_PER_MB;
                let file_mb = file_size / BYTES_PER_MB;
                warn!(
                    file_mb = file_mb,
                    max_mb = max_mb,
                    "Import failed: file too large"
                );
                failed.push(FailedFile {
                    path: file.path.clone(),
                    reason: format!("File too large ({}MB > {}MB limit)", file_mb, max_mb),
                });
                continue;
            }

            let file_name = canonical
                .file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "imported".to_string());

            let file_type = ext.unwrap_or_else(|| "unknown".to_string());
            let file_path_str = canonical.to_string_lossy().to_string();

            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM files WHERE path = ? AND workspace_id = ?",
                    duckdb::params![&file_path_str, &workspace_id],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            if let Some(existing_id) = existing {
                warn!(existing_id = %existing_id, "Import skipped: file already imported");
                failed.push(FailedFile {
                    path: file.path.clone(),
                    reason: "File already imported".to_string(),
                });
                continue;
            }

            let file_id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let file_size_i64 = file_size as i64;

            let insert_result = conn.execute(
                r#"
                INSERT INTO files (id, name, type, size, uploaded_at, status, path, workspace_id, source_type)
                VALUES (?, ?, ?, ?, ?, 'uploaded', ?, ?, 'server_import')
                "#,
                duckdb::params![
                    &file_id,
                    &file_name,
                    &file_type,
                    file_size_i64,
                    &now,
                    &file_path_str,
                    &workspace_id
                ],
            );

            match insert_result {
                Ok(_) => {
                    info!(file_id = %file_id, workspace_id = %workspace_id, "Server file imported");
                    imported.push(ImportedFile {
                        id: file_id.clone(),
                        name: file_name.clone(),
                        path: file_path_str.clone(),
                        status: "uploaded".to_string(),
                    });
                    files_to_process.push((file_id, file_type, canonical));
                }
                Err(e) => {
                    warn!(error = %e, "Import failed: database error");
                    failed.push(FailedFile {
                        path: file.path.clone(),
                        reason: "Database error".to_string(),
                    });
                }
            }
        }
    }

    for (file_id, file_type, file_path) in files_to_process {
        let db = state.db.clone();
        let span = info_span!("server_import", file_id = %file_id, file_type = %file_type);

        tokio::spawn(
            async move {
                tracing::info!("Starting server file import processing");
                {
                    let conn = db.lock().await;
                    if let Err(e) = conn.execute(
                        "UPDATE files SET status = 'processing' WHERE id = ?",
                        duckdb::params![&file_id],
                    ) {
                        tracing::error!(error = %e, file_id = %file_id, "Failed to update status to processing");
                        return;
                    }
                }

                let result = match file_type.as_str() {
                    "mbtiles" => mbtiles::import_mbtiles(&db, &file_id, &file_path).await,
                    "pmtiles" => {
                        let conn = db.lock().await;
                        match conn.execute(
                            "UPDATE files SET status = 'ready', tile_source = 'pmtiles' WHERE id = ?",
                            duckdb::params![&file_id],
                        ) {
                            Ok(_) => Ok(()),
                            Err(e) => {
                                tracing::error!(error = %e, file_id = %file_id, "Failed to update pmtiles status");
                                Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
                            }
                        }
                    }
                    _ => import_spatial_data(&db, &file_id, &file_path).await,
                };

                match result {
                    Ok(_) => {
                        tracing::info!(file_id = %file_id, "Server file import completed successfully");
                        let conn = db.lock().await;
                        if let Err(e) = conn.execute(
                            "UPDATE files SET status = 'ready' WHERE id = ?",
                            duckdb::params![&file_id],
                        ) {
                            tracing::error!(error = %e, file_id = %file_id, "Failed to update status to ready");
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, file_id = %file_id, "Server file import failed");
                        let conn = db.lock().await;
                        if let Err(update_err) = conn.execute(
                            "UPDATE files SET status = 'failed', error = ? WHERE id = ?",
                            duckdb::params![e.to_string(), &file_id],
                        ) {
                            tracing::error!(error = %update_err, file_id = %file_id, "Failed to update status to failed");
                        }
                    }
                }
            }
            .instrument(span),
        );
    }

    if imported.is_empty() {
        return Err(bad_req("No files could be imported"));
    }

    Ok(Json(ImportResponse { imported, failed }))
}
