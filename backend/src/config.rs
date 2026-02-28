const DEFAULT_MAX_SIZE_MB: u64 = 1024;
pub const BYTES_PER_MB: u64 = 1024 * 1024;
const MIN_UPLOAD_SIZE_MB: u64 = 1;
const MAX_UPLOAD_SIZE_MB: u64 = 102400; // 100GB

/// Read CORS allowed origins from environment variable
/// Format: comma-separated list of origins (e.g., "http://localhost:5173,https://example.com")
/// Defaults to allowing development origins if not set
pub fn read_cors_origins() -> Vec<String> {
    std::env::var("CORS_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_else(|| {
            // Default to development origins
            vec![
                "http://localhost:5173".to_string(), // Vite dev server
                "http://localhost:3000".to_string(), // Production preview
            ]
        })
}

pub fn read_cookie_secure() -> bool {
    std::env::var("COOKIE_SECURE")
        .ok()
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(false)
}

pub fn read_max_size_config() -> (u64, String) {
    let max_size_mb = std::env::var("UPLOAD_MAX_SIZE_MB")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SIZE_MB);
    let bytes = max_size_mb.saturating_mul(BYTES_PER_MB);
    (bytes, format_bytes(bytes))
}

pub fn read_max_size_from_db(conn: &duckdb::Connection) -> Option<(u64, String)> {
    let result: Result<String, _> = conn.query_row(
        "SELECT value FROM system_settings WHERE key = 'upload_max_size_mb'",
        [],
        |row| row.get(0),
    );

    result.ok().and_then(|value| {
        value.parse::<u64>().ok().and_then(|mb| {
            if mb < MIN_UPLOAD_SIZE_MB {
                tracing::warn!(
                    db_value = mb,
                    min_required = MIN_UPLOAD_SIZE_MB,
                    "Database contains upload size below minimum, ignoring"
                );
                return None;
            }
            let bytes = mb.saturating_mul(BYTES_PER_MB);
            Some((bytes, format_bytes(bytes)))
        })
    })
}

pub fn save_max_size_to_db(conn: &duckdb::Connection, max_size_mb: u64) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO system_settings (key, value) VALUES ('upload_max_size_mb', ?)",
        duckdb::params![max_size_mb.to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn init_max_size_config(conn: &duckdb::Connection) -> (u64, String) {
    if let Some(cached) = read_max_size_from_db(conn) {
        tracing::info!(
            max_size_mb = cached.0 / BYTES_PER_MB,
            source = "database",
            "Upload size limit loaded from database"
        );
        return cached;
    }

    let (bytes, label) = read_max_size_config();
    let max_size_mb = bytes / BYTES_PER_MB;

    if let Err(e) = save_max_size_to_db(conn, max_size_mb) {
        tracing::warn!(error = %e, "Failed to persist upload size to database, using in-memory value");
    } else {
        tracing::info!(
            max_size_mb,
            source = "env_or_default",
            "Upload size limit initialized and saved to database"
        );
    }

    (bytes, label)
}

pub fn validate_upload_size_mb(value: u64) -> Result<u64, String> {
    if value < MIN_UPLOAD_SIZE_MB {
        return Err(format!(
            "Upload size must be at least {}MB (got {})",
            MIN_UPLOAD_SIZE_MB, value
        ));
    }
    if value > MAX_UPLOAD_SIZE_MB {
        return Err(format!(
            "Upload size must be at most {}MB (got {})",
            MAX_UPLOAD_SIZE_MB, value
        ));
    }
    Ok(value)
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes >= GB && bytes.is_multiple_of(GB) {
        format!("{}GB", bytes / GB)
    } else if bytes >= MB && bytes.is_multiple_of(MB) {
        format!("{}MB", bytes / MB)
    } else if bytes >= KB && bytes.is_multiple_of(KB) {
        format!("{}KB", bytes / KB)
    } else {
        format!("{}B", bytes)
    }
}

const DEFAULT_PREVIEW_MIN_ZOOM: i32 = 0;
const DEFAULT_PREVIEW_MAX_ZOOM: i32 = 22;
const MAX_TILE_ZOOM: i32 = 22;

/// Read preview zoom range from environment variables.
/// Returns (min_zoom, max_zoom) with defaults (0, 22) if not set.
/// Values are clamped to valid tile zoom range (0-22) and ensured min <= max.
pub fn read_preview_zoom_config() -> (i32, i32) {
    let min_zoom = std::env::var("PREVIEW_MIN_ZOOM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PREVIEW_MIN_ZOOM);
    let max_zoom = std::env::var("PREVIEW_MAX_ZOOM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PREVIEW_MAX_ZOOM);

    // Clamp to valid tile zoom range and ensure min <= max
    let min_zoom = min_zoom.clamp(0, MAX_TILE_ZOOM);
    let max_zoom = max_zoom.clamp(min_zoom, MAX_TILE_ZOOM);

    (min_zoom, max_zoom)
}
