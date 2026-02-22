#[cfg(feature = "embed-spatial-extension")]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::Duration,
};

use tokio::sync::Mutex;

pub const DEFAULT_DB_PATH: &str = "./data/mapflow.duckdb";
pub const PROCESSING_RECONCILIATION_ERROR: &str = "Server restarted during processing";
const SPATIAL_INSTALL_MAX_ATTEMPTS: u32 = 5;
const SPATIAL_INSTALL_RETRY_BASE_MS: u64 = 250;
const SPATIAL_EXTENSION_PATH_ENV: &str = "SPATIAL_EXTENSION_PATH";
const SPATIAL_EXTENSION_DIR_ENV: &str = "SPATIAL_EXTENSION_DIR";
const SPATIAL_EXTENSION_FILENAME: &str = "spatial.duckdb_extension";
const DEFAULT_SPATIAL_EXTENSION_RELATIVE_PATH: &str = "extensions/spatial.duckdb_extension";
const DEV_SPATIAL_EXTENSION_RELATIVE_PATH: &str = "backend/extensions/spatial.duckdb_extension";
#[cfg(feature = "embed-spatial-extension")]
const SPATIAL_EXTENSION_CACHE_DIR_ENV: &str = "SPATIAL_EXTENSION_CACHE_DIR";

static SPATIAL_INSTALL_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
#[cfg(feature = "embed-spatial-extension")]
static EMBEDDED_SPATIAL_EXTENSION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/extensions/spatial.duckdb_extension"
));

fn open_with_wal_recovery(db_path: &Path) -> duckdb::Connection {
    match duckdb::Connection::open(db_path) {
        Ok(conn) => conn,
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("replaying WAL") || err_str.contains("WAL file") {
                let wal_path = db_path.with_extension("duckdb.wal");
                tracing::warn!(
                    wal_path = %wal_path.display(),
                    error = %err_str,
                    "Corrupt WAL detected, removing and retrying"
                );
                if let Err(remove_err) = std::fs::remove_file(&wal_path) {
                    tracing::error!(
                        wal_path = %wal_path.display(),
                        error = %remove_err,
                        "Failed to remove corrupt WAL file"
                    );
                }
                duckdb::Connection::open(db_path)
                    .expect("Failed to open database after WAL cleanup")
            } else {
                panic!("Failed to open database: {}", e);
            }
        }
    }
}

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

    let conn = open_with_wal_recovery(db_path);

    ensure_spatial_extension(&conn).expect("Failed to install and load spatial extension");

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
            tile_format VARCHAR,
            minzoom INTEGER,
            maxzoom INTEGER,
            tile_bounds VARCHAR
        );

        CREATE TABLE IF NOT EXISTS published_files (
            file_id VARCHAR PRIMARY KEY,
            slug VARCHAR UNIQUE NOT NULL,
            published_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            minzoom INTEGER,
            maxzoom INTEGER,
            FOREIGN KEY (file_id) REFERENCES files(id)
        );
        ",
    )
    .expect("Failed to create files table");

    // Add new columns for MBTiles support (if they don't exist)
    // These ALTER TABLE statements are idempotent - they will fail silently if columns exist
    let _ = conn.execute("ALTER TABLE files ADD COLUMN tile_format VARCHAR", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN minzoom INTEGER", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN maxzoom INTEGER", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN tile_bounds VARCHAR", []);
    let _ = conn.execute(
        "ALTER TABLE files ADD COLUMN tile_source VARCHAR DEFAULT 'duckdb'",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE published_files ADD COLUMN tile_source VARCHAR DEFAULT 'duckdb'",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE files ADD COLUMN crs_type VARCHAR DEFAULT 'standard'",
        [],
    );
    let _ = conn.execute("ALTER TABLE files ADD COLUMN data_bounds VARCHAR", []);
    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN minzoom INTEGER", []);
    let _ = conn.execute("ALTER TABLE published_files ADD COLUMN maxzoom INTEGER", []);

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

    // Add alias column to dataset_columns (if it doesn't exist)
    let _ = conn.execute("ALTER TABLE dataset_columns ADD COLUMN alias VARCHAR", []);

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

fn append_unique_path(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|existing| existing == &path) {
        candidates.push(path);
    }
}

#[cfg(feature = "embed-spatial-extension")]
fn build_embedded_spatial_filename() -> String {
    // Keep the filename deterministic so multiple restarts reuse the same extracted artifact.
    let checksum = EMBEDDED_SPATIAL_EXTENSION.iter().fold(0u64, |acc, byte| {
        acc.wrapping_mul(16777619).wrapping_add(u64::from(*byte))
    });
    format!(
        "spatial-{}-{:016x}.duckdb_extension",
        EMBEDDED_SPATIAL_EXTENSION.len(),
        checksum
    )
}

#[cfg(feature = "embed-spatial-extension")]
fn embedded_spatial_extension_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(dir) = std::env::var(SPATIAL_EXTENSION_CACHE_DIR_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        append_unique_path(&mut dirs, PathBuf::from(dir));
    }

    #[cfg(target_os = "windows")]
    if let Some(local_app_data) = std::env::var("LOCALAPPDATA")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        append_unique_path(&mut dirs, PathBuf::from(local_app_data).join("mapflow"));
    }

    #[cfg(not(target_os = "windows"))]
    if let Some(xdg_cache_home) = std::env::var("XDG_CACHE_HOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        append_unique_path(&mut dirs, PathBuf::from(xdg_cache_home).join("mapflow"));
    }

    #[cfg(not(target_os = "windows"))]
    if let Some(home) = std::env::var("HOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        append_unique_path(
            &mut dirs,
            PathBuf::from(home).join(".cache").join("mapflow"),
        );
    }

    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        append_unique_path(&mut dirs, exe_dir);
    }

    append_unique_path(&mut dirs, std::env::temp_dir().join("mapflow"));
    dirs
}

#[cfg(feature = "embed-spatial-extension")]
fn write_embedded_spatial_extension(path: &Path) -> Result<(), String> {
    let expected_len = EMBEDDED_SPATIAL_EXTENSION.len() as u64;
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.is_file() && metadata.len() == expected_len {
            return Ok(());
        }
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("Missing parent directory for {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let tmp_path = parent.join(format!(".{SPATIAL_EXTENSION_FILENAME}.{nonce}.tmp"));

    std::fs::write(&tmp_path, EMBEDDED_SPATIAL_EXTENSION)
        .map_err(|e| format!("Failed to write {}: {}", tmp_path.display(), e))?;

    match std::fs::rename(&tmp_path, path) {
        Ok(_) => Ok(()),
        Err(rename_err) => {
            if let Ok(metadata) = std::fs::metadata(path) {
                if metadata.is_file() && metadata.len() == expected_len {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Ok(());
                }
            }
            if std::fs::remove_file(path).is_ok() && std::fs::rename(&tmp_path, path).is_ok() {
                return Ok(());
            }
            let _ = std::fs::remove_file(&tmp_path);
            Err(format!(
                "Failed to move embedded extension into place at {}: {}",
                path.display(),
                rename_err
            ))
        }
    }
}

#[cfg(feature = "embed-spatial-extension")]
fn resolve_embedded_spatial_extension_candidate() -> Result<PathBuf, String> {
    let file_name = build_embedded_spatial_filename();
    let base_dirs = embedded_spatial_extension_directories();
    let mut errors = Vec::new();
    for base_dir in &base_dirs {
        let target_path = base_dir.join("extensions").join(&file_name);
        match write_embedded_spatial_extension(&target_path) {
            Ok(_) => return Ok(target_path),
            Err(e) => errors.push(format!("{} ({})", target_path.display(), e)),
        }
    }

    let attempted_dirs = base_dirs
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    Err(format!(
        "Unable to materialize embedded spatial extension (tried: {}): {}",
        attempted_dirs,
        errors.join(" | ")
    ))
}

fn resolve_local_spatial_extension_candidates(
    env_path: Option<&str>,
    env_dir: Option<&str>,
    embedded_path: Option<&Path>,
    cwd: Option<&Path>,
    exe_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = env_path.map(str::trim).filter(|value| !value.is_empty()) {
        append_unique_path(&mut candidates, PathBuf::from(path));
    }

    if let Some(dir) = env_dir.map(str::trim).filter(|value| !value.is_empty()) {
        append_unique_path(
            &mut candidates,
            PathBuf::from(dir).join(SPATIAL_EXTENSION_FILENAME),
        );
    }

    if let Some(path) = embedded_path {
        append_unique_path(&mut candidates, path.to_path_buf());
    }

    if let Some(dir) = exe_dir {
        append_unique_path(
            &mut candidates,
            dir.join(DEFAULT_SPATIAL_EXTENSION_RELATIVE_PATH),
        );
        append_unique_path(&mut candidates, dir.join(SPATIAL_EXTENSION_FILENAME));
    }

    if let Some(dir) = cwd {
        append_unique_path(
            &mut candidates,
            dir.join(DEFAULT_SPATIAL_EXTENSION_RELATIVE_PATH),
        );
        append_unique_path(
            &mut candidates,
            dir.join(DEV_SPATIAL_EXTENSION_RELATIVE_PATH),
        );
    }

    candidates
}

fn local_spatial_extension_candidates() -> Vec<PathBuf> {
    let env_path = std::env::var(SPATIAL_EXTENSION_PATH_ENV).ok();
    let env_dir = std::env::var(SPATIAL_EXTENSION_DIR_ENV).ok();
    #[cfg(feature = "embed-spatial-extension")]
    let embedded_path = resolve_embedded_spatial_extension_candidate()
        .map_err(|error| {
            tracing::warn!(error = %error, "Embedded spatial extension unavailable");
            error
        })
        .ok();
    #[cfg(not(feature = "embed-spatial-extension"))]
    let embedded_path: Option<PathBuf> = None;
    let cwd = std::env::current_dir().ok();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    resolve_local_spatial_extension_candidates(
        env_path.as_deref(),
        env_dir.as_deref(),
        embedded_path.as_deref(),
        cwd.as_deref(),
        exe_dir.as_deref(),
    )
}

fn find_existing_local_spatial_extension_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|path| path.is_file())
        .map(PathBuf::from)
}

fn build_load_extension_sql(path: &Path) -> Result<String, String> {
    let raw_path = path
        .to_str()
        .ok_or_else(|| format!("Extension path is not valid UTF-8: {}", path.display()))?;
    let escaped = escape_sql_string(raw_path);
    Ok(format!("LOAD '{}';", escaped))
}

/// Escapes single quotes in a string for use in DuckDB SQL string literals.
/// Converts `'` to `''` which is the SQL standard escaping mechanism.
/// This is sufficient for DuckDB which does not interpret backslashes in strings.
pub fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

fn try_load_spatial_from_path(conn: &duckdb::Connection, path: &Path) -> Result<(), String> {
    let load_sql = build_load_extension_sql(path)?;
    conn.execute_batch(&load_sql).map_err(|e| {
        format!(
            "Failed to load spatial extension from {}: {}",
            path.display(),
            e
        )
    })
}

pub fn ensure_spatial_extension(conn: &duckdb::Connection) -> Result<(), String> {
    let local_candidates = local_spatial_extension_candidates();
    let mut errors: Vec<String> = Vec::with_capacity(SPATIAL_INSTALL_MAX_ATTEMPTS as usize + 2);

    // Local-first path: prefer explicit/local/embedded extension files when available.
    if let Some(local_path) = find_existing_local_spatial_extension_path(&local_candidates) {
        match try_load_spatial_from_path(conn, &local_path) {
            Ok(_) => return Ok(()),
            Err(error) => errors.push(error),
        }
    }

    // Fast path fallback: extension already installed in local DuckDB cache.
    if conn.execute_batch("LOAD spatial;").is_ok() {
        return Ok(());
    }

    // Prevent concurrent install attempts in the same process from hammering the extension endpoint.
    let lock = SPATIAL_INSTALL_LOCK.get_or_init(|| StdMutex::new(()));
    let _guard = lock
        .lock()
        .map_err(|_| "Failed to acquire spatial extension install lock".to_string())?;

    if let Some(local_path) = find_existing_local_spatial_extension_path(&local_candidates) {
        match try_load_spatial_from_path(conn, &local_path) {
            Ok(_) => return Ok(()),
            Err(error) => errors.push(format!("post-lock {}", error)),
        }
    }

    // Another thread may have completed install while we were waiting.
    if conn.execute_batch("LOAD spatial;").is_ok() {
        return Ok(());
    }

    for attempt in 1..=SPATIAL_INSTALL_MAX_ATTEMPTS {
        match conn.execute_batch("INSTALL spatial;") {
            Ok(_) => match conn.execute_batch("LOAD spatial;") {
                Ok(_) => return Ok(()),
                Err(e) => errors.push(format!(
                    "attempt {}: install ok, load failed: {}",
                    attempt, e
                )),
            },
            Err(e) => {
                // INSTALL may fail on transient HTTP errors even when extension already exists locally.
                // Retry a plain LOAD first before sleeping.
                if conn.execute_batch("LOAD spatial;").is_ok() {
                    return Ok(());
                }
                errors.push(format!("attempt {}: install failed: {}", attempt, e));
            }
        }

        if attempt < SPATIAL_INSTALL_MAX_ATTEMPTS {
            std::thread::sleep(Duration::from_millis(
                SPATIAL_INSTALL_RETRY_BASE_MS * attempt as u64,
            ));
        }
    }

    Err(format!(
        "Unable to install/load DuckDB spatial extension after {} attempts. {}",
        SPATIAL_INSTALL_MAX_ATTEMPTS,
        errors.join(" | ")
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_candidates_prefers_explicit_env_path() {
        let cwd = Path::new("/workspace/mapflow");
        let exe = Path::new("/opt/mapflow");
        let candidates = resolve_local_spatial_extension_candidates(
            Some("/tmp/custom/spatial.duckdb_extension"),
            Some("/tmp/custom-dir"),
            None,
            Some(cwd),
            Some(exe),
        );

        assert_eq!(
            candidates[0],
            PathBuf::from("/tmp/custom/spatial.duckdb_extension")
        );
        assert_eq!(
            candidates[1],
            PathBuf::from("/tmp/custom-dir").join(SPATIAL_EXTENSION_FILENAME)
        );
    }

    #[test]
    fn resolve_candidates_places_embedded_after_explicit_env_candidates() {
        let cwd = Path::new("/workspace/mapflow");
        let exe = Path::new("/opt/mapflow");
        let embedded = Path::new("/tmp/cache/spatial-embedded.duckdb_extension");
        let candidates = resolve_local_spatial_extension_candidates(
            Some("/tmp/custom/spatial.duckdb_extension"),
            Some("/tmp/custom-dir"),
            Some(embedded),
            Some(cwd),
            Some(exe),
        );

        assert_eq!(
            candidates[0],
            PathBuf::from("/tmp/custom/spatial.duckdb_extension")
        );
        assert_eq!(
            candidates[1],
            PathBuf::from("/tmp/custom-dir").join(SPATIAL_EXTENSION_FILENAME)
        );
        assert_eq!(candidates[2], embedded);
    }

    #[test]
    fn find_existing_path_picks_first_existing_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing = temp.path().join("missing.duckdb_extension");
        let first = temp.path().join("first.duckdb_extension");
        let second = temp.path().join("second.duckdb_extension");
        std::fs::write(&first, b"fake").expect("write first");
        std::fs::write(&second, b"fake").expect("write second");

        let candidates = vec![missing, first.clone(), second];
        let found = find_existing_local_spatial_extension_path(&candidates).expect("found");
        assert_eq!(found, first);
    }

    #[test]
    fn build_load_extension_sql_escapes_single_quotes() {
        let path = Path::new("/tmp/mapflow's/spatial.duckdb_extension");
        let sql = build_load_extension_sql(path).expect("sql");
        assert_eq!(sql, "LOAD '/tmp/mapflow''s/spatial.duckdb_extension';");
    }

    #[test]
    fn escape_sql_string_escapes_single_quotes() {
        assert_eq!(escape_sql_string("normal_path"), "normal_path");
        assert_eq!(escape_sql_string("path'with'quotes"), "path''with''quotes");
        assert_eq!(escape_sql_string("user's data/file"), "user''s data/file");
        // Edge cases
        assert_eq!(escape_sql_string(""), ""); // empty string
        assert_eq!(escape_sql_string("'"), "''"); // only single quote
        assert_eq!(escape_sql_string("'''"), "''''''"); // multiple consecutive quotes
        assert_eq!(escape_sql_string("no quotes here"), "no quotes here"); // normal path with spaces
    }

    #[test]
    fn open_with_wal_recovery_removes_corrupt_wal_and_succeeds() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("test.duckdb");

        let conn = duckdb::Connection::open(&db_path).expect("open");
        conn.execute("CREATE TABLE test (id INTEGER)", [])
            .expect("create table");
        conn.execute("INSERT INTO test VALUES (1)", [])
            .expect("insert");
        drop(conn);

        let wal_path = db_path.with_extension("duckdb.wal");
        std::fs::write(&wal_path, b"corrupted wal data").expect("write corrupt wal");

        let conn = open_with_wal_recovery(&db_path);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM test", [], |r| r.get(0))
            .expect("query");
        assert_eq!(count, 1);
    }
}
