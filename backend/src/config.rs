const DEFAULT_MAX_SIZE_MB: u64 = 200;
const BYTES_PER_MB: u64 = 1024 * 1024;

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
