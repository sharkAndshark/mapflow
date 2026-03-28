use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_login::AuthSession;
use chrono::Utc;
use duckdb::OptionalExt;
use std::path::Path;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{info_span, Instrument};
use uuid::Uuid;

use crate::{
    http_errors::{bad_request, internal_error, payload_too_large},
    import::import_spatial_data,
    mbtiles,
    models::{ErrorResponse, FileItem},
    AppState, AuthBackend,
};
use tracing::debug;

pub async fn upload_file(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    debug!("upload_file: starting upload, workspace_id check");
    let workspace_id = match auth_session.user {
        Some(ref user) => {
            debug!(
                "upload_file: user found, current_workspace_id: {:?}",
                user.current_workspace_id
            );
            let workspace_id = user.current_workspace_id.clone().ok_or_else(|| {
                debug!("upload_file: no current workspace set for user");
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

            workspace_id
        }
        None => {
            debug!("upload_file: no user in session, checking test mode");
            let test_mode = std::env::var("MAPFLOW_TEST_MODE").as_deref() == Ok("1");
            debug!("upload_file: test_mode = {}", test_mode);
            if test_mode {
                debug!("upload_file: test mode enabled, looking for workspace");
                let conn = state.db.lock().await;

                let workspace_id: Option<String> = conn
                    .query_row(
                        "SELECT id FROM workspaces WHERE is_personal = true AND deleted_at IS NULL LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .ok()
                    .flatten();

                if let Some(wid) = workspace_id {
                    drop(conn);
                    debug!(
                        "upload_file: found existing workspace in test mode: {}",
                        wid
                    );
                    wid
                } else {
                    debug!("upload_file: no workspace found, creating one");

                    let existing_user_id: Option<String> = conn
                        .query_row("SELECT id FROM users LIMIT 1", [], |row| row.get(0))
                        .ok()
                        .flatten();

                    let user_id = match existing_user_id {
                        Some(uid) => uid,
                        None => {
                            let new_user_id = uuid::Uuid::new_v4().to_string();
                            conn.execute(
                                "INSERT INTO users (id, username, password_hash, role, current_workspace_id, created_at) VALUES (?, ?, '', 'user', NULL, CURRENT_TIMESTAMP)",
                                duckdb::params![&new_user_id, format!("test_user_{}", &new_user_id[..8])],
                            ).ok();
                            new_user_id
                        }
                    };

                    let new_workspace_id = uuid::Uuid::new_v4().to_string();
                    let workspace_name = "Test Workspace".to_string();
                    let workspace_slug = crate::workspace::workspace_slug_base_from_name_or_id(
                        &workspace_name,
                        &new_workspace_id,
                    );

                    conn.execute(
                        "INSERT INTO workspaces (id, name, slug, owner_id, is_personal, created_at) VALUES (?, ?, ?, ?, true, CURRENT_TIMESTAMP)",
                        duckdb::params![&new_workspace_id, &workspace_name, &workspace_slug, &user_id],
                    ).ok();

                    conn.execute(
                        "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, CURRENT_TIMESTAMP)",
                        duckdb::params![&new_workspace_id, &user_id],
                    ).ok();

                    drop(conn);
                    debug!(
                        "upload_file: created new workspace in test mode: {}",
                        new_workspace_id
                    );
                    new_workspace_id
                }
            } else {
                debug!("upload_file: not authenticated and not in test mode");
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorResponse {
                        error: "Not authenticated".to_string(),
                    }),
                ));
            }
        }
    };

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
        .ok_or_else(|| bad_request("Unsupported file type. Use .zip, .geojson, .json, .geojsonl, .kml, .gpx, .topojson, .mbtiles, or .pmtiles"))?;

    let file_type = match ext.as_str() {
        ".zip" => "shapefile",
        ".geojson" | ".json" => "geojson",
        ".geojsonl" | ".geojsons" => "geojsonl",
        ".kml" => "kml",
        ".gpx" => "gpx",
        ".topojson" => "topojson",
        ".mbtiles" => "mbtiles",
        ".pmtiles" => "pmtiles",
        _ => return Err(bad_request(
            "Unsupported file type. Use .zip, .geojson, .json, .geojsonl, .kml, .gpx, .topojson, .mbtiles, or .pmtiles",
        )),
    };

    let upload_id = create_id();
    tracing::info!(upload_id = %upload_id, filename = %safe_name, file_type = file_type, "Upload started");

    let dir = state.upload_dir.join(&upload_id);
    fs::create_dir_all(&dir).await.map_err(internal_error)?;

    let file_path = dir.join(&safe_name);
    let mut file = BufWriter::new(fs::File::create(&file_path).await.map_err(internal_error)?);

    let mut size: u64 = 0;
    let max_size = *state.max_size.read().await;
    let max_size_label = state.max_size_label.read().await.clone();
    while let Some(chunk) = field.chunk().await.map_err(internal_error)? {
        size = size.saturating_add(chunk.len() as u64);
        if size > max_size {
            drop(file);
            let _ = fs::remove_file(&file_path).await;
            let message = format!("File too large (max {})", max_size_label);
            return Err(payload_too_large(&message));
        }
        file.write_all(&chunk).await.map_err(internal_error)?;
    }
    file.flush().await.map_err(internal_error)?;
    // Force sync to disk before background import to prevent GDAL race condition
    file.get_ref().sync_all().await.map_err(internal_error)?;
    drop(file);

    let base_name = Path::new(&safe_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&safe_name)
        .to_string();

    let validation = match file_type {
        "shapefile" => crate::validation::validate_shapefile_zip(&file_path).await,
        "geojson" => crate::validation::validate_geojson(&file_path).await,
        "mbtiles" => mbtiles::validate_mbtiles_structure_async(&file_path).await,
        "pmtiles" => Ok(()),
        "geojsonl" | "kml" | "gpx" | "topojson" => Ok(()),
        _ => Ok(()),
    };

    let uploaded_at = Utc::now().to_rfc3339();

    let relative = file_path
        .strip_prefix(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
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
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public, workspace_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                &workspace_id,
            ],
        )
        .map_err(internal_error)?;

        drop(conn);
        return Err(bad_request(&message));
    }

    let size_i64 = size as i64;
    conn.execute(
        "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public, workspace_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            &workspace_id,
        ],
    )
    .map_err(internal_error)?;

    drop(conn);

    let db = state.db.clone();
    let upload_id_clone = upload_id.clone();
    let file_path_clone = file_path.clone();
    let file_type_clone = file_type.to_string();
    let span = info_span!("import", upload_id = %upload_id, file_type = %file_type);
    tokio::spawn(
        async move {
            tracing::info!("Starting import");
            {
                let conn = db.lock().await;
                let _ = conn.execute(
                    "UPDATE files SET status = 'processing' WHERE id = ?",
                    duckdb::params![upload_id_clone],
                );
            }

            let result = match file_type_clone.as_str() {
                "mbtiles" => mbtiles::import_mbtiles(&db, &upload_id_clone, &file_path_clone).await,
                "pmtiles" => {
                    let conn = db.lock().await;
                    let _ = conn.execute(
                        "UPDATE files SET status = 'ready', tile_source = 'pmtiles' WHERE id = ?",
                        duckdb::params![upload_id_clone],
                    );
                    Ok(())
                }
                _ => import_spatial_data(&db, &upload_id_clone, &file_path_clone).await,
            };

            match result {
                Ok(_) => {
                    tracing::info!("Import completed successfully");
                    let conn = db.lock().await;
                    let _ = conn.execute(
                        "UPDATE files SET status = 'ready' WHERE id = ?",
                        duckdb::params![upload_id_clone],
                    );
                }
                Err(e) => {
                    tracing::error!(error = %e, "Import failed");
                    let conn = db.lock().await;
                    let _ = conn.execute(
                        "UPDATE files SET status = 'failed', error = ? WHERE id = ?",
                        duckdb::params![e, upload_id_clone],
                    );
                }
            }
        }
        .instrument(span),
    );

    let meta = FileItem {
        id: upload_id,
        name: base_name,
        file_type: file_type.to_string(),
        size,
        uploaded_at,
        status: "uploaded".to_string(),
        crs: None,
        crs_type: None,
        path: rel_string,
        table_name: None,
        error: None,
        is_public: Some(false),
        public_slug: None,
        tile_format: None,
        minzoom: None,
        maxzoom: None,
        use_aliases: None,
        tile_source: None,
    };

    Ok((StatusCode::CREATED, Json(meta)))
}

fn create_id() -> String {
    Uuid::new_v4().to_string()
}
