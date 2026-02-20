use duckdb::Connection;

use crate::crs::{calculate_custom_tile_bbox, DataBounds, CRS_TYPE_CUSTOM};

pub struct TileParams {
    pub source_crs: String,
    pub crs_type: String,
    pub data_bounds: Option<DataBounds>,
}

#[derive(Debug)]
pub struct TileError(pub String);

impl std::fmt::Display for TileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TileError {}

impl From<duckdb::Error> for TileError {
    fn from(e: duckdb::Error) -> Self {
        TileError(e.to_string())
    }
}

pub fn build_mvt_select_sql(
    conn: &Connection,
    source_id: &str,
    table_name: &str,
    params: &TileParams,
    z: i32,
    x: i32,
    y: i32,
) -> Result<String, TileError> {
    if params.crs_type == CRS_TYPE_CUSTOM {
        match &params.data_bounds {
            None => {
                return Err(TileError(
                    "Custom CRS requires data_bounds for tile generation".to_string(),
                ));
            }
            Some(bounds) if !bounds.is_valid() => {
                return Err(TileError(
                    "Custom CRS has invalid data_bounds (zero or negative extent)".to_string(),
                ));
            }
            _ => {}
        }
    }

    let mut props_stmt = conn
        .prepare(
            "SELECT normalized_name, original_name, alias\n         FROM dataset_columns\n         WHERE source_id = ?\n         ORDER BY ordinal",
        )
        .map_err(|e| TileError(format!("Failed to prepare column query: {}", e)))?;

    let props_iter = props_stmt
        .query_map(duckdb::params![source_id], |row| {
            let normalized: String = row.get(0)?;
            let original: String = row.get(1)?;
            let alias: Option<String> = row.get(2)?;
            Ok((normalized, original, alias))
        })
        .map_err(|e| TileError(format!("Failed to query columns: {}", e)))?;

    let mut struct_fields = Vec::new();

    if params.crs_type == CRS_TYPE_CUSTOM {
        let bounds = params.data_bounds.as_ref().unwrap();
        let (minx, miny, maxx, maxy) = calculate_custom_tile_bbox(bounds, z, x, y);
        struct_fields.push(format!(
            "geom := ST_AsMVTGeom(\n                    geom,\n                    ST_MakeBox2D(ST_Point({minx}, {miny}), ST_Point({maxx}, {maxy})),\n                    4096, 256, true\n                )"
        ));
    } else {
        let source_crs = &params.source_crs;
        struct_fields.push(format!(
            "geom := ST_AsMVTGeom(\n                    ST_Transform(geom, '{source_crs}', 'EPSG:3857', always_xy := true),\n                    ST_Extent(ST_TileEnvelope(?, ?, ?)),\n                    4096, 256, true\n                )"
        ));
    }

    struct_fields.push("fid := fid".to_string());

    for entry in props_iter {
        let (normalized, original, alias) = entry?;
        let display_name = alias.unwrap_or(original);
        let key = display_name.replace('"', "\"\"");
        struct_fields.push(format!("\"{key}\" := \"{normalized}\""));
    }

    let struct_expr = format!(
        "struct_pack(\n                {}\n            )",
        struct_fields.join(",\n                ")
    );

    if params.crs_type == CRS_TYPE_CUSTOM {
        Ok(format!(
            "SELECT ST_AsMVT(feature, 'layer', 4096, 'geom', 'fid') FROM (\n                SELECT {struct_expr} as feature\n                FROM \"{table_name}\"\n                WHERE ST_Intersects(\n                    geom,\n                    ST_MakeEnvelope(?, ?, ?, ?)\n                )\n            )"
        ))
    } else {
        let source_crs = &params.source_crs;
        Ok(format!(
            "SELECT ST_AsMVT(feature, 'layer', 4096, 'geom', 'fid') FROM (\n                SELECT {struct_expr} as feature\n                FROM \"{table_name}\"\n                WHERE ST_Intersects(\n                    ST_Transform(geom, '{source_crs}', 'EPSG:3857', always_xy := true),\n                    ST_TileEnvelope(?, ?, ?)\n                )\n            )"
        ))
    }
}

pub fn build_mvt_query_params(
    params: &TileParams,
    z: i32,
    x: i32,
    y: i32,
) -> Vec<Box<dyn duckdb::ToSql>> {
    if params.crs_type == CRS_TYPE_CUSTOM {
        let bounds = params
            .data_bounds
            .as_ref()
            .expect("data_bounds required for custom CRS");
        let (minx, miny, maxx, maxy) = calculate_custom_tile_bbox(bounds, z, x, y);
        vec![
            Box::new(minx),
            Box::new(miny),
            Box::new(maxx),
            Box::new(maxy),
        ]
    } else {
        vec![
            Box::new(z),
            Box::new(x),
            Box::new(y),
            Box::new(z),
            Box::new(x),
            Box::new(y),
        ]
    }
}
