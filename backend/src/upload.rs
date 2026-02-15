use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use rand::RngCore;
use std::path::Path;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};

use crate::{
    http_errors::{bad_request, internal_error, payload_too_large},
    import::import_spatial_data,
    mbtiles,
    models::{ErrorResponse, FileItem},
    AppState,
};

pub async fn upload_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
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
    let dir = state.upload_dir.join(&upload_id);
    fs::create_dir_all(&dir).await.map_err(internal_error)?;

    let file_path = dir.join(&safe_name);
    let mut file = BufWriter::new(fs::File::create(&file_path).await.map_err(internal_error)?);

    let mut size: u64 = 0;
    while let Some(chunk) = field.chunk().await.map_err(internal_error)? {
        size = size.saturating_add(chunk.len() as u64);
        if size > state.max_size {
            drop(file);
            let _ = fs::remove_file(&file_path).await;
            let message = format!("File too large (max {})", state.max_size_label);
            return Err(payload_too_large(&message));
        }
        file.write_all(&chunk).await.map_err(internal_error)?;
    }
    file.flush().await.map_err(internal_error)?;
    drop(file);

    let base_name = Path::new(&safe_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&safe_name)
        .to_string();

    let validation = match file_type {
        "shapefile" => crate::validation::validate_shapefile_zip(&file_path).await,
        "geojson" => crate::validation::validate_geojson(&file_path).await,
        "mbtiles" => mbtiles::validate_mbtiles_structure(&file_path),
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
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            ],
        )
        .map_err(internal_error)?;

        drop(conn);
        return Err(bad_request(&message));
    }

    let size_i64 = size as i64;
    conn.execute(
        "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
        ],
    )
    .map_err(internal_error)?;

    drop(conn);

    let db = state.db.clone();
    let upload_id_clone = upload_id.clone();
    let file_path_clone = file_path.clone();
    let file_type_clone = file_type.to_string();
    tokio::spawn(async move {
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
                println!("Successfully imported spatial data for {}", upload_id_clone);
                let conn = db.lock().await;
                let _ = conn.execute(
                    "UPDATE files SET status = 'ready' WHERE id = ?",
                    duckdb::params![upload_id_clone],
                );
            }
            Err(e) => {
                eprintln!(
                    "Failed to import spatial data for {}: {}",
                    upload_id_clone, e
                );
                let conn = db.lock().await;
                let _ = conn.execute(
                    "UPDATE files SET status = 'failed', error = ? WHERE id = ?",
                    duckdb::params![e, upload_id_clone],
                );
            }
        }
    });

    let meta = FileItem {
        id: upload_id,
        name: base_name,
        file_type: file_type.to_string(),
        size,
        uploaded_at,
        status: "uploaded".to_string(),
        crs: None,
        path: rel_string,
        table_name: None,
        error: None,
        is_public: Some(false),
        public_slug: None,
    };

    Ok((StatusCode::CREATED, Json(meta)))
}

fn create_id() -> String {
    let mut bytes = [0u8; 3];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
