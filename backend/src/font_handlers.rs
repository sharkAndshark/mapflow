use axum::{
    extract::{Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use axum_login::AuthSession;
use chrono::Utc;
use duckdb::OptionalExt;
use serde::{Deserialize, Serialize};
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
    AppState, AuthBackend,
};

fn create_id() -> String {
    Uuid::new_v4().to_string()
}

async fn get_workspace_id(
    auth_session: &AuthSession<crate::AuthBackend>,
    state: &AppState,
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    match &auth_session.user {
        Some(user) => {
            let workspace_id = user.current_workspace_id.clone().ok_or_else(|| {
                (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse {
                        error: "No active workspace available, please switch workspace".to_string(),
                    }),
                )
            })?;

            let conn = state.db.lock().await;
            let active_workspace: Option<String> = conn
                .query_row(
                    r"
                    SELECT w.id
                    FROM workspaces w
                    JOIN workspace_members wm ON w.id = wm.workspace_id
                    WHERE w.id = ? AND wm.user_id = ? AND w.deleted_at IS NULL
                    LIMIT 1
                    ",
                    duckdb::params![&workspace_id, &user.id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(internal_error)?;
            drop(conn);

            if active_workspace.is_none() {
                return Err((
                    StatusCode::CONFLICT,
                    Json(ErrorResponse {
                        error:
                            "Current workspace is archived or inaccessible, please switch workspace"
                                .to_string(),
                    }),
                ));
            }

            Ok(workspace_id)
        }
        None => Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Not authenticated".to_string(),
            }),
        )),
    }
}

#[derive(Debug, Serialize, Deserialize)]
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
pub struct PublishFontResponse {
    pub url: String,
    pub slug: String,
    pub is_public: bool,
}

pub async fn upload_font(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_workspace_id(&auth_session, &state).await?;

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

    let original_path = fonts_dir.join("original");
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
    let original_rel = format!("./uploads/fonts/{}/original", &font_id);
    let glyphs_rel = format!("./uploads/fonts/{}/glyphs", &font_id);

    let created_at = Utc::now().to_rfc3339();
    let display_name = Path::new(&safe_name)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or(&safe_name)
        .to_string();

    let conn = state.db.lock().await;
    conn.execute(
        "INSERT INTO fonts (id, workspace_id, name, fontstack, original_path, glyphs_path, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        duckdb::params![
            &font_id,
            &workspace_id,
            &display_name,
            &display_name,
            &original_rel,
            &glyphs_rel,
            "processing",
            &created_at,
        ],
    )
    .map_err(internal_error)?;
    drop(conn);

    let state_clone = state.clone();
    let font_id_clone = font_id.clone();
    let original_path_clone = original_path.clone();
    let glyphs_dir_clone = glyphs_dir.clone();

    tokio::spawn(async move {
        let original = original_path_clone.clone();
        let glyphs = glyphs_dir_clone.clone();
        let font_id_for_blocking = font_id_clone.clone();

        let result = tokio::task::spawn_blocking(move || process_font(&original, &glyphs)).await;

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
            }
            Err(e) => {
                error!(font_id = %font_id_for_blocking, error = %e, "Failed to spawn blocking task");
                update_font_error(&state_clone, &font_id_for_blocking, &e.to_string()).await;
            }
        }
    });

    Ok(Json(FontUploadResponse {
        id: font_id,
        status: "processing".to_string(),
    }))
}

async fn update_font_error(state: &AppState, font_id: &str, error: &str) {
    let conn = state.db.lock().await;
    let _ = conn.execute(
        "UPDATE fonts SET status = 'failed', error = ? WHERE id = ?",
        duckdb::params![error, font_id],
    );
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
    Ok(())
}

pub async fn list_fonts(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_workspace_id(&auth_session, &state).await?;

    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, fontstack, family, style, glyph_count, start_cp, end_cp, status, error, is_public, slug, created_at, published_at
             FROM fonts
             WHERE workspace_id = ?
             ORDER BY created_at DESC",
        )
        .map_err(internal_error)?;

    let fonts_iter = stmt
        .query_map(duckdb::params![&workspace_id], |row| {
            let created_at: chrono::NaiveDateTime = row.get(12)?;
            let published_at: Option<chrono::NaiveDateTime> = row.get(13)?;
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
                created_at: created_at.and_utc().to_rfc3339(),
                published_at: published_at.map(|t| t.and_utc().to_rfc3339()),
            })
        })
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
    let workspace_id = get_workspace_id(&auth_session, &state).await?;

    let conn = state.db.lock().await;
    let font: Option<FontItem> = conn
        .query_row(
            "SELECT id, name, fontstack, family, style, glyph_count, start_cp, end_cp, status, error, is_public, slug, created_at, published_at
             FROM fonts
             WHERE id = ? AND workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            |row| {
                let created_at: chrono::NaiveDateTime = row.get(12)?;
                let published_at: Option<chrono::NaiveDateTime> = row.get(13)?;
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
                    created_at: created_at.and_utc().to_rfc3339(),
                    published_at: published_at.map(|t| t.and_utc().to_rfc3339()),
                })
            },
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
    let workspace_id = get_workspace_id(&auth_session, &state).await?;

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

    let glyphs_path = glyphs_path.trim_start_matches("./uploads/");
    let glyphs_dir = state.upload_dir.join(glyphs_path);
    let font_dir = glyphs_dir
        .parent()
        .map_or(glyphs_dir.clone(), std::path::Path::to_path_buf);
    if let Err(e) = tokio::fs::remove_dir_all(&font_dir).await {
        warn!(font_dir = %font_dir.display(), error = %e, "Failed to remove font directory");
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

pub async fn publish_font(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PublishFontRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_workspace_id(&auth_session, &state).await?;

    let slug = match req.slug {
        Some(s) => validate_slug(&s).map_err(|e| bad_request(&e))?,
        None => validate_slug(&id).map_err(|e| bad_request(&e))?,
    };

    info!(font_id = %id, slug = %slug, workspace_id = %workspace_id, "Publish font request");

    let conn = state.db.lock().await;

    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM fonts WHERE id = ? AND workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal_error)?;

    let Some(status) = status else {
        drop(conn);
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Font not found".to_string(),
            }),
        ));
    };

    if status != "ready" {
        drop(conn);
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!("Font is not ready for publishing (status: {})", status),
            }),
        ));
    }

    let published_at = Utc::now().naive_utc();
    let result = conn.execute(
        "UPDATE fonts SET is_public = TRUE, slug = ?, published_at = ? WHERE id = ? AND workspace_id = ?",
        duckdb::params![&slug, published_at, &id, &workspace_id],
    );

    match result {
        Ok(_) => {
            drop(conn);
            info!(font_id = %id, slug = %slug, "Font published");
            let url = format!("/fonts/{}/glyphs/{{fontstack}}/{{range}}.pbf", slug);
            Ok(Json(PublishFontResponse {
                url,
                slug,
                is_public: true,
            }))
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("UNIQUE") || err_msg.contains("unique") {
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
    let workspace_id = get_workspace_id(&auth_session, &state).await?;

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
    AxumPath((slug, _fontstack, range)): AxumPath<(String, String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.db.lock().await;

    let glyphs_path: Option<String> = conn
        .query_row(
            "SELECT glyphs_path FROM fonts WHERE slug = ? AND is_public = TRUE",
            duckdb::params![&slug],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal_error)?;

    drop(conn);

    let Some(glyphs_path) = glyphs_path else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Font not found".to_string(),
            }),
        ));
    };

    let glyphs_path = glyphs_path.trim_start_matches("./uploads/");
    let normalized_range = range.strip_suffix(".pbf").unwrap_or(range.as_str());
    let pbf_path = state
        .upload_dir
        .join(glyphs_path)
        .join(format!("{}.pbf", normalized_range));

    match fs::read(&pbf_path).await {
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
    }
}
