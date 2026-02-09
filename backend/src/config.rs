const DEFAULT_MAX_SIZE_MB: u64 = 200;
const BYTES_PER_MB: u64 = 1024 * 1024;

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
