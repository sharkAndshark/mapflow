use axum::{
    extract::{Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use axum_login::AuthSession;
use duckdb::OptionalExt;
use pbf_font_tools::prost::Message;
use pbf_font_tools::Glyphs;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    font_processor::{process_font, FontMetadata},
    http_errors::{bad_request, internal_error},
    models::ErrorResponse,
    workspace::get_active_workspace_id,
    AppState, AuthBackend,
};

fn create_id() -> String {
    Uuid::new_v4().to_string()
}

fn relative_path_for(absolute: &Path, upload_dir: &Path) -> String {
    if let Ok(relative) = absolute.strip_prefix(upload_dir) {
        let dir_name = upload_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("uploads");
        return format!(
            "./{}/{}",
            dir_name,
            relative.to_string_lossy().replace('\\', "/")
        );
    }
    let s = absolute.to_string_lossy().replace('\\', "/");
    if s.starts_with('.') {
        s
    } else {
        format!("./{s}")
    }
}

fn resolve_glyphs_dir(stored_path: &str, upload_dir: &Path) -> std::path::PathBuf {
    let dir_name = upload_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("uploads");
    let prefix = format!("./{dir_name}/");
    let relative = stored_path.strip_prefix(&prefix).unwrap_or(stored_path);
    upload_dir.join(relative)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontItem {
    pub id: String,
    pub name: String,
    pub fontstack: String,
    pub family: Option<String>,
    pub style: Option<String>,
    pub glyph_count: Option<i32>,
    pub start_cp: Option<i32>,
    pub end_cp: Option<i32>,
    pub status: String,
    pub error: Option<String>,
    pub is_public: bool,
    pub slug: Option<String>,
    pub workspace_slug: String,
    pub created_at: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FontUploadResponse {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishFontRequest {
    pub slug: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishFontResponse {
    pub url: String,
    pub slug: String,
    pub is_public: bool,
    pub workspace_slug: String,
}

pub async fn upload_font(
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
        .ok_or_else(|| bad_request("Unsupported file type. Use .ttf or .otf"))?;

    if !["ttf", "otf"].contains(&ext.as_str()) {
        return Err(bad_request("Unsupported file type. Use .ttf or .otf"));
    }

    let font_id = create_id();
    info!(font_id = %font_id, filename = %safe_name, "Font upload started");

    let fonts_dir = state.upload_dir.join("fonts").join(&font_id);
    fs::create_dir_all(&fonts_dir)
        .await
        .map_err(internal_error)?;

    let original_path = fonts_dir.join(format!("original.{}", ext));
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
            let _ = fs::remove_dir(&fonts_dir).await;
            let message = format!("File too large (max {})", max_size_label);
            return Err(bad_request(&message));
        }
        file.write_all(&chunk).await.map_err(internal_error)?;
    }
    file.flush().await.map_err(internal_error)?;
    file.get_ref().sync_all().await.map_err(internal_error)?;
    drop(file);

    let glyphs_dir = fonts_dir.join("glyphs");
    let original_rel = relative_path_for(&original_path, &state.upload_dir);
    let glyphs_rel = relative_path_for(&glyphs_dir, &state.upload_dir);

    let display_name = Path::new(&safe_name)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or(&safe_name)
        .to_string();

    let conn = state.db.lock().await;
    let insert_result: Result<(), duckdb::Error> = conn
        .execute(
            "INSERT INTO fonts (id, workspace_id, name, fontstack, original_path, glyphs_path, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)",
            duckdb::params![
                &font_id,
                &workspace_id,
                &display_name,
                &display_name,
                &original_rel,
                &glyphs_rel,
                "processing",
            ],
        )
        .map(|_| ());
    drop(conn);

    if let Err(e) = insert_result {
        let _ = fs::remove_dir_all(&fonts_dir).await;
        return Err(internal_error(e));
    }

    let state_clone = state.clone();
    let font_id_clone = font_id.clone();
    let original_path_clone = original_path.clone();
    let glyphs_dir_clone = glyphs_dir.clone();
    let fonts_dir_clone = fonts_dir.clone();

    tokio::spawn(async move {
        let font_id_for_blocking = font_id_clone.clone();
        let cleanup_dir = fonts_dir_clone.clone();

        let result = tokio::task::spawn_blocking(move || process_font(&original_path_clone, &glyphs_dir_clone)).await;

        match result {
            Ok(Ok((metadata, ranges))) => {
                if let Err(e) =
                    update_font_ready(&state_clone, &font_id_clone, &metadata, ranges.len()).await
                {
                    error!(font_id = %font_id_clone, error = %e, "Failed to update font status");
                }
            }
            Ok(Err(e)) => {
                error!(font_id = %font_id_clone, error = %e, "Failed to process font");
                update_font_error(&state_clone, &font_id_clone, &e.to_string()).await;
                if let Err(cleanup_err) = tokio::fs::remove_dir_all(&fonts_dir_clone).await {
                    warn!(font_dir = %fonts_dir_clone.display(), error = %cleanup_err, "Failed to clean up font directory after processing failure");
                }
            }
            Err(e) => {
                error!(font_id = %font_id_for_blocking, error = %e, "Failed to spawn blocking task");
                update_font_error(&state_clone, &font_id_for_blocking, &e.to_string()).await;
                if let Err(cleanup_err) = tokio::fs::remove_dir_all(&cleanup_dir).await {
                    warn!(font_dir = %cleanup_dir.display(), error = %cleanup_err, "Failed to clean up font directory after blocking task failure");
                }
            }
        }
    });

    Ok((
        StatusCode::CREATED,
        Json(FontUploadResponse {
            id: font_id,
            status: "processing".to_string(),
        }),
    ))
}

async fn update_font_error(state: &AppState, font_id: &str, error: &str) {
    let conn = state.db.lock().await;
    if let Err(e) = conn.execute(
        "UPDATE fonts SET status = 'failed', error = ? WHERE id = ?",
        duckdb::params![error, font_id],
    ) {
        warn!(font_id = %font_id, db_error = %e, "Failed to update font error status");
    }
    drop(conn);
}

async fn update_font_ready(
    state: &AppState,
    font_id: &str,
    metadata: &FontMetadata,
    _range_count: usize,
) -> Result<(), String> {
    let conn = state.db.lock().await;
    conn.execute(
        "UPDATE fonts SET status = 'ready', fontstack = ?, family = ?, style = ?, glyph_count = ?, start_cp = ?, end_cp = ?, error = NULL WHERE id = ?",
        duckdb::params![
            &metadata.fontstack,
            &metadata.family,
            &metadata.style,
            metadata.glyph_count as i32,
            metadata.start_cp as i32,
            metadata.end_cp as i32,
            font_id,
        ],
    )
    .map_err(|e| e.to_string())?;
    drop(conn);
    Ok(())
}

fn read_font_row(row: &duckdb::Row) -> Result<FontItem, duckdb::Error> {
    let created_at: chrono::NaiveDateTime = row.get(13)?;
    let published_at: Option<chrono::NaiveDateTime> = row.get(14)?;
    Ok(FontItem {
        id: row.get(0)?,
        name: row.get(1)?,
        fontstack: row.get(2)?,
        family: row.get(3)?,
        style: row.get(4)?,
        glyph_count: row.get(5)?,
        start_cp: row.get(6)?,
        end_cp: row.get(7)?,
        status: row.get(8)?,
        error: row.get(9)?,
        is_public: row.get(10)?,
        slug: row.get(11)?,
        workspace_slug: row.get(12)?,
        created_at: created_at.and_utc().to_rfc3339(),
        published_at: published_at.map(|t| t.and_utc().to_rfc3339()),
    })
}

pub async fn list_fonts(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT f.id, f.name, f.fontstack, f.family, f.style, f.glyph_count, f.start_cp, f.end_cp, f.status, f.error, f.is_public, f.slug, COALESCE(w.slug, w.id), f.created_at, f.published_at
             FROM fonts f
             JOIN workspaces w ON w.id = f.workspace_id
             WHERE f.workspace_id = ?
             ORDER BY f.created_at DESC",
        )
        .map_err(internal_error)?;

    let fonts_iter = stmt
        .query_map(duckdb::params![&workspace_id], read_font_row)
        .map_err(internal_error)?;

    let mut fonts = Vec::new();
    for font in fonts_iter {
        fonts.push(font.map_err(internal_error)?);
    }

    drop(conn);
    Ok(Json(fonts))
}

pub async fn get_font(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;
    let font: Option<FontItem> = conn
        .query_row(
            "SELECT f.id, f.name, f.fontstack, f.family, f.style, f.glyph_count, f.start_cp, f.end_cp, f.status, f.error, f.is_public, f.slug, COALESCE(w.slug, w.id), f.created_at, f.published_at
             FROM fonts f
             JOIN workspaces w ON w.id = f.workspace_id
             WHERE f.id = ? AND f.workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            read_font_row,
        )
        .optional()
        .map_err(internal_error)?;

    drop(conn);

    match font {
        Some(f) => Ok(Json(f)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Font not found".to_string(),
            }),
        )),
    }
}

pub async fn delete_font(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;

    let glyphs_path: Option<String> = conn
        .query_row(
            "SELECT glyphs_path FROM fonts WHERE id = ? AND workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal_error)?;

    let Some(glyphs_path) = glyphs_path else {
        drop(conn);
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Font not found".to_string(),
            }),
        ));
    };

    conn.execute(
        "DELETE FROM fonts WHERE id = ? AND workspace_id = ?",
        duckdb::params![&id, &workspace_id],
    )
    .map_err(internal_error)?;

    drop(conn);

    let glyphs_dir = resolve_glyphs_dir(&glyphs_path, &state.upload_dir);
    let font_dir = glyphs_dir
        .parent()
        .map_or(glyphs_dir.clone(), std::path::Path::to_path_buf);

    match tokio::fs::canonicalize(&font_dir).await {
        Ok(canonical_font_dir) if canonical_font_dir.starts_with(&state.upload_dir_canonical) => {
            if let Err(e) = tokio::fs::remove_dir_all(&canonical_font_dir).await {
                warn!(font_dir = %canonical_font_dir.display(), error = %e, "Failed to remove font directory");
            }
        }
        Ok(canonical_font_dir) => {
            warn!(
                font_dir = %canonical_font_dir.display(),
                "Skipping font directory removal: path escapes upload directory"
            );
        }
        Err(e) => {
            warn!(font_dir = %font_dir.display(), error = %e, "Font directory not found on disk, skipping removal");
        }
    }

    info!(font_id = %id, "Font deleted");
    Ok(StatusCode::NO_CONTENT)
}

fn validate_slug(slug: &str) -> Result<String, String> {
    let slug = slug.trim().to_lowercase();
    if slug.is_empty() {
        return Err("Slug cannot be empty".to_string());
    }
    if slug.len() > 100 {
        return Err("Slug too long (max 100 characters)".to_string());
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Slug can only contain letters, numbers, hyphens, and underscores".to_string());
    }
    Ok(slug)
}

fn parse_glyph_range(range: &str) -> Option<(u32, u32)> {
    let normalized = range.strip_suffix(".pbf").unwrap_or(range);
    if normalized.contains('/') || normalized.contains('\\') {
        return None;
    }

    let mut parts = normalized.split('-');
    let start = parts.next()?.parse::<u32>().ok()?;
    let end = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if end < start || end - start > 255 {
        return None;
    }

    Some((start, end))
}

fn parse_requested_fontstacks(fontstack: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    for part in fontstack.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            parsed.push(trimmed.to_string());
        }
    }
    parsed
}

pub async fn publish_font(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PublishFontRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let slug = match req.slug {
        Some(s) => validate_slug(&s).map_err(|e| bad_request(&e))?,
        None => validate_slug(&id).map_err(|e| bad_request(&e))?,
    };

    info!(font_id = %id, slug = %slug, workspace_id = %workspace_id, "Publish font request");

    let conn = state.db.lock().await;

    let workspace_meta: Option<(String, String)> = conn
        .query_row(
            "SELECT COALESCE(w.slug, w.id), f.fontstack
             FROM fonts f
             JOIN workspaces w ON w.id = f.workspace_id
             WHERE f.id = ? AND f.workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(internal_error)?;

    let Some((workspace_slug, fontstack)) = workspace_meta else {
        drop(conn);
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Font not found".to_string(),
            }),
        ));
    };

    let published_conflict: bool = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM fonts
                WHERE workspace_id = ? AND id != ? AND is_public = TRUE AND fontstack = ?
            )",
            duckdb::params![&workspace_id, &id, &fontstack],
            |row| row.get(0),
        )
        .map_err(internal_error)?;

    if published_conflict {
        drop(conn);
        return Err(bad_request(
            "Another published font with the same fontstack already exists in this workspace",
        ));
    }

    let result = conn.execute(
        "UPDATE fonts SET is_public = TRUE, slug = ?, published_at = CURRENT_TIMESTAMP WHERE id = ? AND workspace_id = ? AND status = 'ready'",
        duckdb::params![&slug, &id, &workspace_id],
    );

    match result {
        Ok(rows_affected) => {
            drop(conn);
            if rows_affected == 0 {
                return Err((
                    StatusCode::CONFLICT,
                    Json(ErrorResponse {
                        error: "Font is not ready for publishing".to_string(),
                    }),
                ));
            }
            info!(font_id = %id, slug = %slug, "Font published");
            let url = format!("/fonts/{}/{{fontstack}}/{{range}}.pbf", workspace_slug);
            Ok(Json(PublishFontResponse {
                url,
                slug,
                is_public: true,
                workspace_slug,
            }))
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("idx_fonts_workspace_slug") {
                drop(conn);
                Err(bad_request("Slug already in use"))
            } else {
                Err(internal_error(e))
            }
        }
    }
}

pub async fn unpublish_font(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    info!(font_id = %id, "Unpublish font request");

    let conn = state.db.lock().await;

    let rows_affected = conn
        .execute(
            "UPDATE fonts SET is_public = FALSE, slug = NULL, published_at = NULL WHERE id = ? AND workspace_id = ? AND is_public = TRUE",
            duckdb::params![&id, &workspace_id],
        )
        .map_err(internal_error)?;

    drop(conn);

    if rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Font not published".to_string(),
            }),
        ));
    }

    info!(font_id = %id, "Font unpublished");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_public_glyph(
    State(state): State<AppState>,
    AxumPath((workspace_slug, fontstack, range)): AxumPath<(String, String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let (start, end) = parse_glyph_range(&range)
        .ok_or_else(|| bad_request("Invalid glyph range format, expected <start>-<end>.pbf"))?;
    let requested_fontstacks = parse_requested_fontstacks(&fontstack);
    if requested_fontstacks.is_empty() {
        return Err(bad_request("Invalid fontstack"));
    }

    let conn = state.db.lock().await;

    let mut glyphs_paths = Vec::new();
    for requested_fontstack in &requested_fontstacks {
        let glyphs_path: Option<String> = conn
            .query_row(
                "SELECT f.glyphs_path
                 FROM fonts f
                 JOIN workspaces w ON w.id = f.workspace_id
                 WHERE COALESCE(w.slug, w.id) = ?
                   AND f.fontstack = ?
                   AND f.is_public = TRUE
                   AND f.status = 'ready'
                   AND w.deleted_at IS NULL
                 LIMIT 1",
                duckdb::params![&workspace_slug, requested_fontstack],
                |row| row.get(0),
            )
            .optional()
            .map_err(internal_error)?;
        if let Some(path) = glyphs_path {
            glyphs_paths.push(path);
        }
    }

    drop(conn);

    if glyphs_paths.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Font not found".to_string(),
            }),
        ));
    }

    let should_merge = requested_fontstacks.len() > 1 || glyphs_paths.len() > 1;
    if !should_merge {
        let pbf_path = resolve_glyphs_dir(&glyphs_paths[0], &state.upload_dir)
            .join(format!("{}-{}.pbf", start, end));

        let canonical_path = match fs::canonicalize(&pbf_path).await {
            Ok(path) => path,
            Err(_) => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: "Glyph range not found".to_string(),
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

        return match fs::read(&canonical_path).await {
            Ok(data) => Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/x-protobuf")],
                data,
            )),
            Err(_) => Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Glyph range not found".to_string(),
                }),
            )),
        };
    }

    let mut merged_glyphs = Glyphs::default();
    let mut has_any_stack = false;

    for glyphs_path in glyphs_paths {
        let pbf_path = resolve_glyphs_dir(&glyphs_path, &state.upload_dir)
            .join(format!("{}-{}.pbf", start, end));

        let canonical_path = match fs::canonicalize(&pbf_path).await {
            Ok(path) => path,
            Err(_) => continue,
        };

        if !canonical_path.starts_with(&state.upload_dir_canonical) {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "Access denied".to_string(),
                }),
            ));
        }

        let bytes = match fs::read(&canonical_path).await {
            Ok(data) => data,
            Err(_) => continue,
        };

        let glyphs = match Glyphs::decode(bytes.as_slice()) {
            Ok(g) => g,
            Err(e) => {
                warn!(path = %pbf_path.display(), error = %e, "Skipping corrupted glyph file in merge");
                continue;
            }
        };
        if glyphs.stacks.is_empty() {
            continue;
        }

        has_any_stack = true;
        merged_glyphs.stacks.extend(glyphs.stacks);
    }

    if !has_any_stack {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Glyph range not found".to_string(),
            }),
        ));
    }

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        merged_glyphs.encode_to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{parse_glyph_range, parse_requested_fontstacks, FontItem};

    #[test]
    fn parse_glyph_range_accepts_valid_patterns() {
        assert_eq!(parse_glyph_range("0-255"), Some((0, 255)));
        assert_eq!(parse_glyph_range("256-511.pbf"), Some((256, 511)));
    }

    #[test]
    fn parse_glyph_range_rejects_invalid_patterns() {
        assert_eq!(parse_glyph_range("abc-def"), None);
        assert_eq!(parse_glyph_range("10-300"), None);
        assert_eq!(parse_glyph_range("300-10"), None);
        assert_eq!(parse_glyph_range("../0-255"), None);
        assert_eq!(parse_glyph_range("0-255/extra"), None);
    }

    #[test]
    fn parse_requested_fontstacks_splits_and_deduplicates() {
        assert_eq!(
            parse_requested_fontstacks("Noto Sans Regular, Noto Sans Regular , Arial Unicode"),
            vec!["Noto Sans Regular".to_string(), "Arial Unicode".to_string()]
        );
        assert_eq!(parse_requested_fontstacks(" ,  "), Vec::<String>::new());
    }

    #[test]
    fn font_item_serializes_as_camel_case() {
        let item = FontItem {
            id: "id-1".to_string(),
            name: "Noto Sans".to_string(),
            fontstack: "Noto Sans Regular".to_string(),
            family: Some("Noto Sans".to_string()),
            style: Some("Regular".to_string()),
            glyph_count: Some(1024),
            start_cp: Some(0),
            end_cp: Some(1023),
            status: "ready".to_string(),
            error: None,
            is_public: true,
            slug: Some("noto-sans".to_string()),
            workspace_slug: "team-alpha".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            published_at: Some("2024-01-01T00:01:00Z".to_string()),
        };

        let value = serde_json::to_value(item).expect("serialize FontItem");
        let obj = value.as_object().expect("json object");

        assert!(obj.contains_key("glyphCount"));
        assert!(obj.contains_key("startCp"));
        assert!(obj.contains_key("endCp"));
        assert!(obj.contains_key("isPublic"));
        assert!(obj.contains_key("workspaceSlug"));
        assert!(obj.contains_key("createdAt"));
        assert!(obj.contains_key("publishedAt"));

        assert!(!obj.contains_key("glyph_count"));
        assert!(!obj.contains_key("start_cp"));
        assert!(!obj.contains_key("is_public"));
        assert!(!obj.contains_key("created_at"));
    }
}
