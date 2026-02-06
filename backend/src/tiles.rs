use duckdb::Connection;

pub fn build_mvt_select_sql(
    conn: &Connection,
    source_id: &str,
    table_name: &str,
    source_crs: &str,
) -> Result<String, duckdb::Error> {
    // Build property struct keys based on captured column metadata.
    // We keep property keys as original names for UX.
    // Note: We exclude fid + geom.
    let mut props_stmt = conn.prepare(
        "SELECT normalized_name, original_name\n         FROM dataset_columns\n         WHERE source_id = ?\n         ORDER BY ordinal",
    )?;
    let props_iter = props_stmt.query_map(duckdb::params![source_id], |row| {
        let normalized: String = row.get(0)?;
        let original: String = row.get(1)?;
        Ok((normalized, original))
    })?;

    let mut struct_fields = Vec::new();
    struct_fields.push(format!(
        "geom := ST_AsMVTGeom(\n                    ST_Transform(geom, '{source_crs}', 'EPSG:3857', always_xy := true),\n                    ST_Extent(ST_TileEnvelope(?, ?, ?)),\n                    4096, 256, true\n                )"
    ));
    struct_fields.push("fid := fid".to_string());

    for entry in props_iter {
        let (normalized, original) = entry?;

        // Use the original column name as the MVT property key.
        // DuckDB `struct_pack` uses identifier keys; quoted identifiers allow spaces/symbols.
        // Escape embedded double quotes per SQL identifier rules.
        let key = original.replace('"', "\"\"");
        struct_fields.push(format!("\"{key}\" := \"{normalized}\""));
    }

    let struct_expr = format!(
        "struct_pack(\n                {}\n            )",
        struct_fields.join(",\n                ")
    );

    Ok(format!(
        "SELECT ST_AsMVT(feature, 'layer', 4096, 'geom', 'fid') FROM (\n            SELECT {struct_expr} as feature\n            FROM \"{table_name}\"\n            WHERE ST_Intersects(\n                ST_Transform(geom, '{source_crs}', 'EPSG:3857', always_xy := true),\n                ST_TileEnvelope(?, ?, ?)\n            )\n        )"
    ))
}
