use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

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
        // Use /vsizip/ prefix for GDAL to read directly from zip
        format!("/vsizip/{}", abs_path)
    } else {
        abs_path
    };

    let conn = db.lock().await;

    // 1. Detect CRS using ST_Read_Meta
    // layers[1].geometry_fields[1].crs.auth_name / auth_code
    // Note: ST_Read_Meta return structure depends on the file.
    // We try to get the first layer's CRS.
    // List indexing in DuckDB is 1-based.
    let crs_query = format!(
        "SELECT 
            layers[1].geometry_fields[1].crs.auth_name || ':' || layers[1].geometry_fields[1].crs.auth_code 
         FROM ST_Read_Meta('{abs_path}')"
    );

    let detected_crs: Option<String> = conn.query_row(&crs_query, [], |row| row.get(0)).ok();

    // Update files table with detected CRS
    if let Some(crs) = &detected_crs {
        let _ = conn.execute(
            "UPDATE files SET crs = ? WHERE id = ?",
            duckdb::params![crs, source_id],
        );
    }

    // 2. Import Data into a per-dataset table (layer_<id>) so we can preserve columns.
    // We keep a stable feature id column (fid) for MVT feature ids.
    let table_name = format!("layer_{}", source_id);
    let safe_table_name =
        normalize_column_name(&table_name).unwrap_or_else(|| format!("layer_{}", source_id));

    // Drop if exists (id collision should be impossible, but keep idempotent).
    let _ = conn.execute(&format!("DROP TABLE IF EXISTS \"{safe_table_name}\""), []);

    let create_sql = format!(
        "CREATE TABLE \"{safe_table_name}\" AS\n         SELECT row_number() OVER ()::BIGINT AS fid, *\n         FROM ST_Read('{abs_path}')"
    );

    conn.execute(&create_sql, [])
        .map_err(|e| format!("Spatial import failed: {}", e))?;

    // Record table name on the file record.
    let _ = conn.execute(
        "UPDATE files SET table_name = ? WHERE id = ?",
        duckdb::params![safe_table_name.as_str(), source_id],
    );

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
                "INSERT INTO dataset_columns (source_id, normalized_name, original_name, ordinal, mvt_type)\n                 VALUES (?1, ?2, ?3, ?4, ?5)",
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
