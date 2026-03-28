#[cfg(all(unix, feature = "embed-spatial-extension"))]
use std::os::unix::fs::PermissionsExt;

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use duckdb::OptionalExt;
use tokio::sync::Mutex;

pub const DEFAULT_DB_PATH: &str = "./data/mapflow.duckdb";
pub const PROCESSING_RECONCILIATION_ERROR: &str = "Server restarted during processing";

const SPATIAL_EXTENSION_PATH_ENV: &str = "SPATIAL_EXTENSION_PATH";
const SPATIAL_EXTENSION_DIR_ENV: &str = "SPATIAL_EXTENSION_DIR";
const SPATIAL_EXTENSION_FILENAME: &str = "spatial.duckdb_extension";
const DEFAULT_SPATIAL_EXTENSION_RELATIVE_PATH: &str = "extensions/spatial.duckdb_extension";
const DEV_SPATIAL_EXTENSION_RELATIVE_PATH: &str = "backend/extensions/spatial.duckdb_extension";
const WAL_RECOVERY_STRICT_ENV: &str = "WAL_RECOVERY_STRICT";

#[cfg(feature = "embed-spatial-extension")]
const SPATIAL_EXTENSION_CACHE_DIR_ENV: &str = "SPATIAL_EXTENSION_CACHE_DIR";

#[cfg(feature = "embed-spatial-extension")]
static EMBEDDED_SPATIAL_EXTENSION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/extensions/spatial.duckdb_extension"
));

fn open_with_wal_recovery(db_path: &Path) -> Result<duckdb::Connection, String> {
    match duckdb::Connection::open(db_path) {
        Ok(conn) => Ok(conn),
        Err(e) => {
            let err_str = e.to_string();
            if !is_wal_related_open_error(&err_str) {
                return Err(format!(
                    "DB open failed with non-WAL error: {}; db_path={}",
                    err_str,
                    db_path.display()
                ));
            }

            if wal_recovery_strict_mode() {
                return Err(format!(
                    "WAL recovery strict mode enabled; refusing automatic WAL isolation. \
                     db_path={}, error={}",
                    db_path.display(),
                    err_str
                ));
            }

            recover_after_wal_open_error(db_path, &err_str)
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

pub async fn reconcile_processing_fonts(
    db: &Arc<Mutex<duckdb::Connection>>,
) -> Result<usize, duckdb::Error> {
    let conn = db.lock().await;
    conn.execute(
        "UPDATE fonts SET status = 'failed', error = ? WHERE status = 'processing'",
        duckdb::params![PROCESSING_RECONCILIATION_ERROR],
    )
}

fn ensure_workspace_schema_and_backfill(conn: &duckdb::Connection) {
    let _ = conn.execute(
        "ALTER TABLE users ADD COLUMN current_workspace_id VARCHAR",
        [],
    );
    let _ = conn.execute("ALTER TABLE files ADD COLUMN workspace_id VARCHAR", []);
    let _ = conn.execute("ALTER TABLE workspaces ADD COLUMN slug VARCHAR", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_files_workspace ON files(workspace_id)",
        [],
    );
    let _ = conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_workspaces_slug ON workspaces(slug)",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE fonts ADD COLUMN is_public BOOLEAN DEFAULT FALSE",
        [],
    );
    let _ = conn.execute("ALTER TABLE fonts ADD COLUMN slug VARCHAR", []);
    let _ = conn.execute("ALTER TABLE fonts ADD COLUMN published_at TIMESTAMP", []);
    let _ = conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_fonts_workspace_slug ON fonts(workspace_id, slug)",
        [],
    );
    let _ = conn.execute("DROP INDEX IF EXISTS idx_fonts_workspace_fontstack", []);

    recover_detached_workspace_members(conn).expect("Failed to recover detached workspace members");
    backfill_workspace_data(conn).expect("Failed to backfill workspace data");
}

fn recover_detached_workspace_members(conn: &duckdb::Connection) -> Result<(), duckdb::Error> {
    let backup_rows = {
        let mut stmt =
            conn.prepare("SELECT workspace_id, user_id, joined_at FROM workspace_member_backups")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for (workspace_id, user_id, joined_at) in &backup_rows {
        conn.execute(
            "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
            duckdb::params![workspace_id, user_id, joined_at],
        )?;
    }

    if !backup_rows.is_empty() {
        conn.execute("DELETE FROM workspace_member_backups", [])?;
    }

    Ok(())
}

fn backfill_workspace_data(conn: &duckdb::Connection) -> Result<(), duckdb::Error> {
    let user_rows = {
        let mut stmt = conn.prepare(
            "SELECT id, username, created_at FROM users WHERE id NOT IN (SELECT owner_id FROM workspaces)",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for (user_id, username, created_at) in user_rows {
        let workspace_id = format!("personal-{}", user_id);
        let workspace_name = unique_workspace_name(
            conn,
            &crate::workspace::make_personal_workspace_name(&username),
            &workspace_id,
        )?;
        let workspace_slug = unique_workspace_slug(conn, &workspace_name, &workspace_id)?;

        conn.execute(
            "INSERT INTO workspaces (id, name, slug, owner_id, is_personal, created_at) VALUES (?, ?, ?, ?, TRUE, ?)",
            duckdb::params![&workspace_id, &workspace_name, &workspace_slug, &user_id, &created_at],
        )?;
        conn.execute(
            "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?)",
            duckdb::params![&workspace_id, &user_id, &created_at],
        )?;
    }

    let orphan_members = {
        let mut stmt = conn.prepare(
            r"
            SELECT w.id, w.owner_id, w.created_at
            FROM workspaces w
            LEFT JOIN workspace_members wm
              ON wm.workspace_id = w.id AND wm.user_id = w.owner_id
            WHERE wm.user_id IS NULL
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for (workspace_id, owner_id, created_at) in orphan_members {
        conn.execute(
            "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?)",
            duckdb::params![&workspace_id, &owner_id, &created_at],
        )?;
    }

    let legacy_file_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE workspace_id IS NULL",
        [],
        |row| row.get(0),
    )?;

    let legacy_shared_workspace_id = if legacy_file_count > 0 {
        let shared_workspace_id = "legacy-shared-workspace".to_string();
        let owner_row: Option<(String, String)> = conn
            .query_row(
                "SELECT id, created_at FROM users ORDER BY created_at ASC, id ASC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        if let Some((owner_id, created_at)) = owner_row {
            let exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM workspaces WHERE id = ?",
                duckdb::params![&shared_workspace_id],
                |row| row.get(0),
            )?;

            if exists == 0 {
                let workspace_name = unique_workspace_name(
                    conn,
                    crate::workspace::make_legacy_shared_workspace_name(),
                    &shared_workspace_id,
                )?;
                let workspace_slug =
                    unique_workspace_slug(conn, &workspace_name, &shared_workspace_id)?;
                conn.execute(
                    "INSERT INTO workspaces (id, name, slug, owner_id, is_personal, created_at) VALUES (?, ?, ?, ?, FALSE, ?)",
                    duckdb::params![&shared_workspace_id, &workspace_name, &workspace_slug, &owner_id, &created_at],
                )?;
            }

            let all_user_rows = {
                let mut stmt = conn.prepare("SELECT id, created_at FROM users")?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            for (user_id, joined_at) in all_user_rows {
                let member_exists: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
                    duckdb::params![&shared_workspace_id, &user_id],
                    |row| row.get(0),
                )?;
                if member_exists == 0 {
                    conn.execute(
                        "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, ?)",
                        duckdb::params![&shared_workspace_id, &user_id, &joined_at],
                    )?;
                }
            }

            conn.execute(
                "UPDATE files SET workspace_id = ? WHERE workspace_id IS NULL",
                duckdb::params![&shared_workspace_id],
            )?;

            Some(shared_workspace_id)
        } else {
            None
        }
    } else {
        None
    };

    let user_workspace_rows = {
        let mut stmt = conn.prepare(
            r"
            SELECT u.id,
                   u.current_workspace_id,
                   (
                     SELECT w.id
                     FROM workspaces w
                     JOIN workspace_members wm ON wm.workspace_id = w.id
                     WHERE wm.user_id = u.id AND w.deleted_at IS NULL
                     ORDER BY w.is_personal DESC, w.created_at ASC
                     LIMIT 1
                   ) AS fallback_workspace_id
            FROM users u
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for (user_id, current_workspace_id, fallback_workspace_id) in user_workspace_rows {
        let target_workspace_id = match (&legacy_shared_workspace_id, fallback_workspace_id) {
            (Some(shared_workspace_id), _) => shared_workspace_id.clone(),
            (None, Some(fallback_workspace_id)) => fallback_workspace_id,
            (None, None) => continue,
        };

        let current_is_valid = if let Some(current_workspace_id) = current_workspace_id {
            let count: i64 = conn.query_row(
                r"
                SELECT COUNT(*)
                FROM workspaces w
                JOIN workspace_members wm ON wm.workspace_id = w.id
                WHERE w.id = ? AND wm.user_id = ? AND w.deleted_at IS NULL
                ",
                duckdb::params![&current_workspace_id, &user_id],
                |row| row.get(0),
            )?;
            count > 0
        } else {
            false
        };

        if !current_is_valid {
            conn.execute(
                "UPDATE users SET current_workspace_id = ? WHERE id = ?",
                duckdb::params![&target_workspace_id, &user_id],
            )?;
        }
    }

    let workspaces_without_slug = {
        let mut stmt = conn.prepare("SELECT id, name FROM workspaces WHERE slug IS NULL")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for (workspace_id, workspace_name) in workspaces_without_slug {
        let workspace_slug = unique_workspace_slug(conn, &workspace_name, &workspace_id)?;
        conn.execute(
            "UPDATE workspaces SET slug = ? WHERE id = ?",
            duckdb::params![&workspace_slug, &workspace_id],
        )?;
    }

    Ok(())
}

fn unique_workspace_name(
    conn: &duckdb::Connection,
    base_name: &str,
    workspace_id: &str,
) -> Result<String, duckdb::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workspaces WHERE name = ?",
        duckdb::params![base_name],
        |row| row.get(0),
    )?;

    if count == 0 {
        return Ok(base_name.to_string());
    }

    Ok(format!(
        "{} ({})",
        base_name,
        &workspace_id[..workspace_id.len().min(8)]
    ))
}

fn unique_workspace_slug(
    conn: &duckdb::Connection,
    workspace_name: &str,
    workspace_id: &str,
) -> Result<String, duckdb::Error> {
    let base = crate::workspace::workspace_slug_base_from_name_or_id(workspace_name, workspace_id);

    let mut candidate = base.clone();
    let mut suffix = 1_u32;
    loop {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM workspaces WHERE slug = ?",
            duckdb::params![&candidate],
            |row| row.get(0),
        )?;
        if count == 0 {
            return Ok(candidate);
        }

        let suffix_text = format!("-{suffix}");
        let max_base_len = crate::workspace::WORKSPACE_SLUG_MAX_LEN
            .saturating_sub(suffix_text.len())
            .max(crate::workspace::WORKSPACE_SLUG_MIN_LEN);
        let truncated_base = if base.len() > max_base_len {
            base[..max_base_len].trim_end_matches('-').to_string()
        } else {
            base.clone()
        };
        candidate = format!("{truncated_base}{suffix_text}");
        suffix = suffix.saturating_add(1);
    }
}

pub fn init_database(db_path: &Path) -> duckdb::Connection {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create database directory");
    }

    let conn = open_with_wal_recovery(db_path).unwrap_or_else(|e| {
        panic!(
            "Failed to open database with WAL recovery. db_path={}, error={}",
            db_path.display(),
            e
        )
    });

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
    let _ = conn.execute(
        "ALTER TABLE published_files ADD COLUMN use_aliases BOOLEAN DEFAULT TRUE",
        [],
    );

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
            current_workspace_id VARCHAR,
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

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS postgis_connections (
            id VARCHAR PRIMARY KEY,
            name VARCHAR NOT NULL,
            host VARCHAR NOT NULL,
            port INTEGER NOT NULL,
            database_name VARCHAR NOT NULL,
            username VARCHAR NOT NULL,
            password_encrypted VARCHAR NOT NULL,
            ssl_mode VARCHAR NOT NULL DEFAULT 'disable',
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_postgis_connections_name
            ON postgis_connections(name);

        CREATE TABLE IF NOT EXISTS postgis_sources (
            file_id VARCHAR PRIMARY KEY,
            connection_id VARCHAR NOT NULL,
            schema_name VARCHAR NOT NULL,
            object_name VARCHAR NOT NULL,
            geom_column VARCHAR NOT NULL,
            fid_column VARCHAR NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (file_id) REFERENCES files(id),
            FOREIGN KEY (connection_id) REFERENCES postgis_connections(id)
        );

        CREATE INDEX IF NOT EXISTS idx_postgis_sources_connection_id
            ON postgis_sources(connection_id);
        ",
    )
    .expect("Failed to create PostGIS source tables");

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS workspaces (
            id VARCHAR PRIMARY KEY,
            name VARCHAR UNIQUE NOT NULL,
            slug VARCHAR UNIQUE,
            owner_id VARCHAR NOT NULL REFERENCES users(id),
            is_personal BOOLEAN NOT NULL DEFAULT FALSE,
            deleted_at TIMESTAMP,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_workspaces_owner
            ON workspaces(owner_id);

        CREATE INDEX IF NOT EXISTS idx_workspaces_deleted_at
            ON workspaces(deleted_at);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_workspaces_slug
            ON workspaces(slug);

        CREATE TABLE IF NOT EXISTS workspace_members (
            workspace_id VARCHAR NOT NULL REFERENCES workspaces(id),
            user_id VARCHAR NOT NULL REFERENCES users(id),
            joined_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (workspace_id, user_id)
        );

        CREATE INDEX IF NOT EXISTS idx_workspace_members_user
            ON workspace_members(user_id);

        CREATE TABLE IF NOT EXISTS workspace_member_backups (
            workspace_id VARCHAR NOT NULL,
            user_id VARCHAR NOT NULL,
            joined_at TIMESTAMP NOT NULL,
            PRIMARY KEY (workspace_id, user_id)
        );
        ",
    )
    .expect("Failed to create workspace tables");

    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS fonts (
            id VARCHAR PRIMARY KEY,
            workspace_id VARCHAR NOT NULL REFERENCES workspaces(id),
            name VARCHAR NOT NULL,
            fontstack VARCHAR NOT NULL,
            family VARCHAR,
            style VARCHAR,
            original_path VARCHAR NOT NULL,
            glyphs_path VARCHAR NOT NULL,
            glyph_count INTEGER,
            start_cp INTEGER,
            end_cp INTEGER,
            status VARCHAR NOT NULL DEFAULT 'processing',
            error VARCHAR,
            is_public BOOLEAN DEFAULT FALSE,
            slug VARCHAR,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            published_at TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_fonts_workspace
            ON fonts(workspace_id);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_fonts_workspace_slug
            ON fonts(workspace_id, slug);
        ",
    )
    .expect("Failed to create fonts table");

    ensure_workspace_schema_and_backfill(&conn);

    conn
}

fn append_unique_path(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|existing| existing == &path) {
        candidates.push(path);
    }
}

fn wal_recovery_strict_mode() -> bool {
    std::env::var(WAL_RECOVERY_STRICT_ENV)
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn build_appended_wal_path(db_path: &Path) -> PathBuf {
    let mut wal_name = OsString::from(db_path.as_os_str());
    wal_name.push(".wal");
    PathBuf::from(wal_name)
}

fn candidate_wal_paths(db_path: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    append_unique_path(&mut candidates, build_appended_wal_path(db_path));
    append_unique_path(&mut candidates, db_path.with_extension("duckdb.wal"));
    candidates
}

fn find_existing_wal_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.exists()).cloned()
}

fn recover_after_wal_open_error(
    db_path: &Path,
    err_str: &str,
) -> Result<duckdb::Connection, String> {
    let wal_candidates = candidate_wal_paths(db_path);
    let wal_candidates_for_log = wal_candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    tracing::warn!(
        db_path = %db_path.display(),
        error = %err_str,
        wal_candidates = ?wal_candidates_for_log,
        "WAL-related database open failure detected"
    );

    let wal_path = find_existing_wal_path(&wal_candidates).ok_or_else(|| {
        format!(
            "WAL-related error but no WAL file found in candidates. db_path={}, \
             candidates={:?}, error={}",
            db_path.display(),
            wal_candidates_for_log,
            err_str
        )
    })?;

    let backup_path = isolate_wal_file(&wal_path).map_err(|isolate_err| {
        format!(
            "Failed to isolate WAL file before retry. db_path={}, wal_path={}, error={}",
            db_path.display(),
            wal_path.display(),
            isolate_err
        )
    })?;

    tracing::warn!(
        db_path = %db_path.display(),
        wal_path = %wal_path.display(),
        wal_backup_path = %backup_path.display(),
        "Isolated WAL file; retrying database open"
    );

    duckdb::Connection::open(db_path).map_err(|retry_err| {
        format!(
            "Database open still failed after WAL isolation. db_path={}, wal_path={}, \
             wal_backup_path={}, original_error={}, retry_error={}",
            db_path.display(),
            wal_path.display(),
            backup_path.display(),
            err_str,
            retry_err
        )
    })
}

fn build_wal_backup_path(wal_path: &Path) -> PathBuf {
    let timestamp_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut backup_name = OsString::from(wal_path.as_os_str());
    backup_name.push(format!(".bak.{}", timestamp_millis));
    PathBuf::from(backup_name)
}

fn isolate_wal_file(wal_path: &Path) -> Result<PathBuf, String> {
    let backup_path = build_wal_backup_path(wal_path);
    match std::fs::rename(wal_path, &backup_path) {
        Ok(_) => return Ok(backup_path),
        Err(rename_err) => {
            tracing::warn!(
                wal_path = %wal_path.display(),
                wal_backup_path = %backup_path.display(),
                error = %rename_err,
                "Failed to rename WAL file, falling back to copy+remove"
            );
        }
    }

    std::fs::copy(wal_path, &backup_path).map_err(|copy_err| {
        format!(
            "copy failed from {} to {}: {}",
            wal_path.display(),
            backup_path.display(),
            copy_err
        )
    })?;

    std::fs::remove_file(wal_path).map_err(|remove_err| {
        format!(
            "remove failed for {} after copy to {}: {}",
            wal_path.display(),
            backup_path.display(),
            remove_err
        )
    })?;

    Ok(backup_path)
}

fn is_wal_related_open_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("wal file")
        || normalized.contains("replaying wal")
        || (normalized.contains("replay") && normalized.contains("wal"))
        || normalized.contains("failure while replaying wal file")
        || normalized.contains("write-ahead log")
}

#[cfg(feature = "embed-spatial-extension")]
fn build_embedded_spatial_filename() -> String {
    // DuckDB derives entrypoint function name from the extension filename.
    // The filename MUST be "spatial.duckdb_extension" to match the expected
    // entrypoint "spatial_duckdb_cpp_init". Use a versioned subdirectory
    // for integrity checking and automatic invalidation on upgrades.
    let checksum = EMBEDDED_SPATIAL_EXTENSION.iter().fold(0u64, |acc, byte| {
        acc.wrapping_mul(16777619).wrapping_add(u64::from(*byte))
    });
    format!(
        "v{}-{:016x}/spatial.duckdb_extension",
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
fn embedded_spatial_extension_file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
}

#[cfg(feature = "embed-spatial-extension")]
fn verify_materialized_embedded_spatial_extension(
    path: &Path,
    expected_len: u64,
) -> Result<(), String> {
    match embedded_spatial_extension_file_len(path) {
        Some(actual_len) if actual_len == expected_len => Ok(()),
        Some(actual_len) => Err(format!(
            "Embedded spatial extension size mismatch at {}: expected {} bytes, got {} bytes",
            path.display(),
            expected_len,
            actual_len
        )),
        None => Err(format!(
            "Embedded spatial extension missing after materialization at {}",
            path.display()
        )),
    }
}

#[cfg(feature = "embed-spatial-extension")]
fn write_embedded_spatial_extension(path: &Path) -> Result<(), String> {
    let expected_len = EMBEDDED_SPATIAL_EXTENSION.len() as u64;
    if embedded_spatial_extension_file_len(path) == Some(expected_len) {
        tracing::debug!(
            path = %path.display(),
            size = expected_len,
            "Embedded spatial extension already exists with correct size"
        );
        return Ok(());
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("Missing parent directory for {}", path.display()))?;

    tracing::info!(
        path = %path.display(),
        parent = %parent.display(),
        size = expected_len,
        "Materializing embedded spatial extension"
    );

    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let tmp_path = parent.join(format!(".{SPATIAL_EXTENSION_FILENAME}.{nonce}.tmp"));

    std::fs::write(&tmp_path, EMBEDDED_SPATIAL_EXTENSION)
        .map_err(|e| format!("Failed to write {}: {}", tmp_path.display(), e))?;
    #[cfg(unix)]
    std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("Failed to set permissions on {}: {}", tmp_path.display(), e))?;

    match std::fs::rename(&tmp_path, path) {
        Ok(_) => {
            let verify_result = verify_materialized_embedded_spatial_extension(path, expected_len);
            if verify_result.is_err() {
                let _ = std::fs::remove_file(path);
            }
            verify_result
        }
        Err(rename_err) => {
            if embedded_spatial_extension_file_len(path) == Some(expected_len) {
                let _ = std::fs::remove_file(&tmp_path);
                return Ok(());
            }
            if std::fs::remove_file(path).is_ok() && std::fs::rename(&tmp_path, path).is_ok() {
                let verify_result =
                    verify_materialized_embedded_spatial_extension(path, expected_len);
                if verify_result.is_err() {
                    let _ = std::fs::remove_file(path);
                }
                return verify_result;
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

    tracing::info!(
        filename = %file_name,
        candidate_count = base_dirs.len(),
        "Attempting to materialize embedded spatial extension"
    );

    let mut errors = Vec::new();
    for base_dir in &base_dirs {
        let target_path = base_dir.join("extensions").join(&file_name);
        tracing::debug!(
            path = %target_path.display(),
            "Trying to write embedded extension to candidate directory"
        );
        match write_embedded_spatial_extension(&target_path) {
            Ok(_) => {
                tracing::info!(
                    path = %target_path.display(),
                    "Embedded spatial extension materialized successfully"
                );
                return Ok(target_path);
            }
            Err(e) => {
                tracing::debug!(
                    path = %target_path.display(),
                    error = %e,
                    "Failed to write embedded extension to candidate directory"
                );
                errors.push(format!("{} ({})", target_path.display(), e));
            }
        }
    }

    let attempted_dirs = base_dirs
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    Err(format!(
        "Unable to materialize embedded spatial extension (tried {} directories: {}): {}",
        base_dirs.len(),
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

fn ensure_spatial_extension_with_candidates(
    conn: &duckdb::Connection,
    local_candidates: &[PathBuf],
) -> Result<(), String> {
    let mut local_load_error: Option<String> = None;

    tracing::info!(
        candidates = ?local_candidates,
        "Searching for embedded spatial extension"
    );

    if let Some(local_path) = find_existing_local_spatial_extension_path(local_candidates) {
        tracing::info!(
            path = %local_path.display(),
            "Loading spatial extension from embedded file"
        );
        match try_load_spatial_from_path(conn, &local_path) {
            Ok(_) => {
                tracing::info!("Spatial extension loaded successfully");
                return Ok(());
            }
            Err(error) => {
                let formatted = format!(
                    "Failed to load local spatial extension from '{}': {}",
                    local_path.display(),
                    error
                );
                tracing::warn!(error = %formatted, "Local spatial extension load failed");
                local_load_error = Some(formatted);
            }
        }
    }

    // Fallback for environments where spatial extension is installed globally.
    if conn.execute_batch("LOAD spatial;").is_ok() {
        tracing::info!("Spatial extension loaded from DuckDB default extension path");
        return Ok(());
    }

    #[cfg(feature = "embed-spatial-extension")]
    panic!(
        "Embedded spatial extension feature is enabled but no usable extension could be loaded.\n\
         \n\
         Local load error: {}\n\
         \n\
         Searched paths:\n{}\n\
         \n\
         This indicates a build or packaging error in the release bundle.\n\
         Please report this issue if you downloaded an official release.",
        local_load_error.unwrap_or_else(|| "none".to_string()),
        local_candidates
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );

    #[cfg(not(feature = "embed-spatial-extension"))]
    {
        Err(format!(
            "Unable to load DuckDB spatial extension from local paths and LOAD spatial failed.\n\
             \n\
             Local load error: {}\n\
             \n\
             Searched paths:\n{}\n\
             \n\
             For local development, run `just setup-dev` to download the extension for your platform.\n\
             For release/self-contained builds, compile with `--features embed-spatial-extension` after preparing backend/extensions/spatial.duckdb_extension.",
            local_load_error.unwrap_or_else(|| "none".to_string()),
            local_candidates
                .iter()
                .map(|p| format!("  - {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

pub fn ensure_spatial_extension(conn: &duckdb::Connection) -> Result<(), String> {
    let local_candidates = local_spatial_extension_candidates();
    ensure_spatial_extension_with_candidates(conn, &local_candidates)
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

pub fn ensure_app_secret(conn: &duckdb::Connection) -> Result<String, String> {
    let new_secret = generate_random_secret();

    let rows_affected = conn
        .execute(
            "INSERT INTO system_settings (key, value) VALUES ('app_secret', ?) ON CONFLICT (key) DO NOTHING",
            duckdb::params![&new_secret],
        )
        .map_err(|e| format!("Failed to store app_secret: {}", e))?;

    if rows_affected > 0 {
        tracing::info!("Generated and stored new app_secret for PostGIS credential encryption");
        return Ok(new_secret);
    }

    let mut stmt = conn
        .prepare("SELECT value FROM system_settings WHERE key = 'app_secret'")
        .map_err(|e| format!("Failed to prepare app_secret query: {}", e))?;

    stmt.query_row([], |row| row.get(0))
        .map_err(|e| format!("Failed to read app_secret after conflict: {}", e))
}

pub fn get_app_secret(conn: &duckdb::Connection) -> Result<Option<String>, String> {
    let mut stmt = conn
        .prepare("SELECT value FROM system_settings WHERE key = 'app_secret'")
        .map_err(|e| format!("Failed to prepare app_secret query: {}", e))?;

    match stmt.query_row([], |row| row.get(0)) {
        Ok(secret) => Ok(Some(secret)),
        Err(duckdb::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(format!("Failed to read app_secret: {}", e)),
    }
}

fn generate_random_secret() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    const SECRET_LENGTH: usize = 64;

    let mut rng = rand::thread_rng();
    (0..SECRET_LENGTH)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{make_legacy_shared_workspace_name, make_personal_workspace_name};

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
    fn candidate_wal_paths_support_multiple_db_suffixes() {
        let duckdb_path = Path::new("/tmp/mapflow.duckdb");
        let duckdb_candidates = candidate_wal_paths(duckdb_path);
        assert_eq!(
            duckdb_candidates,
            vec![PathBuf::from("/tmp/mapflow.duckdb.wal")]
        );

        let db_path = Path::new("/tmp/mapflow.db");
        let db_candidates = candidate_wal_paths(db_path);
        assert_eq!(
            db_candidates,
            vec![
                PathBuf::from("/tmp/mapflow.db.wal"),
                PathBuf::from("/tmp/mapflow.duckdb.wal"),
            ]
        );

        let no_ext_path = Path::new("/tmp/mapflow");
        let no_ext_candidates = candidate_wal_paths(no_ext_path);
        assert_eq!(
            no_ext_candidates,
            vec![
                PathBuf::from("/tmp/mapflow.wal"),
                PathBuf::from("/tmp/mapflow.duckdb.wal"),
            ]
        );
    }

    #[test]
    fn is_wal_related_open_error_matches_common_variants() {
        assert!(is_wal_related_open_error(
            "IO Error: Failure while replaying WAL file \"/tmp/mapflow.duckdb.wal\": Duplicate key"
        ));
        assert!(is_wal_related_open_error(
            "duckdb error: WAL file \"C:\\\\data\\\\mapflow.duckdb.wal\" is corrupted"
        ));
        assert!(!is_wal_related_open_error(
            "IO Error: Permission denied opening main database file"
        ));
    }

    #[test]
    fn recover_after_wal_open_error_supports_non_duckdb_db_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("test.db");

        let conn = duckdb::Connection::open(&db_path).expect("open");
        conn.execute("CREATE TABLE test (id INTEGER)", [])
            .expect("create table");
        conn.execute("INSERT INTO test VALUES (1)", [])
            .expect("insert");
        conn.execute("CHECKPOINT", []).expect("checkpoint");
        drop(conn);

        let wal_path = build_appended_wal_path(&db_path);
        std::fs::write(&wal_path, b"corrupted wal data").expect("write corrupt wal");

        let conn = recover_after_wal_open_error(
            &db_path,
            "IO Error: Failure while replaying WAL file: corrupted input",
        )
        .expect("recover open");
        let one: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).expect("query");
        assert_eq!(one, 1);

        let backup_candidates = std::fs::read_dir(temp.path())
            .expect("read temp dir")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("test.db.wal.bak."))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        assert_eq!(backup_candidates.len(), 1);
        assert!(!wal_path.exists());
    }

    #[test]
    fn recover_after_wal_open_error_isolates_wal_and_succeeds() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("test.duckdb");

        let conn = duckdb::Connection::open(&db_path).expect("open");
        conn.execute("CREATE TABLE test (id INTEGER)", [])
            .expect("create table");
        conn.execute("INSERT INTO test VALUES (1)", [])
            .expect("insert");
        conn.execute("CHECKPOINT", []).expect("checkpoint");
        drop(conn);

        let wal_path = db_path.with_extension("duckdb.wal");
        std::fs::write(&wal_path, b"corrupted wal data").expect("write corrupt wal");

        let conn = recover_after_wal_open_error(
            &db_path,
            "IO Error: Failure while replaying WAL file: corrupted input",
        )
        .expect("recover open");
        let one: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).expect("query");
        assert_eq!(one, 1);

        let backup_candidates = std::fs::read_dir(temp.path())
            .expect("read temp dir")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("test.duckdb.wal.bak."))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        assert_eq!(backup_candidates.len(), 1);
        assert!(!wal_path.exists());
    }

    #[test]
    fn ensure_spatial_extension_falls_back_after_local_load_failure() {
        let valid_extension = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("extensions")
            .join(SPATIAL_EXTENSION_FILENAME);
        if !valid_extension.is_file() {
            eprintln!(
                "Skipping: valid spatial extension file not found at {}",
                valid_extension.display()
            );
            return;
        }

        let conn = duckdb::Connection::open_in_memory().expect("open in-memory db");
        try_load_spatial_from_path(&conn, &valid_extension)
            .expect("preload spatial extension from valid path");

        if conn.execute_batch("LOAD spatial;").is_err() {
            eprintln!("Skipping: LOAD spatial is not available in current test environment");
            return;
        }

        let temp = tempfile::tempdir().expect("tempdir");
        let bad_extension = temp.path().join("bad-spatial.duckdb_extension");
        std::fs::write(&bad_extension, b"not-a-valid-duckdb-extension")
            .expect("write invalid extension file");

        let result = ensure_spatial_extension_with_candidates(&conn, &[bad_extension]);
        assert!(
            result.is_ok(),
            "expected fallback LOAD spatial to succeed after local load failure, got: {:?}",
            result
        );
    }

    #[cfg(feature = "embed-spatial-extension")]
    #[test]
    fn write_embedded_spatial_extension_materializes_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("extensions")
            .join("spatial.duckdb_extension");

        write_embedded_spatial_extension(&path).expect("write embedded extension");

        let metadata = std::fs::metadata(&path).expect("embedded metadata");
        assert!(metadata.is_file());
        assert_eq!(metadata.len(), EMBEDDED_SPATIAL_EXTENSION.len() as u64);
    }

    #[cfg(feature = "embed-spatial-extension")]
    #[test]
    fn write_embedded_spatial_extension_replaces_wrong_size_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("extensions")
            .join("spatial.duckdb_extension");
        let parent = path.parent().expect("parent");
        std::fs::create_dir_all(parent).expect("create dir");
        std::fs::write(&path, b"bad").expect("write bad file");

        write_embedded_spatial_extension(&path).expect("rewrite embedded extension");

        let metadata = std::fs::metadata(&path).expect("embedded metadata");
        assert!(metadata.is_file());
        assert_eq!(metadata.len(), EMBEDDED_SPATIAL_EXTENSION.len() as u64);
    }

    #[cfg(feature = "embed-spatial-extension")]
    #[test]
    fn verify_materialized_embedded_spatial_extension_rejects_size_mismatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("extensions")
            .join("spatial.duckdb_extension");
        let parent = path.parent().expect("parent");
        std::fs::create_dir_all(parent).expect("create dir");
        std::fs::write(&path, b"bad").expect("write bad file");

        let expected_len = EMBEDDED_SPATIAL_EXTENSION.len() as u64;
        let error = verify_materialized_embedded_spatial_extension(&path, expected_len)
            .expect_err("expected size mismatch");
        assert!(error.contains("size mismatch"));
    }

    #[cfg(all(unix, feature = "embed-spatial-extension"))]
    #[test]
    fn write_embedded_spatial_extension_sets_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("extensions")
            .join("spatial.duckdb_extension");

        write_embedded_spatial_extension(&path).expect("write embedded extension");

        let mode = std::fs::metadata(&path)
            .expect("embedded metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn init_database_backfills_legacy_users_files_and_current_workspace() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("legacy.duckdb");

        let conn = duckdb::Connection::open(&db_path).expect("open legacy db");
        conn.execute_batch(
            r"
            CREATE TABLE users (
                id VARCHAR PRIMARY KEY,
                username VARCHAR UNIQUE NOT NULL,
                password_hash VARCHAR NOT NULL,
                role VARCHAR NOT NULL,
                created_at TIMESTAMP NOT NULL
            );
            CREATE TABLE files (
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
            CREATE TABLE published_files (
                file_id VARCHAR PRIMARY KEY,
                slug VARCHAR UNIQUE NOT NULL,
                published_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                minzoom INTEGER,
                maxzoom INTEGER,
                FOREIGN KEY (file_id) REFERENCES files(id)
            );
            ",
        )
        .expect("seed legacy schema");

        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at) VALUES (?, ?, ?, ?, ?)",
            duckdb::params!["legacy-user-1", "alice", "hash", "admin", "2026-01-01 00:00:00"],
        )
        .expect("insert legacy user 1");
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at) VALUES (?, ?, ?, ?, ?)",
            duckdb::params!["legacy-user-2", "bob", "hash", "user", "2026-01-02 00:00:00"],
        )
        .expect("insert legacy user 2");
        conn.execute(
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                "legacy-file-1",
                "roads",
                "geojson",
                1_i64,
                "2026-01-03 00:00:00",
                "ready",
                Option::<String>::None,
                "./uploads/roads.geojson",
                Option::<String>::None,
                Option::<String>::None,
                false,
            ],
        )
        .expect("insert legacy file");
        drop(conn);

        let conn = init_database(&db_path);

        let current_workspace_columns: Vec<String> = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(users)")
                .expect("users pragma");
            stmt.query_map([], |row| row.get::<_, String>(1))
                .expect("query columns")
                .map(|row| row.expect("column"))
                .collect()
        };
        assert!(
            current_workspace_columns.contains(&"current_workspace_id".to_string()),
            "expected current_workspace_id column to be added"
        );

        let shared_workspace_name: String = conn
            .query_row(
                "SELECT name FROM workspaces WHERE id = 'legacy-shared-workspace'",
                [],
                |row| row.get(0),
            )
            .expect("shared workspace exists");
        assert_eq!(shared_workspace_name, make_legacy_shared_workspace_name());

        let shared_member_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workspace_members WHERE workspace_id = 'legacy-shared-workspace'",
                [],
                |row| row.get(0),
            )
            .expect("shared member count");
        assert_eq!(shared_member_count, 2);

        let alice_personal_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workspaces WHERE owner_id = ? AND is_personal = TRUE AND name = ?",
                duckdb::params!["legacy-user-1", make_personal_workspace_name("alice")],
                |row| row.get(0),
            )
            .expect("alice personal workspace");
        assert_eq!(alice_personal_exists, 1);

        let bob_personal_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workspaces WHERE owner_id = ? AND is_personal = TRUE AND name = ?",
                duckdb::params!["legacy-user-2", make_personal_workspace_name("bob")],
                |row| row.get(0),
            )
            .expect("bob personal workspace");
        assert_eq!(bob_personal_exists, 1);

        let file_workspace_id: String = conn
            .query_row(
                "SELECT workspace_id FROM files WHERE id = 'legacy-file-1'",
                [],
                |row| row.get(0),
            )
            .expect("file workspace id");
        assert_eq!(file_workspace_id, "legacy-shared-workspace");

        let alice_current_workspace: Option<String> = conn
            .query_row(
                "SELECT current_workspace_id FROM users WHERE id = 'legacy-user-1'",
                [],
                |row| row.get(0),
            )
            .expect("alice current workspace");
        assert_eq!(
            alice_current_workspace.as_deref(),
            Some("legacy-shared-workspace")
        );

        let bob_current_workspace: Option<String> = conn
            .query_row(
                "SELECT current_workspace_id FROM users WHERE id = 'legacy-user-2'",
                [],
                |row| row.get(0),
            )
            .expect("bob current workspace");
        assert_eq!(
            bob_current_workspace.as_deref(),
            Some("legacy-shared-workspace")
        );
    }
}
