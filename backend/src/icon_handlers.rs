use axum::{
    extract::{Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use axum_login::AuthSession;
use duckdb::OptionalExt;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{error, info, warn};

use crate::{
    http_errors::{bad_request, internal_error},
    models::ErrorResponse,
    storage::{create_id, relative_path_for, resolve_stored_path},
    workspace::get_active_workspace_id,
    AppState, AuthBackend,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconItem {
    pub id: String,
    pub name: String,
    pub file_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub size: i64,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IconUploadResponse {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIconRequest {
    pub name: Option<String>,
}

fn read_image_dimensions(
    path: &Path,
    file_type: &str,
) -> Result<(u32, u32), Box<dyn std::error::Error + Send + Sync>> {
    match file_type {
        "png" => {
            let decoder = image::codecs::png::PngDecoder::new(std::io::BufReader::new(
                std::fs::File::open(path)?,
            ))?;
            let (w, h) = image::ImageDecoder::dimensions(&decoder);
            Ok((w, h))
        }
        "svg" => {
            let data = std::fs::read(path)?;
            let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default())?;
            let size = tree.size();
            let w = size.width().round() as u32;
            let h = size.height().round() as u32;
            if w == 0 || h == 0 {
                return Err("SVG has no usable dimensions".into());
            }
            Ok((w, h))
        }
        _ => Err(format!("Unsupported file type: {file_type}").into()),
    }
}

pub async fn upload_icon(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

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
        .map(|ext| ext.to_lowercase())
        .ok_or_else(|| bad_request("Unsupported file type. Use .png or .svg"))?;

    if !["png", "svg"].contains(&ext.as_str()) {
        return Err(bad_request("Unsupported file type. Use .png or .svg"));
    }

    let icon_id = create_id();
    info!(icon_id = %icon_id, filename = %safe_name, "Icon upload started");

    let icon_dir = state.upload_dir.join("icons").join(&icon_id);
    fs::create_dir_all(&icon_dir)
        .await
        .map_err(internal_error)?;

    let original_path = icon_dir.join(format!("original.{}", ext));
    let mut file = BufWriter::new(
        fs::File::create(&original_path)
            .await
            .map_err(internal_error)?,
    );

    let mut size: u64 = 0;
    let max_size = *state.max_size.read().await;
    let max_size_label = state.max_size_label.read().await.clone();
    while let Some(chunk) = field.chunk().await.map_err(internal_error)? {
        size = size.saturating_add(chunk.len() as u64);
        if size > max_size {
            drop(file);
            let _ = fs::remove_file(&original_path).await;
            let _ = fs::remove_dir(&icon_dir).await;
            let message = format!("File too large (max {})", max_size_label);
            return Err(bad_request(&message));
        }
        file.write_all(&chunk).await.map_err(internal_error)?;
    }
    file.flush().await.map_err(internal_error)?;
    file.get_ref().sync_all().await.map_err(internal_error)?;
    drop(file);

    let original_rel = relative_path_for(&original_path, &state.upload_dir);

    let display_name = Path::new(&safe_name)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or(&safe_name)
        .to_string();

    let original_path_clone = original_path.clone();
    let ext_clone = ext.clone();
    let dimensions = tokio::task::spawn_blocking(move || {
        read_image_dimensions(&original_path_clone, &ext_clone)
    })
    .await;

    let (width, height) = match dimensions {
        Ok(Ok((w, h))) => (Some(w as i32), Some(h as i32)),
        Ok(Err(e)) => {
            error!(icon_id = %icon_id, error = %e, "Failed to read image dimensions");
            (None, None)
        }
        Err(e) => {
            error!(icon_id = %icon_id, error = %e, "Failed to spawn blocking task");
            (None, None)
        }
    };

    let conn = state.db.lock().await;
    let insert_result: Result<(), duckdb::Error> = conn
        .execute(
            "INSERT INTO icons (id, workspace_id, name, original_path, file_type, width, height, size, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'ready', CURRENT_TIMESTAMP)",
            duckdb::params![
                &icon_id,
                &workspace_id,
                &display_name,
                &original_rel,
                &ext,
                width,
                height,
                size as i64,
            ],
        )
        .map(|_| ());
    drop(conn);

    if let Err(e) = insert_result {
        let _ = fs::remove_dir_all(&icon_dir).await;
        return Err(internal_error(e));
    }

    info!(icon_id = %icon_id, name = %display_name, "Icon uploaded");

    Ok((
        StatusCode::CREATED,
        Json(IconUploadResponse {
            id: icon_id,
            status: "ready".to_string(),
        }),
    ))
}

fn read_icon_row(row: &duckdb::Row) -> Result<IconItem, duckdb::Error> {
    let created_at: chrono::NaiveDateTime = row.get(8)?;
    let updated_at: Option<chrono::NaiveDateTime> = row.get(9)?;
    Ok(IconItem {
        id: row.get(0)?,
        name: row.get(1)?,
        file_type: row.get(2)?,
        width: row.get(3)?,
        height: row.get(4)?,
        size: row.get(5)?,
        status: row.get(6)?,
        error: row.get(7)?,
        created_at: created_at.and_utc().to_rfc3339(),
        updated_at: updated_at.map(|t| t.and_utc().to_rfc3339()),
    })
}

pub async fn list_icons(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, file_type, width, height, size, status, error, created_at, updated_at
             FROM icons
             WHERE workspace_id = ?
             ORDER BY created_at DESC",
        )
        .map_err(internal_error)?;

    let icons_iter = stmt
        .query_map(duckdb::params![&workspace_id], read_icon_row)
        .map_err(internal_error)?;

    let mut icons = Vec::new();
    for icon in icons_iter {
        icons.push(icon.map_err(internal_error)?);
    }

    drop(conn);
    Ok(Json(icons))
}

pub async fn update_icon(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<UpdateIconRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let name = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad_request("Name cannot be empty"))?;

    let conn = state.db.lock().await;
    let rows_affected = conn
        .execute(
            "UPDATE icons SET name = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND workspace_id = ?",
            duckdb::params![name, &id, &workspace_id],
        )
        .map_err(internal_error)?;
    drop(conn);

    if rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Icon not found".to_string(),
            }),
        ));
    }

    info!(icon_id = %id, name = %name, "Icon updated");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_icon(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;

    let original_path: Option<String> = conn
        .query_row(
            "SELECT original_path FROM icons WHERE id = ? AND workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal_error)?;

    let Some(original_path) = original_path else {
        drop(conn);
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Icon not found".to_string(),
            }),
        ));
    };

    conn.execute(
        "DELETE FROM icons WHERE id = ? AND workspace_id = ?",
        duckdb::params![&id, &workspace_id],
    )
    .map_err(internal_error)?;

    drop(conn);

    let icon_file_dir = resolve_stored_path(&original_path, &state.upload_dir);
    let icon_dir = icon_file_dir
        .parent()
        .map_or(icon_file_dir.clone(), std::path::Path::to_path_buf);

    match tokio::fs::canonicalize(&icon_dir).await {
        Ok(canonical_icon_dir) if canonical_icon_dir.starts_with(&state.upload_dir_canonical) => {
            if let Err(e) = tokio::fs::remove_dir_all(&canonical_icon_dir).await {
                warn!(icon_dir = %canonical_icon_dir.display(), error = %e, "Failed to remove icon directory");
            }
        }
        Ok(canonical_icon_dir) => {
            warn!(
                icon_dir = %canonical_icon_dir.display(),
                "Skipping icon directory removal: path escapes upload directory"
            );
        }
        Err(e) => {
            warn!(icon_dir = %icon_dir.display(), error = %e, "Icon directory not found on disk, skipping removal");
        }
    }

    info!(icon_id = %id, "Icon deleted");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_icon_file(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;
    let result: Option<(String, String)> = conn
        .query_row(
            "SELECT original_path, file_type FROM icons WHERE id = ? AND workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(internal_error)?;
    drop(conn);

    let Some((original_path, file_type)) = result else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Icon not found".to_string(),
            }),
        ));
    };

    let file_path = resolve_stored_path(&original_path, &state.upload_dir);

    let canonical_path = match fs::canonicalize(&file_path).await {
        Ok(path) => path,
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Icon file not found".to_string(),
                }),
            ))
        }
    };

    if !canonical_path.starts_with(&state.upload_dir_canonical) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Access denied".to_string(),
            }),
        ));
    }

    let content_type = match file_type.as_str() {
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };

    match fs::read(&canonical_path).await {
        Ok(data) => Ok((StatusCode::OK, [(header::CONTENT_TYPE, content_type)], data)),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Icon file not found".to_string(),
            }),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::IconItem;

    #[test]
    fn icon_item_serializes_as_camel_case() {
        let item = IconItem {
            id: "id-1".to_string(),
            name: "marker".to_string(),
            file_type: "png".to_string(),
            width: Some(24),
            height: Some(24),
            size: 1024,
            status: "ready".to_string(),
            error: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: None,
        };

        let value = serde_json::to_value(item).expect("serialize IconItem");
        let obj = value.as_object().expect("json object");

        assert!(obj.contains_key("fileType"));
        assert!(obj.contains_key("createdAt"));
        assert!(obj.contains_key("updatedAt"));

        assert!(!obj.contains_key("file_type"));
        assert!(!obj.contains_key("created_at"));
        assert!(!obj.contains_key("updated_at"));
    }
}
