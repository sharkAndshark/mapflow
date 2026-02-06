use std::path::Path;

use tokio::fs;
use zip::ZipArchive;

pub async fn validate_shapefile_zip(file_path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(file_path).map_err(|_| "Unable to read zip file".to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "Unable to read zip file".to_string())?;

    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|_| "Unable to read zip file".to_string())?;
        if entry.is_file() {
            if let Some(name) = Path::new(entry.name()).file_name() {
                entries.push(name.to_string_lossy().to_lowercase());
            }
        }
    }

    if entries.iter().all(|name| !name.ends_with(".shp")) {
        return Err("Missing .shp file in zip".to_string());
    }

    let shp_bases: Vec<String> = entries
        .iter()
        .filter_map(|name| name.strip_suffix(".shp").map(|base| base.to_string()))
        .collect();

    for base in shp_bases {
        let has_shx = entries.iter().any(|name| name == &format!("{base}.shx"));
        let has_dbf = entries.iter().any(|name| name == &format!("{base}.dbf"));
        if has_shx && has_dbf {
            return Ok(());
        }
    }

    Err("Shapefile zip must include .shp/.shx/.dbf with the same name".to_string())
}

pub async fn validate_geojson(file_path: &Path) -> Result<(), String> {
    let data = fs::read_to_string(file_path)
        .await
        .map_err(|_| "Invalid GeoJSON".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&data).map_err(|_| "Invalid GeoJSON".to_string())?;
    if !value.is_object() {
        return Err("Invalid GeoJSON".to_string());
    }
    Ok(())
}
