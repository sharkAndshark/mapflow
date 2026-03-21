use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

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
        "CREATE TABLE \"{safe_table_name}\" AS\n         SELECT row_number() OVER ()::BIGINT AS fid, *\n         FROM ST_Read('{escaped_path}')"
    );

    execute_create_with_retry(&conn, &create_sql, source_id)?;

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

    for (name, data_type, ordinal) in &columns {
        let lower = name.to_ascii_lowercase();

        // GDAL readers may expose source-side feature ids (e.g. OGC_FID).
        // We already maintain our own stable `fid`, so skip these metadata columns.
        if lower == "ogc_fid" {
            continue;
        }

        // DuckDB/GDAL may expose a synthetic feature-id `id` column for GeoJSON reads.
        // If that column is entirely NULL, skip it so schema/properties stay stable.
        if lower == "id" && is_all_null_column(&conn, &safe_table_name, name)? {
            continue;
        }

        let is_reserved = lower == "fid" || lower == "geom";

        // Determine normalized name.
        let mut normalized = if is_reserved {
            lower.clone()
        } else if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && (name
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false))
        {
            // Keep as-is but lowercase to match DuckDB identifier behavior.
            lower.clone()
        } else {
            normalize_column_name(name).unwrap_or_else(|| format!("col_{ordinal}"))
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
        let mvt_type = if lower == "geom" {
            "GEOMETRY".to_string()
        } else if lower == "fid" {
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

        if lower != "geom" && lower != "fid" {
            // Record property columns (exclude geom + fid).
            let _ = conn.execute(
                "INSERT INTO dataset_columns (source_id, normalized_name, original_name, alias, ordinal, mvt_type)\n                 VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
                duckdb::params![
                    source_id,
                    normalized.as_str(),
                    name.as_str(),
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
    use super::is_retryable_st_read_error;

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
}
