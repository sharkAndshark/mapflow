use std::{path::Path, sync::Arc};

use tokio::sync::Mutex;

pub const DEFAULT_DB_PATH: &str = "./data/mapflow.duckdb";
pub const PROCESSING_RECONCILIATION_ERROR: &str = "Server restarted during processing";

pub async fn reconcile_processing_files(
    db: &Arc<Mutex<duckdb::Connection>>,
) -> Result<usize, duckdb::Error> {
    let conn = db.lock().await;
    conn.execute(
        "UPDATE files SET status = 'failed', error = ? WHERE status = 'processing'",
        duckdb::params![PROCESSING_RECONCILIATION_ERROR],
    )
}

pub fn init_database(db_path: &Path) -> duckdb::Connection {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create database directory");
    }

    let conn = duckdb::Connection::open(db_path).expect("Failed to open database");

    conn.execute_batch("INSTALL spatial; LOAD spatial;")
        .expect("Failed to install and load spatial extension");

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS files (
            id VARCHAR PRIMARY KEY,
            name VARCHAR NOT NULL,
            type VARCHAR NOT NULL,
            size BIGINT NOT NULL,
            uploaded_at TIMESTAMP NOT NULL,
            status VARCHAR NOT NULL,
            crs VARCHAR,
            path VARCHAR NOT NULL,
            table_name VARCHAR,
            error VARCHAR,
            is_public BOOLEAN DEFAULT FALSE,
            tile_source VARCHAR DEFAULT 'duckdb',
            tile_format VARCHAR,
            minzoom INTEGER,
            maxzoom INTEGER,
            tile_bounds VARCHAR,
            mbtiles_path VARCHAR,
            pmtiles_path VARCHAR
        );

        CREATE TABLE IF NOT EXISTS published_files (
            file_id VARCHAR PRIMARY KEY,
            slug VARCHAR UNIQUE NOT NULL,
            published_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            tile_source VARCHAR DEFAULT 'duckdb',
            tile_format VARCHAR,
            minzoom INTEGER,
            maxzoom INTEGER,
            tile_bounds VARCHAR,
            mbtiles_path VARCHAR,
            pmtiles_path VARCHAR,
            FOREIGN KEY (file_id) REFERENCES files(id)
        );
        ",
    )
    .expect("Failed to create files table");

    // Add new columns for existing databases (migration)
    // These ALTER TABLE statements are idempotent
    let _ = conn.execute("ALTER TABLE files ADD COLUMN tile_source VARCHAR DEFAULT 'duckdb'", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN tile_format VARCHAR", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN minzoom INTEGER", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN maxzoom INTEGER", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN tile_bounds VARCHAR", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN mbtiles_path VARCHAR", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN pmtiles_path VARCHAR", []);

    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN tile_source VARCHAR DEFAULT 'duckdb'", []);
    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN tile_format VARCHAR", []);
    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN minzoom INTEGER", []);
    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN maxzoom INTEGER", []);
    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN tile_bounds VARCHAR", []);
    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN mbtiles_path VARCHAR", []);
    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN pmtiles_path VARCHAR", []);

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS dataset_columns (
            source_id VARCHAR NOT NULL,
            normalized_name VARCHAR NOT NULL,
            original_name VARCHAR NOT NULL,
            ordinal BIGINT NOT NULL,
            mvt_type VARCHAR NOT NULL,
            PRIMARY KEY (source_id, normalized_name)
        );

        CREATE INDEX IF NOT EXISTS idx_dataset_columns_source
            ON dataset_columns(source_id);
        ",
    )
    .expect("Failed to create dataset metadata tables");

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS users (
            id VARCHAR PRIMARY KEY,
            username VARCHAR UNIQUE NOT NULL,
            password_hash VARCHAR NOT NULL,
            role VARCHAR NOT NULL,
            created_at TIMESTAMP NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_users_username
            ON users(username);
        ",
    )
    .expect("Failed to create users table");

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS sessions (
            id VARCHAR PRIMARY KEY,
            data VARCHAR NOT NULL,
            expiry_date TIMESTAMP NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_expiry_date
            ON sessions(expiry_date);
        ",
    )
    .expect("Failed to create sessions table");

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS system_settings (
            key VARCHAR PRIMARY KEY,
            value VARCHAR NOT NULL
        );
        ",
    )
    .expect("Failed to create system_settings table");

    conn
}

pub fn is_initialized(conn: &duckdb::Connection) -> Result<bool, duckdb::Error> {
    let mut stmt = conn.prepare(
        "SELECT COUNT(*) FROM system_settings WHERE key = 'initialized' AND value = '1'",
    )?;

    let count: i64 = stmt.query_row([], |row| row.get(0))?;
    Ok(count > 0)
}

pub fn set_initialized(conn: &duckdb::Connection) -> Result<(), duckdb::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO system_settings (key, value) VALUES ('initialized', '1')",
        [],
    )?;
    Ok(())
}
