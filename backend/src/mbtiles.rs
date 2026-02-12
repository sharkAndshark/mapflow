//! MBTiles file handling module
//!
//! MBTiles is a file format for storing map tiles in a SQLite database.
//! This module provides functions to:
//! - Validate MBTiles file structure
//! - Extract metadata (format, bounds, zoom levels)
//! - Retrieve individual tiles
//!
//! ## Coordinate Systems
//! - MBTiles tiles use TMS (Tile Map Service) y-coordinate system
//! - Bounds are stored in WGS84 (EPSG:4326) per MBTiles spec
//! - Tiles are in Web Mercator (EPSG:3857) projection

use rusqlite::Connection;
use std::path::Path;

/// Validate that a file is a valid MBTiles SQLite database
/// with the required metadata and tiles tables
pub fn validate_mbtiles_structure(file_path: &Path) -> Result<(), String> {
    let conn = Connection::open(file_path).map_err(|e| format!("Invalid MBTiles file: {}", e))?;

    // Check metadata table exists
    let has_metadata: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='metadata')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to check metadata table: {}", e))?;

    if !has_metadata {
        return Err("MBTiles file missing metadata table".to_string());
    }

    // Check tiles table exists
    let has_tiles: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='tiles')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to check tiles table: {}", e))?;

    if !has_tiles {
        return Err("MBTiles file missing tiles table".to_string());
    }

    Ok(())
}

/// MBTiles metadata extracted from the metadata table
#[derive(Debug)]
pub struct MbtilesMetadata {
    pub format: String,         // "pbf" or "png"
    pub bounds: Option<String>, // "minx,miny,maxx,maxy"
    #[allow(dead_code)]
    pub center: Option<String>,
    pub minzoom: Option<i32>,
    pub maxzoom: Option<i32>,
    #[allow(dead_code)]
    pub name: Option<String>,
}

/// Extract metadata from an MBTiles file
pub fn extract_mbtiles_metadata(file_path: &Path) -> Result<MbtilesMetadata, String> {
    let conn =
        Connection::open(file_path).map_err(|e| format!("Cannot open MBTiles file: {}", e))?;

    let mut format = String::from("pbf");
    let mut bounds = None;
    let mut center = None;
    let mut minzoom = None;
    let mut maxzoom = None;
    let mut name = None;

    // Query all metadata at once (performance optimization)
    let mut stmt = conn
        .prepare("SELECT name, value FROM metadata")
        .map_err(|e| format!("Failed to prepare metadata query: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| format!("Failed to execute metadata query: {}", e))?;

    for row_result in rows {
        let (key, value) = row_result.map_err(|e| format!("Failed to read metadata row: {}", e))?;
        match key.as_str() {
            "format" => format = value,
            "bounds" => bounds = Some(value),
            "center" => center = Some(value),
            "minzoom" => minzoom = value.parse().ok(),
            "maxzoom" => maxzoom = value.parse().ok(),
            "name" => name = Some(value),
            _ => {
                // Ignore unknown metadata keys
            }
        }
    }

    Ok(MbtilesMetadata {
        format,
        bounds,
        center,
        minzoom,
        maxzoom,
        name,
    })
}

/// Import MBTiles metadata into the database
/// This doesn't import the actual tiles - they stay in the SQLite file
pub async fn import_mbtiles(
    db: &std::sync::Arc<tokio::sync::Mutex<duckdb::Connection>>,
    source_id: &str,
    file_path: &Path,
) -> Result<(), String> {
    let metadata = extract_mbtiles_metadata(file_path)?;

    // Normalize format: "pbf" -> "mvt"
    let tile_format = match metadata.format.as_str() {
        "pbf" => "mvt",
        "png" | "jpg" | "jpeg" => "png",
        _ => return Err(format!("Unsupported tile format: {}", metadata.format)),
    };

    // Parse bounds into JSON array
    // MBTiles spec: bounds in WGS84 (EPSG:4326) as "minx,miny,maxx,maxy"
    let bounds_json = metadata.bounds.and_then(|b| {
        let parts: Vec<&str> = b.split(',').collect();
        if parts.len() != 4 {
            return None;
        }
        let parsed: Vec<f64> = parts.iter().filter_map(|s| s.trim().parse().ok()).collect();
        if parsed.len() == 4 {
            Some(serde_json::json!(parsed).to_string())
        } else {
            None
        }
    });

    let conn = db.lock().await;
    conn.execute(
        "UPDATE files SET crs = 'EPSG:3857', tile_format = ?, minzoom = ?, maxzoom = ?, tile_bounds = ? WHERE id = ?",
        duckdb::params![tile_format, metadata.minzoom, metadata.maxzoom, bounds_json, source_id],
    )
    .map_err(|e| format!("Failed to update file metadata: {}", e))?;

    Ok(())
}

/// Get a tile from an MBTiles file
/// Returns Ok(Some(data)) if tile exists, Ok(None) if tile doesn't exist (but coords are valid)
pub async fn get_tile_from_mbtiles(
    file_path: &Path,
    z: i32,
    x: i32,
    y: i32,
) -> Result<Option<Vec<u8>>, String> {
    // Convert XYZ to TMS (MBTiles uses TMS y-coordinate)
    // TMS y = 2^z - 1 - XYZ y
    let tms_y = (1_i32 << z) - 1 - y;

    let conn =
        Connection::open(file_path).map_err(|e| format!("Cannot open MBTiles file: {}", e))?;

    let tile_data: Option<Vec<u8>> = conn
        .query_row(
            "SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?",
            [z, x, tms_y],
            |row| row.get(0),
        )
        .unwrap_or(None);

    // Filter out empty tiles
    Ok(tile_data.filter(|d| !d.is_empty()))
}

/// Convert a stored file path to an absolute path for MBTiles access
/// The database stores paths as "./relative/path" or "absolute/path"
pub fn resolve_mbtiles_path(file_path: &str) -> std::path::PathBuf {
    use std::path::PathBuf;

    // Handle paths like "./absolute/path" -> "absolute/path"
    // This happens when strip_prefix creates a path starting with ./
    #[allow(clippy::manual_strip)]
    if file_path.starts_with("./") {
        PathBuf::from(&file_path[2..])
    } else if file_path.starts_with('.') {
        // Relative path: join with current directory
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(file_path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(file_path))
    } else {
        // Already an absolute path
        PathBuf::from(file_path)
    }
}
