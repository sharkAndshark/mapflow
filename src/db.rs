use crate::models::{AppError, Result};
use duckdb::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info};

const DB_FILE: &str = "nodes.duckdb";

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new() -> Result<Self> {
        let conn = Self::create_connection()?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn create_connection() -> Result<Connection> {
        let path = Path::new(DB_FILE);
        if path.exists() {
            info!("Connecting to existing database: {}", DB_FILE);
        } else {
            info!("Creating new database: {}", DB_FILE);
        }

        let conn = Connection::open(DB_FILE)?;
        Self::initialize(&conn)?;

        Ok(conn)
    }

    fn initialize(conn: &Connection) -> Result<()> {
        info!("Initializing database schema...");

        conn.execute_batch(
            "
            INSTALL spatial;
            LOAD spatial;
        ",
        )
        .map_err(|e| AppError::Database(format!("Failed to load spatial extension: {}", e)))?;

        info!("Database initialized successfully");
        Ok(())
    }

    pub fn import_shapefile(&self, table_name: &str, shapefile_path: &str) -> Result<()> {
        debug!(
            "Importing shapefile '{}' into table '{}'",
            shapefile_path, table_name
        );

        let conn = self.conn.lock().unwrap();

        // Ensure spatial extension is loaded
        conn.execute("INSTALL spatial;", params![]).map_err(|e| {
            AppError::Database(format!("Failed to install spatial extension: {}", e))
        })?;
        conn.execute("LOAD spatial;", params![])
            .map_err(|e| AppError::Database(format!("Failed to load spatial extension: {}", e)))?;

        // Use st_read (case sensitive in DuckDB)
        let sql = format!(
            "CREATE TABLE {} AS SELECT * FROM st_read('{}')",
            table_name, shapefile_path
        );

        debug!("Executing SQL: {}", sql);

        conn.execute(&sql, params![])
            .map_err(|e| AppError::Database(format!("Failed to import shapefile: {}", e)))?;

        // Create a regular index on geometry column
        let sql = format!(
            "CREATE INDEX idx_{}_geom ON {}(geom)",
            table_name, table_name
        );
        conn.execute(&sql, params![])
            .map_err(|e| AppError::Database(format!("Failed to create spatial index: {}", e)))?;

        info!("Successfully imported shapefile to table '{}'", table_name);
        Ok(())
    }

    pub fn table_exists(&self, table_name: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let sql = "
            SELECT COUNT(*) FROM information_schema.tables 
            WHERE table_name = $1 AND table_schema = 'main'
        ";

        let mut stmt = conn.prepare(sql)?;
        let count: i64 = stmt.query_row(params![table_name], |row| row.get(0))?;

        Ok(count > 0)
    }

    pub fn get_table_info(&self, table_name: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let sql = "
            SELECT column_name FROM information_schema.columns 
            WHERE table_name = $1 AND table_schema = 'main'
            ORDER BY ordinal_position
        ";

        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query(params![table_name])?;

        let mut columns = Vec::new();
        while let Some(row) = rows.next()? {
            let col_name: String = row.get(0)?;
            columns.push(col_name);
        }

        Ok(columns)
    }

    pub fn generate_tile(
        &self,
        z: u32,
        x: u32,
        y: u32,
        table_name: &str,
        fields: &[String],
    ) -> Result<Option<Vec<u8>>> {
        debug!(
            "Generating tile z={}, x={}, y={} from table '{}'",
            z, x, y, table_name
        );

        let conn = self.conn.lock().unwrap();

        let field_list = if fields.is_empty() {
            let columns = self.get_non_geometry_columns(table_name)?;
            columns
                .into_iter()
                .map(|name| format!("\"{}\"", name.replace('\"', "\"\"")))
                .collect::<Vec<_>>()
        } else {
            fields.iter().cloned().collect::<Vec<_>>()
        };

        let select_prefix = if field_list.is_empty() {
            String::new()
        } else {
            format!("{},", field_list.join(","))
        };

        let layer_name = table_name.split('_').next().unwrap_or(table_name);

        let sql = format!(
            "SELECT ST_AsMVT(tile, '{}', 4096, 'geom') 
             FROM (
                 SELECT 
                     {}
                     ST_AsMVTGeom(geom, CAST(ST_TileEnvelope($1, $2, $3) AS BOX_2D), 4096, 64, true) AS geom
                 FROM {}
                 WHERE ST_Intersects(geom, ST_TileEnvelope($1, $2, $3))
             ) AS tile",
            layer_name, select_prefix, table_name
        );

        let mut stmt = conn.prepare(&sql)?;

        match stmt.query_row(params![z as i32, x as i32, y as i32], |row| {
            let tile_data: Option<Vec<u8>> = row.get(0)?;
            Ok(tile_data)
        }) {
            Ok(Some(data)) => {
                info!(
                    "Generated tile z={}, x={}, y={}, size: {} bytes",
                    z,
                    x,
                    y,
                    data.len()
                );
                Ok(Some(data))
            }
            Ok(None) => {
                debug!("No data for tile z={}, x={}, y={}", z, x, y);
                Ok(None)
            }
            Err(e) => {
                error!("Failed to generate tile: {}", e);
                Err(AppError::TileGeneration(format!("Query failed: {}", e)))
            }
        }
    }

    pub fn get_table_bounds(&self, table_name: &str) -> Result<Option<(f64, f64, f64, f64)>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT ST_XMin(geom), ST_YMin(geom), ST_XMax(geom), ST_YMax(geom)
             FROM (SELECT ST_Extent(geom) as geom FROM {}) t",
            table_name
        );

        let mut stmt = conn.prepare(&sql)?;

        match stmt.query_row(params![], |row| {
            let min_x: f64 = row.get(0)?;
            let min_y: f64 = row.get(1)?;
            let max_x: f64 = row.get(2)?;
            let max_y: f64 = row.get(3)?;
            Ok((min_x, min_y, max_x, max_y))
        }) {
            Ok(bounds) => Ok(Some(bounds)),
            Err(_) => Ok(None),
        }
    }

    fn get_non_geometry_columns(&self, table_name: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let sql = "
            SELECT column_name, data_type FROM information_schema.columns 
            WHERE table_name = $1 AND table_schema = 'main'
            ORDER BY ordinal_position
        ";

        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query(params![table_name])?;

        let mut columns = Vec::new();
        while let Some(row) = rows.next()? {
            let col_name: String = row.get(0)?;
            let data_type: String = row.get(1)?;
            if data_type.to_uppercase() != "GEOMETRY" {
                columns.push(col_name);
            }
        }

        Ok(columns)
    }

    pub fn close(&self) {
        debug!("Closing database connection");
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_database_creation() {
        let test_db = "test_nodes.duckdb";
        if Path::new(test_db).exists() {
            fs::remove_file(test_db).unwrap();
        }

        let db = Database::new();
        assert!(db.is_ok());

        if Path::new(test_db).exists() {
            fs::remove_file(test_db).unwrap();
        }
    }

    #[test]
    fn test_table_operations() {
        let db = Database::new().unwrap();

        let exists = db.table_exists("nonexistent");
        assert!(exists.is_ok());
        assert!(!exists.unwrap());

        let info = db.get_table_info("nonexistent");
        assert!(info.is_ok());
    }
}
