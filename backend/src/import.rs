use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::Mutex;
use tracing::warn;

use crate::crs::{normalize_crs, DataBounds, CRS_TYPE_CUSTOM};
use crate::db::escape_sql_string;

const ST_READ_CREATE_MAX_ATTEMPTS: usize = 3;

fn is_all_null_column(
    conn: &duckdb::Connection,
    safe_table_name: &str,
    column_name: &str,
) -> Result<bool, String> {
    let escaped_column = column_name.replace('"', "\"\"");
    let sql = format!(
        "SELECT NOT EXISTS (SELECT 1 FROM \"{safe_table_name}\" WHERE \"{escaped_column}\" IS NOT NULL LIMIT 1)"
    );
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(|e| format!("Failed to inspect column nullability: {}", e))
}

fn is_geojson_like(file_path: &Path) -> bool {
    file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            let ext = ext.to_ascii_lowercase();
            ext == "geojson" || ext == "json"
        })
        .unwrap_or(false)
}

fn is_zip_like(file_path: &Path) -> bool {
    file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
}

fn is_duplicate_ogc_fid_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("duplicate column name")
        && lower.contains("ogc_fid")
        && lower.contains("st_read")
}

struct GeoJsonOgcFidWorkaround {
    temp_path: PathBuf,
    original_name_overrides: HashMap<String, String>,
}

struct ShapefileOgcFidWorkaround {
    temp_dir: PathBuf,
    shp_path: PathBuf,
}

#[derive(Default)]
struct ImportWorkaroundArtifacts {
    rewritten_geojson_path: Option<PathBuf>,
    extracted_shapefile_dir: Option<PathBuf>,
}

impl ImportWorkaroundArtifacts {
    fn set_rewritten_geojson_path(&mut self, path: PathBuf) {
        self.rewritten_geojson_path = Some(path);
    }

    fn set_extracted_shapefile_dir(&mut self, path: PathBuf) {
        self.extracted_shapefile_dir = Some(path);
    }

    fn cleanup(&mut self) {
        if let Some(path) = self.rewritten_geojson_path.take() {
            let _ = std::fs::remove_file(path);
        }
        if let Some(path) = self.extracted_shapefile_dir.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

impl Drop for ImportWorkaroundArtifacts {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn choose_geojson_ogc_fid_workaround_key(root: &Value) -> String {
    let mut candidate = "__mapflow_src_ogc_fid".to_string();
    let mut suffix: usize = 2;
    loop {
        let mut conflict = false;
        match root.get("type").and_then(|v| v.as_str()) {
            Some("FeatureCollection") => {
                if let Some(features) = root.get("features").and_then(|v| v.as_array()) {
                    for feature in features {
                        let Some(obj) = feature.as_object() else {
                            continue;
                        };
                        let Some(props) = obj.get("properties").and_then(|v| v.as_object()) else {
                            continue;
                        };
                        if props.keys().any(|k| k.eq_ignore_ascii_case(&candidate)) {
                            conflict = true;
                            break;
                        }
                    }
                }
            }
            Some("Feature") => {
                if let Some(props) = root.get("properties").and_then(|v| v.as_object()) {
                    conflict = props.keys().any(|k| k.eq_ignore_ascii_case(&candidate));
                }
            }
            _ => {}
        }
        if !conflict {
            return candidate;
        }
        candidate = format!("__mapflow_src_ogc_fid_{suffix}");
        suffix += 1;
    }
}

fn rewrite_feature_ogc_fid_properties(
    props: &mut serde_json::Map<String, Value>,
    workaround_key: &str,
    original_name_overrides: &mut HashMap<String, String>,
) -> bool {
    let keys_to_replace: Vec<String> = props
        .keys()
        .filter(|k| k.eq_ignore_ascii_case("ogc_fid"))
        .cloned()
        .collect();
    if keys_to_replace.is_empty() {
        return false;
    }

    let mut removed: Vec<(String, Value)> = Vec::new();
    for key in keys_to_replace {
        if let Some(value) = props.remove(&key) {
            removed.push((key, value));
        }
    }
    if removed.is_empty() {
        return false;
    }

    let mut next_suffix = 2usize;
    for (index, (original_key, value)) in removed.into_iter().enumerate() {
        let mut candidate = if index == 0 {
            workaround_key.to_string()
        } else {
            let key = format!("{workaround_key}_{next_suffix}");
            next_suffix += 1;
            key
        };
        while props.keys().any(|k| k.eq_ignore_ascii_case(&candidate)) {
            candidate = format!("{workaround_key}_{next_suffix}");
            next_suffix += 1;
        }
        original_name_overrides.insert(candidate.to_ascii_lowercase(), original_key);
        props.insert(candidate, value);
    }

    true
}

fn rewrite_geojson_ogc_fid_properties(
    file_path: &Path,
    source_id: &str,
) -> Result<Option<GeoJsonOgcFidWorkaround>, String> {
    let data = std::fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read GeoJSON for OGC_FID workaround: {}", e))?;
    let mut root: Value = serde_json::from_str(&data)
        .map_err(|e| format!("Failed to parse GeoJSON for OGC_FID workaround: {}", e))?;

    let root_type = root.get("type").and_then(|v| v.as_str());
    let is_feature_collection = root_type == Some("FeatureCollection");
    let is_single_feature = root_type == Some("Feature");
    if !is_feature_collection && !is_single_feature {
        return Ok(None);
    }

    let workaround_key = choose_geojson_ogc_fid_workaround_key(&root);
    let mut changed = false;
    let mut overrides: HashMap<String, String> = HashMap::new();

    if is_feature_collection {
        let Some(features) = root.get_mut("features").and_then(|v| v.as_array_mut()) else {
            return Ok(None);
        };
        for feature in features {
            let Some(feature_obj) = feature.as_object_mut() else {
                continue;
            };
            let Some(props) = feature_obj
                .get_mut("properties")
                .and_then(|v| v.as_object_mut())
            else {
                continue;
            };
            changed |= rewrite_feature_ogc_fid_properties(props, &workaround_key, &mut overrides);
        }
    } else {
        let Some(props) = root.get_mut("properties").and_then(|v| v.as_object_mut()) else {
            return Ok(None);
        };
        changed = rewrite_feature_ogc_fid_properties(props, &workaround_key, &mut overrides);
    }

    if !changed {
        return Ok(None);
    }

    let temp_name = format!(
        "mapflow-import-{}-{}.geojson",
        source_id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Failed to compute temp file timestamp: {}", e))?
            .as_nanos()
    );
    let temp_path = std::env::temp_dir().join(temp_name);
    let serialized = serde_json::to_vec(&root)
        .map_err(|e| format!("Failed to serialize rewritten GeoJSON: {}", e))?;
    std::fs::write(&temp_path, serialized)
        .map_err(|e| format!("Failed to write rewritten GeoJSON temp file: {}", e))?;

    Ok(Some(GeoJsonOgcFidWorkaround {
        temp_path,
        original_name_overrides: overrides,
    }))
}

fn extract_shapefile_for_ogc_fid_workaround(
    file_path: &Path,
    source_id: &str,
) -> Result<Option<ShapefileOgcFidWorkaround>, String> {
    let file = std::fs::File::open(file_path)
        .map_err(|e| format!("Failed to open shapefile zip for OGC_FID workaround: {e}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read shapefile zip for OGC_FID workaround: {e}"))?;

    let mut names: Vec<String> = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|e| format!("Failed to inspect shapefile zip entry: {e}"))?;
        if !entry.is_file() {
            continue;
        }
        let Some(name) = Path::new(entry.name()).file_name() else {
            continue;
        };
        names.push(name.to_string_lossy().to_ascii_lowercase());
    }

    let shp_bases: Vec<String> = names
        .iter()
        .filter_map(|name| name.strip_suffix(".shp").map(|base| base.to_string()))
        .collect();
    let Some(base) = shp_bases.into_iter().find(|candidate| {
        names.iter().any(|name| name == &format!("{candidate}.shx"))
            && names.iter().any(|name| name == &format!("{candidate}.dbf"))
    }) else {
        return Ok(None);
    };

    let temp_dir_name = format!(
        "mapflow-import-{}-{}",
        source_id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Failed to compute temp dir timestamp: {e}"))?
            .as_nanos()
    );
    let temp_dir = std::env::temp_dir().join(temp_dir_name);
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create shapefile workaround dir: {e}"))?;

    let prefix = format!("{base}.");
    let mut has_shp = false;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("Failed to read shapefile zip entry: {e}"))?;
        if !entry.is_file() {
            continue;
        }
        let Some(name) = Path::new(entry.name())
            .file_name()
            .and_then(|name| name.to_str())
        else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if !lower.starts_with(&prefix) {
            continue;
        }

        let ext = Path::new(&lower)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let out_name = if ext.is_empty() {
            lower.clone()
        } else {
            format!("{base}.{ext}")
        };
        let out_path = temp_dir.join(out_name);
        if out_path.exists() {
            continue;
        }

        let mut out_file = std::fs::File::create(&out_path)
            .map_err(|e| format!("Failed to create shapefile workaround entry: {e}"))?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|e| format!("Failed to extract shapefile workaround entry: {e}"))?;
        if ext.eq_ignore_ascii_case("shp") {
            has_shp = true;
        }
    }

    if !has_shp {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Ok(None);
    }

    let shp_path = temp_dir.join(format!("{base}.shp"));
    Ok(Some(ShapefileOgcFidWorkaround { temp_dir, shp_path }))
}

fn meta_fields_contains_column(fields_json: &str, target_column: &str) -> Option<bool> {
    let value: Value = serde_json::from_str(fields_json).ok()?;
    let fields = value.as_array()?;
    Some(fields.iter().any(|field| {
        field
            .get("name")
            .and_then(|name| name.as_str())
            .map(|name| name.eq_ignore_ascii_case(target_column))
            .unwrap_or(false)
    }))
}

fn source_fields_contains_column(
    conn: &duckdb::Connection,
    escaped_path: &str,
    target_column: &str,
) -> Option<bool> {
    let sql = format!("SELECT to_json(layers[1].fields) FROM ST_Read_Meta('{escaped_path}')");
    let fields_json = match conn.query_row(&sql, [], |row| row.get::<_, String>(0)) {
        Ok(fields_json) => fields_json,
        Err(e) => {
            warn!(
                target_column = %target_column,
                error = %e,
                "Failed to inspect source fields from ST_Read_Meta; preserving imported column"
            );
            return None;
        }
    };
    if let Some(contains) = meta_fields_contains_column(&fields_json, target_column) {
        return Some(contains);
    }
    warn!(
        target_column = %target_column,
        "Failed to parse ST_Read_Meta fields JSON; preserving imported column"
    );
    None
}

fn should_skip_ogc_fid_column(source_has_ogc_fid: Option<bool>) -> bool {
    matches!(source_has_ogc_fid, Some(false))
}

fn drop_synthetic_columns_before_normalization(
    conn: &duckdb::Connection,
    safe_table_name: &str,
    columns: &mut Vec<(String, String, i64)>,
    source_has_id_column: Option<bool>,
    source_has_ogc_fid_column: Option<bool>,
) -> Result<(), String> {
    let mut dropped_columns: HashSet<String> = HashSet::new();
    for (name, _data_type, _ordinal) in columns.iter() {
        let lower = name.to_ascii_lowercase();

        let should_drop_ogc_fid =
            lower == "ogc_fid" && should_skip_ogc_fid_column(source_has_ogc_fid_column);
        let should_drop_id = lower == "id"
            && matches!(source_has_id_column, Some(false))
            && is_all_null_column(conn, safe_table_name, name)?;

        if should_drop_ogc_fid || should_drop_id {
            let escaped_column = name.replace('"', "\"\"");
            let drop_sql = format!(
                "ALTER TABLE \"{safe_table_name}\" DROP COLUMN IF EXISTS \"{escaped_column}\""
            );
            conn.execute(&drop_sql, []).map_err(|e| {
                format!(
                    "Failed to drop synthetic column before normalization: {}",
                    e
                )
            })?;
            dropped_columns.insert(lower);
        }
    }

    if !dropped_columns.is_empty() {
        columns.retain(|(name, _, _)| !dropped_columns.contains(&name.to_ascii_lowercase()));
    }

    Ok(())
}

pub async fn import_spatial_data(
    db: &Arc<Mutex<duckdb::Connection>>,
    source_id: &str,
    file_path: &Path,
) -> Result<(), String> {
    let abs_path = std::fs::canonicalize(file_path)
        .map_err(|e| format!("Cannot resolve file path {:?}: {}", file_path, e))?
        .to_string_lossy()
        .to_string();

    let abs_path = if file_path.extension().and_then(|e| e.to_str()) == Some("zip") {
        format!("/vsizip/{}", abs_path)
    } else {
        abs_path
    };
    let escaped_path = escape_sql_string(&abs_path);
    let mut import_path_for_st_read = abs_path.clone();
    let mut original_name_overrides: HashMap<String, String> = HashMap::new();
    let mut workaround_artifacts = ImportWorkaroundArtifacts::default();
    let mut preserve_ogc_fid_from_workaround = false;

    let conn = db.lock().await;

    // 1. Detect CRS using ST_Read_Meta
    // layers[1].geometry_fields[1].crs.auth_name / auth_code
    // Note: ST_Read_Meta return structure depends on the file.
    // We try to get the first layer's CRS.
    // List indexing in DuckDB is 1-based.
    let crs_query = format!(
        "SELECT 
            layers[1].geometry_fields[1].crs.auth_name || ':' || layers[1].geometry_fields[1].crs.auth_code 
         FROM ST_Read_Meta('{escaped_path}')"
    );

    let detected_crs: Option<String> = conn.query_row(&crs_query, [], |row| row.get(0)).ok();

    // Normalize CRS and determine type
    let normalized = normalize_crs(detected_crs.as_deref());

    // 2. Import Data into a per-dataset table (layer_<id>) so we can preserve columns.
    // We keep a stable feature id column (fid) for MVT feature ids.
    let table_name = format!("layer_{}", source_id);
    let safe_table_name =
        normalize_column_name(&table_name).unwrap_or_else(|| format!("layer_{}", source_id));

    // Drop if exists (id collision should be impossible, but keep idempotent).
    let _ = conn.execute(&format!("DROP TABLE IF EXISTS \"{safe_table_name}\""), []);

    let create_sql = format!(
        "CREATE TABLE \"{safe_table_name}\" AS\n         SELECT row_number() OVER ()::BIGINT AS fid, *\n         FROM ST_Read('{}')",
        escape_sql_string(&import_path_for_st_read)
    );

    if let Err(initial_error) = execute_create_with_retry(&conn, &create_sql, source_id) {
        if !is_duplicate_ogc_fid_error(&initial_error) {
            return Err(initial_error);
        }

        if is_geojson_like(file_path) {
            if let Some(workaround) = rewrite_geojson_ogc_fid_properties(file_path, source_id)? {
                let _ = conn.execute(&format!("DROP TABLE IF EXISTS \"{safe_table_name}\""), []);
                let temp_path = workaround.temp_path;
                import_path_for_st_read = temp_path.to_string_lossy().to_string();
                workaround_artifacts.set_rewritten_geojson_path(temp_path);
                original_name_overrides = workaround.original_name_overrides;
                preserve_ogc_fid_from_workaround = true;

                let workaround_sql = format!(
                    "CREATE TABLE \"{safe_table_name}\" AS\n                     SELECT row_number() OVER ()::BIGINT AS fid, *\n                     FROM ST_Read('{}')",
                    escape_sql_string(&import_path_for_st_read)
                );
                execute_create_with_retry(&conn, &workaround_sql, source_id)?;
            } else {
                return Err(initial_error);
            }
        } else if is_zip_like(file_path) {
            if let Some(workaround) =
                extract_shapefile_for_ogc_fid_workaround(file_path, source_id)?
            {
                let _ = conn.execute(&format!("DROP TABLE IF EXISTS \"{safe_table_name}\""), []);
                import_path_for_st_read = workaround.shp_path.to_string_lossy().to_string();
                workaround_artifacts.set_extracted_shapefile_dir(workaround.temp_dir);
                preserve_ogc_fid_from_workaround = true;

                let workaround_sql = format!(
                    "CREATE TABLE \"{safe_table_name}\" AS\n                     SELECT row_number() OVER ()::BIGINT AS fid, *\n                     FROM ST_ReadSHP('{}')",
                    escape_sql_string(&import_path_for_st_read)
                );
                execute_create_with_retry(&conn, &workaround_sql, source_id)?;
            } else {
                return Err(initial_error);
            }
        } else {
            return Err(initial_error);
        }
    }

    if !original_name_overrides.is_empty() {
        // When workaround is active, OGC_FID is a synthetic metadata column.
        let _ = conn.execute(
            &format!("ALTER TABLE \"{safe_table_name}\" DROP COLUMN IF EXISTS \"OGC_FID\""),
            [],
        );
    }

    // Calculate data_bounds (extent of all geometries)
    let bounds_query = format!(
        "SELECT ST_XMin(e), ST_YMin(e), ST_XMax(e), ST_YMax(e) FROM (SELECT ST_Extent(geom) as e FROM \"{safe_table_name}\")"
    );
    let data_bounds: Option<DataBounds> = conn
        .query_row(&bounds_query, [], |row| {
            let minx: Option<f64> = row.get(0).ok();
            let miny: Option<f64> = row.get(1).ok();
            let maxx: Option<f64> = row.get(2).ok();
            let maxy: Option<f64> = row.get(3).ok();
            match (minx, miny, maxx, maxy) {
                (Some(x1), Some(y1), Some(x2), Some(y2)) => Ok(Some(DataBounds {
                    minx: x1,
                    miny: y1,
                    maxx: x2,
                    maxy: y2,
                })),
                _ => Ok(None),
            }
        })
        .ok()
        .flatten();

    // If GDAL reports EPSG:4326 but coordinates are outside valid WGS84 range,
    // override to custom CRS (GDAL defaults to EPSG:4326 for GeoJSON without CRS)
    let normalized = if normalized.crs.as_deref() == Some("EPSG:4326") {
        match &data_bounds {
            Some(bounds) if !bounds.is_valid_wgs84() => crate::crs::NormalizedCrs {
                crs: None,
                crs_type: CRS_TYPE_CUSTOM.to_string(),
            },
            _ => normalized,
        }
    } else {
        normalized
    };

    // Update files table with CRS info and data_bounds
    let data_bounds_json = data_bounds.map(|b| b.to_json());
    conn.execute(
        "UPDATE files SET crs = ?, crs_type = ?, data_bounds = ?, table_name = ? WHERE id = ?",
        duckdb::params![
            normalized.crs,
            normalized.crs_type,
            data_bounds_json,
            safe_table_name.as_str(),
            source_id
        ],
    )
    .map_err(|e| format!("Failed to update file metadata: {}", e))?;

    // 3. Normalize/rename columns when needed and capture metadata.
    // DuckDB is case-insensitive for identifiers, so we treat case-only differences as conflicts.
    // Strategy:
    // - Keep original name if it is already a safe identifier and unique (case-insensitive)
    // - Otherwise normalize (lowercase + non [a-z0-9_] -> '_' + trim)
    // - If still conflicts, suffix _2, _3...
    // - Ensure reserved columns fid + geom stay as-is.
    let mut columns_stmt = conn
        .prepare(
            "SELECT column_name, data_type, ordinal_position\n             FROM information_schema.columns\n             WHERE table_schema = 'main' AND table_name = ?\n             ORDER BY ordinal_position",
        )
        .map_err(|e| format!("Metadata query failed: {}", e))?;

    let columns_iter = columns_stmt
        .query_map(duckdb::params![safe_table_name.as_str()], |row| {
            let name: String = row.get(0)?;
            let data_type: String = row.get(1)?;
            let ordinal: i64 = row.get(2)?;
            Ok((name, data_type, ordinal))
        })
        .map_err(|e| format!("Metadata query failed: {}", e))?;

    let mut columns: Vec<(String, String, i64)> = Vec::new();
    for col in columns_iter {
        columns.push(col.map_err(|e| format!("Metadata query failed: {}", e))?);
    }

    // Clear any prior metadata.
    let _ = conn.execute(
        "DELETE FROM dataset_columns WHERE source_id = ?",
        duckdb::params![source_id],
    );

    let mut used: HashSet<String> = HashSet::new();
    used.insert("fid".to_string());

    // Ensure geometry column is named `geom` for downstream queries.
    // Most drivers already use `geom`, but don't rely on it.
    // If we find a GEOMETRY column that isn't named `geom`, rename it.
    for (name, data_type, _ordinal) in &columns {
        if data_type.eq_ignore_ascii_case("GEOMETRY") && name != "geom" {
            let alter =
                format!("ALTER TABLE \"{safe_table_name}\" RENAME COLUMN \"{name}\" TO geom");
            conn.execute(&alter, [])
                .map_err(|e| format!("Failed to normalize geometry column: {}", e))?;
        }
    }

    // Refresh columns after potential geom rename.
    let mut refresh_stmt = conn
        .prepare(
            "SELECT column_name, data_type, ordinal_position\n             FROM information_schema.columns\n             WHERE table_schema = 'main' AND table_name = ?\n             ORDER BY ordinal_position",
        )
        .map_err(|e| format!("Metadata query failed: {}", e))?;

    let columns_iter = refresh_stmt
        .query_map(duckdb::params![safe_table_name.as_str()], |row| {
            let name: String = row.get(0)?;
            let data_type: String = row.get(1)?;
            let ordinal: i64 = row.get(2)?;
            Ok((name, data_type, ordinal))
        })
        .map_err(|e| format!("Metadata query failed: {}", e))?;

    let mut columns: Vec<(String, String, i64)> = Vec::new();
    for col in columns_iter {
        columns.push(col.map_err(|e| format!("Metadata query failed: {}", e))?);
    }

    // Use source metadata to distinguish synthetic `id` from real source attributes.
    // If metadata is unavailable, we preserve `id` to avoid silently dropping user columns.
    let source_has_id_column = source_fields_contains_column(&conn, &escaped_path, "id");
    let source_has_ogc_fid_column = if preserve_ogc_fid_from_workaround {
        Some(true)
    } else {
        source_fields_contains_column(&conn, &escaped_path, "ogc_fid")
    };

    // Drop synthetic reader columns before normalization/rename.
    // Otherwise, later renames (e.g. "ogc fid" -> "ogc_fid") can collide with
    // skipped synthetic columns that still physically exist in the table.
    drop_synthetic_columns_before_normalization(
        &conn,
        &safe_table_name,
        &mut columns,
        source_has_id_column,
        source_has_ogc_fid_column,
    )?;

    for (name, data_type, ordinal) in &columns {
        let override_original_name = original_name_overrides
            .get(&name.to_ascii_lowercase())
            .cloned();
        let original_name = override_original_name.unwrap_or_else(|| name.clone());
        let lower = name.to_ascii_lowercase();
        let original_lower = original_name.to_ascii_lowercase();

        let is_reserved = original_lower == "fid" || original_lower == "geom";

        // Determine normalized name.
        let mut normalized = if is_reserved {
            original_lower.clone()
        } else if original_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && (original_name
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false))
        {
            // Keep as-is but lowercase to match DuckDB identifier behavior.
            original_lower.clone()
        } else {
            normalize_column_name(&original_name).unwrap_or_else(|| format!("col_{ordinal}"))
        };

        if is_reserved {
            used.insert(normalized.clone());
        } else {
            if normalized.is_empty() {
                normalized = format!("col_{ordinal}");
            }
            let mut candidate = normalized.clone();
            let mut suffix = 2;
            while used.contains(&candidate) {
                candidate = format!("{normalized}_{suffix}");
                suffix += 1;
            }
            normalized = candidate;
            used.insert(normalized.clone());

            if normalized != lower {
                let alter = format!(
                    "ALTER TABLE \"{safe_table_name}\" RENAME COLUMN \"{name}\" TO \"{normalized}\""
                );
                conn.execute(&alter, [])
                    .map_err(|e| format!("Failed to normalize column name: {}", e))?;
            }
        }

        // Coerce unsupported property types to VARCHAR so they can be included in MVT.
        // Keep GEOMETRY as-is.
        let mvt_type = if original_lower == "geom" {
            "GEOMETRY".to_string()
        } else if original_lower == "fid" {
            "BIGINT".to_string()
        } else {
            match data_type.as_str() {
                "VARCHAR" | "BOOLEAN" | "DOUBLE" | "FLOAT" | "BIGINT" | "INTEGER" => {
                    data_type.clone()
                }
                "SMALLINT" | "TINYINT" => {
                    let alter = format!(
                        "ALTER TABLE \"{safe_table_name}\" ALTER COLUMN \"{normalized}\" SET DATA TYPE INTEGER"
                    );
                    conn.execute(&alter, [])
                        .map_err(|e| format!("Failed to coerce column type: {}", e))?;
                    "INTEGER".to_string()
                }
                "UBIGINT" | "UINTEGER" | "USMALLINT" | "UTINYINT" => {
                    let alter = format!(
                        "ALTER TABLE \"{safe_table_name}\" ALTER COLUMN \"{normalized}\" SET DATA TYPE BIGINT"
                    );
                    conn.execute(&alter, [])
                        .map_err(|e| format!("Failed to coerce column type: {}", e))?;
                    "BIGINT".to_string()
                }
                _ => {
                    // Cast to VARCHAR in-place.
                    let alter = format!(
                        "ALTER TABLE \"{safe_table_name}\" ALTER COLUMN \"{normalized}\" SET DATA TYPE VARCHAR"
                    );
                    conn.execute(&alter, [])
                        .map_err(|e| format!("Failed to coerce column type: {}", e))?;
                    "VARCHAR".to_string()
                }
            }
        };

        if original_lower != "geom" && original_lower != "fid" {
            // Record property columns (exclude geom + fid).
            let _ = conn.execute(
                "INSERT INTO dataset_columns (source_id, normalized_name, original_name, alias, ordinal, mvt_type)\n                 VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
                duckdb::params![
                    source_id,
                    normalized.as_str(),
                    original_name.as_str(),
                    *ordinal,
                    mvt_type.as_str()
                ],
            );
        }
    }

    Ok(())
}

fn execute_create_with_retry(
    conn: &duckdb::Connection,
    create_sql: &str,
    source_id: &str,
) -> Result<(), String> {
    let mut last_error = String::new();

    for attempt in 1..=ST_READ_CREATE_MAX_ATTEMPTS {
        match conn.execute(create_sql, []) {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_error = e.to_string();
                let should_retry = attempt < ST_READ_CREATE_MAX_ATTEMPTS
                    && is_retryable_st_read_error(&last_error);
                if should_retry {
                    let backoff_ms = 50_u64 * attempt as u64;
                    warn!(
                        source_id = %source_id,
                        attempt = attempt,
                        max_attempts = ST_READ_CREATE_MAX_ATTEMPTS,
                        backoff_ms = backoff_ms,
                        error = %last_error,
                        "Transient ST_Read failure, retrying spatial import"
                    );
                    std::thread::sleep(Duration::from_millis(backoff_ms));
                    continue;
                }
                break;
            }
        }
    }

    Err(format!("Spatial import failed: {}", last_error))
}

fn is_retryable_st_read_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let has_file_open_signal = lower.contains("no such file or directory")
        || lower.contains("cannot open")
        || lower.contains("gdal error (4)");
    let is_read_path = lower.contains("st_read") || lower.contains("gdal");
    has_file_open_signal && is_read_path
}

fn normalize_column_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }

    while out.contains("__") {
        out = out.replace("__", "_");
    }

    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        return None;
    }

    let first = out.chars().next().unwrap();
    let mut out = if first.is_ascii_alphabetic() || first == '_' {
        out
    } else {
        format!("col_{out}")
    };

    // Avoid a small set of very common keywords.
    // DuckDB has more, but we mainly want to dodge obvious foot-guns.
    const KEYWORDS: [&str; 10] = [
        "select", "from", "where", "group", "order", "by", "limit", "offset", "join", "table",
    ];
    if KEYWORDS.contains(&out.as_str()) {
        out = format!("col_{out}");
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{
        choose_geojson_ogc_fid_workaround_key, drop_synthetic_columns_before_normalization,
        is_retryable_st_read_error, meta_fields_contains_column,
        rewrite_feature_ogc_fid_properties, should_skip_ogc_fid_column, ImportWorkaroundArtifacts,
    };

    #[test]
    fn test_retryable_st_read_error_true_for_missing_file() {
        let err = "IO Error: GDAL Error (4): /tmp/a.geojson: No such file or directory";
        assert!(is_retryable_st_read_error(err));
    }

    #[test]
    fn test_retryable_st_read_error_false_for_sql_error() {
        let err = "Binder Error: Referenced column \"geomx\" not found in FROM clause";
        assert!(!is_retryable_st_read_error(err));
    }

    #[test]
    fn test_meta_fields_contains_column_detects_existing_name() {
        let fields_json = r#"[{"name":"id","type":"String"},{"name":"name","type":"String"}]"#;
        assert_eq!(meta_fields_contains_column(fields_json, "id"), Some(true));
        assert_eq!(meta_fields_contains_column(fields_json, "name"), Some(true));
        assert_eq!(
            meta_fields_contains_column(fields_json, "missing"),
            Some(false)
        );
    }

    #[test]
    fn test_meta_fields_contains_column_handles_empty_array() {
        assert_eq!(meta_fields_contains_column("[]", "id"), Some(false));
    }

    #[test]
    fn test_meta_fields_contains_column_handles_invalid_json() {
        assert_eq!(meta_fields_contains_column("{", "id"), None);
        assert_eq!(meta_fields_contains_column(r#"{"name":"id"}"#, "id"), None);
    }

    #[test]
    fn test_choose_geojson_ogc_fid_workaround_key_ignores_case() {
        let root: serde_json::Value = serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "properties": {
                        "__MAPFLOW_SRC_OGC_FID": 1
                    },
                    "geometry": {"type": "Point", "coordinates": [0.0, 0.0]}
                }
            ]
        });
        let key = choose_geojson_ogc_fid_workaround_key(&root);
        assert_eq!(key, "__mapflow_src_ogc_fid_2");
    }

    #[test]
    fn test_choose_geojson_ogc_fid_workaround_key_for_top_level_feature() {
        let root: serde_json::Value = serde_json::json!({
            "type": "Feature",
            "properties": {
                "__MAPFLOW_SRC_OGC_FID": 1,
                "name": "A"
            },
            "geometry": {"type": "Point", "coordinates": [0.0, 0.0]}
        });
        let key = choose_geojson_ogc_fid_workaround_key(&root);
        assert_eq!(key, "__mapflow_src_ogc_fid_2");
    }

    #[test]
    fn test_rewrite_feature_ogc_fid_properties_rewrites_all_case_variants() {
        let mut props = serde_json::Map::new();
        props.insert("ogc_fid".to_string(), serde_json::json!(1));
        props.insert("OGC_FID".to_string(), serde_json::json!(2));
        props.insert("name".to_string(), serde_json::json!("A"));

        let mut overrides = std::collections::HashMap::new();
        let changed =
            rewrite_feature_ogc_fid_properties(&mut props, "__mapflow_src_ogc_fid", &mut overrides);
        assert!(changed);

        assert!(props.contains_key("__mapflow_src_ogc_fid"));
        assert!(props.contains_key("__mapflow_src_ogc_fid_2"));
        assert!(!props.contains_key("ogc_fid"));
        assert!(!props.contains_key("OGC_FID"));

        let mut override_values: Vec<String> = overrides.values().cloned().collect();
        override_values.sort();
        assert_eq!(
            override_values,
            vec!["OGC_FID".to_string(), "ogc_fid".to_string()]
        );
    }

    #[test]
    fn test_should_skip_ogc_fid_column_only_when_source_lacks_it() {
        assert!(!should_skip_ogc_fid_column(Some(true)));
        assert!(should_skip_ogc_fid_column(Some(false)));
        assert!(!should_skip_ogc_fid_column(None));
    }

    fn make_unique_temp_path(prefix: &str) -> std::path::PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{ts}", std::process::id()))
    }

    #[test]
    fn test_import_workaround_artifacts_drop_cleans_temp_paths() {
        let temp_file = make_unique_temp_path("mapflow-import-temp-file");
        std::fs::write(&temp_file, b"tmp").expect("create temp file");
        let temp_dir = make_unique_temp_path("mapflow-import-temp-dir");
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        std::fs::write(temp_dir.join("data.txt"), b"tmp").expect("write temp dir file");

        {
            let mut artifacts = ImportWorkaroundArtifacts::default();
            artifacts.set_rewritten_geojson_path(temp_file.clone());
            artifacts.set_extracted_shapefile_dir(temp_dir.clone());
        }

        assert!(
            !temp_file.exists(),
            "rewritten GeoJSON temp file should be removed on drop"
        );
        assert!(
            !temp_dir.exists(),
            "extracted shapefile temp dir should be removed on drop"
        );
    }

    #[test]
    fn test_import_workaround_artifacts_cleanup_is_idempotent() {
        let temp_file = make_unique_temp_path("mapflow-import-temp-file-idempotent");
        std::fs::write(&temp_file, b"tmp").expect("create temp file");
        let temp_dir = make_unique_temp_path("mapflow-import-temp-dir-idempotent");
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let mut artifacts = ImportWorkaroundArtifacts::default();
        artifacts.set_rewritten_geojson_path(temp_file.clone());
        artifacts.set_extracted_shapefile_dir(temp_dir.clone());
        artifacts.cleanup();
        artifacts.cleanup();

        assert!(!temp_file.exists(), "cleanup should remove temp file");
        assert!(!temp_dir.exists(), "cleanup should remove temp dir");
    }

    #[test]
    fn test_drop_synthetic_ogc_fid_column_allows_conflicting_rename() {
        let conn = duckdb::Connection::open_in_memory().expect("in-memory duckdb");
        conn.execute(
            "CREATE TABLE \"layer_test\" (\"fid\" BIGINT, \"ogc_fid\" INTEGER, \"ogc fid\" INTEGER)",
            [],
        )
        .expect("create table");

        let mut columns = vec![
            ("fid".to_string(), "BIGINT".to_string(), 1),
            ("ogc_fid".to_string(), "INTEGER".to_string(), 2),
            ("ogc fid".to_string(), "INTEGER".to_string(), 3),
        ];

        drop_synthetic_columns_before_normalization(
            &conn,
            "layer_test",
            &mut columns,
            Some(true),
            Some(false),
        )
        .expect("drop synthetic columns");

        conn.execute(
            "ALTER TABLE \"layer_test\" RENAME COLUMN \"ogc fid\" TO \"ogc_fid\"",
            [],
        )
        .expect("rename should succeed after dropping synthetic ogc_fid");
    }

    #[test]
    fn test_drop_synthetic_all_null_id_column_allows_conflicting_rename() {
        let conn = duckdb::Connection::open_in_memory().expect("in-memory duckdb");
        conn.execute(
            "CREATE TABLE \"layer_test\" (\"fid\" BIGINT, \"id\" INTEGER, \"id \" INTEGER)",
            [],
        )
        .expect("create table");
        conn.execute(
            "INSERT INTO \"layer_test\" (\"fid\", \"id\", \"id \") VALUES (1, NULL, 99)",
            [],
        )
        .expect("insert row");

        let mut columns = vec![
            ("fid".to_string(), "BIGINT".to_string(), 1),
            ("id".to_string(), "INTEGER".to_string(), 2),
            ("id ".to_string(), "INTEGER".to_string(), 3),
        ];

        drop_synthetic_columns_before_normalization(
            &conn,
            "layer_test",
            &mut columns,
            Some(false),
            Some(true),
        )
        .expect("drop synthetic columns");

        conn.execute(
            "ALTER TABLE \"layer_test\" RENAME COLUMN \"id \" TO \"id\"",
            [],
        )
        .expect("rename should succeed after dropping synthetic id");
    }
}
