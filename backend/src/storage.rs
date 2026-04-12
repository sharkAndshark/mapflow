use std::path::Path;
use uuid::Uuid;

pub fn create_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn relative_path_for(absolute: &Path, upload_dir: &Path) -> String {
    if let Ok(relative) = absolute.strip_prefix(upload_dir) {
        let dir_name = upload_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("uploads");
        return format!(
            "./{}/{}",
            dir_name,
            relative.to_string_lossy().replace('\\', "/")
        );
    }
    let s = absolute.to_string_lossy().replace('\\', "/");
    if s.starts_with('.') {
        s
    } else {
        format!("./{s}")
    }
}

pub fn resolve_stored_path(stored_path: &str, upload_dir: &Path) -> std::path::PathBuf {
    let dir_name = upload_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("uploads");
    let prefix = format!("./{dir_name}/");
    let relative = stored_path.strip_prefix(&prefix).unwrap_or(stored_path);
    upload_dir.join(relative)
}
