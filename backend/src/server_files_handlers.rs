use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use axum_login::AuthSession;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

use crate::{
    handlers::get_workspace_id,
    http_errors::internal_error,
    models::ErrorResponse,
    AppState,
};

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

/// Check if a path is within allowed directories
fn is_path_allowed(path: &PathBuf) -> bool {
    let canonical = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };

    let allowed_dirs = get_allowed_directories();
    for allowed in allowed_dirs {
        if let Ok(allowed_canonical) = allowed.canonicalize() {
            if canonical.starts_with(&allowed_canonical) {
                return true;
            }
        }
    }
    false
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
            let name = p.file_name()?.to_string_lossy().to_string();
            let path = p.to_string_lossy().to_string();
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
    let base_dirs = get_allowed_directories();
    if base_dirs.is_empty() {
        return Err(bad_req("No data directories configured"));
    }

    // Default to first allowed directory
    let target_path = match query.path {
        Some(ref p) if !p.is_empty() => {
            let path = PathBuf::from(p);
            if !is_path_allowed(&path) {
                return Err(forbidden("Access denied: path outside allowed directories"));
            }
            path
        }
        _ => base_dirs.into_iter().next().unwrap(),
    };

    // Security check for path traversal
    let canonical = target_path.canonicalize().map_err(|_| {
        not_found("Directory not found")
    })?;

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

        let path = entry.path();
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        // Skip hidden files
        if name.starts_with('.') {
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
            let ext = path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()));
            
            // Only show supported file types
            if let Some(ref e) = ext {
                let ext_without_dot = e.trim_start_matches('.');
                if !is_supported_extension(ext_without_dot) {
                    continue;
                }
            } else {
                continue; // Skip files without extension
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
    items.sort_by(|a, b| {
        match (a.item_type.as_str(), b.item_type.as_str()) {
            ("directory", "file") => std::cmp::Ordering::Less,
            ("file", "directory") => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    // Calculate parent path if within allowed directories
    let parent_path = canonical.parent().and_then(|p| {
        if is_path_allowed(&p.to_path_buf()) {
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
pub struct ImportResponse {
    pub imported: Vec<ImportedFile>,
}

/// POST /api/server-files/import
/// Import files from server (reference mode - no copy)
pub async fn import_files(
    auth_session: AuthSession<crate::AuthBackend>,
    State(state): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> ApiResult<impl IntoResponse> {
    let workspace_id = get_workspace_id(&auth_session, &state).await?;

    if req.files.is_empty() {
        return Err(bad_req("No files specified"));
    }

    if req.files.len() > 50 {
        return Err(bad_req("Maximum 50 files per import"));
    }

    let mut imported = Vec::new();
    let conn = state.db.lock().await;

    for file in &req.files {
        let path = PathBuf::from(&file.path);

        // Security checks
        if !is_path_allowed(&path) {
            warn!(path = %file.path, "Import blocked: path outside allowed directories");
            continue;
        }

        // Check file exists and is readable
        let canonical = match path.canonicalize() {
            Ok(c) => c,
            Err(e) => {
                warn!(path = %file.path, error = %e, "Import failed: file not found");
                continue;
            }
        };

        if !canonical.is_file() {
            warn!(path = %file.path, "Import failed: not a file");
            continue;
        }

        // Validate extension
        let ext = canonical
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        
        if let Some(ref e) = ext {
            if !is_supported_extension(e) {
                warn!(path = %file.path, ext = %e, "Import failed: unsupported file type");
                continue;
            }
        } else {
            continue;
        }

        // Get file metadata
        let metadata = match std::fs::metadata(&canonical) {
            Ok(m) => m,
            Err(e) => {
                warn!(path = %file.path, error = %e, "Import failed: cannot read metadata");
                continue;
            }
        };

        let file_name = canonical
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "imported".to_string());

        let file_size = metadata.len() as i64;
        let file_type = ext.unwrap_or_else(|| "unknown".to_string());
        let file_path_str = canonical.to_string_lossy().to_string();
        let file_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // Insert into database with source_type = 'server_import'
        let insert_result = conn.execute(
            r#"
            INSERT INTO files (id, name, type, size, uploaded_at, status, path, workspace_id, source_type)
            VALUES (?, ?, ?, ?, ?, 'uploaded', ?, ?, 'server_import')
            "#,
            duckdb::params![
                &file_id,
                &file_name,
                &file_type,
                file_size,
                &now,
                &file_path_str,
                &workspace_id
            ],
        );

        match insert_result {
            Ok(_) => {
                info!(file_id = %file_id, path = %file_path_str, workspace_id = %workspace_id, "Server file imported");
                imported.push(ImportedFile {
                    id: file_id,
                    name: file_name,
                    path: file_path_str,
                    status: "uploaded".to_string(),
                });
            }
            Err(e) => {
                warn!(path = %file.path, error = %e, "Import failed: database error");
            }
        }
    }

    drop(conn);

    if imported.is_empty() {
        return Err(bad_req("No files could be imported"));
    }

    Ok(Json(ImportResponse { imported }))
}
