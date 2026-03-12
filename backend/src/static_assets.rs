#[cfg(feature = "embed-web-dist")]
use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};

#[cfg(feature = "embed-web-dist")]
use include_dir::{include_dir, Dir, DirEntry};

#[cfg(feature = "embed-web-dist")]
static EMBEDDED_WEB_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../frontend/dist");
#[cfg(feature = "embed-web-dist")]
const EMBED_VIEWER_ROUTE_MARKERS: [&str; 2] = ["/tiles/:slug/embed", "tile-embed-page"];

#[cfg(feature = "embed-web-dist")]
pub fn embedded_spa_available() -> bool {
    EMBEDDED_WEB_DIST.get_file("index.html").is_some() && embedded_bundle_has_embed_marker()
}

#[cfg(feature = "embed-web-dist")]
fn embedded_bundle_has_embed_marker() -> bool {
    dir_has_embed_marker(&EMBEDDED_WEB_DIST)
}

#[cfg(feature = "embed-web-dist")]
fn dir_has_embed_marker(dir: &Dir<'_>) -> bool {
    dir.entries().iter().any(|entry| match entry {
        DirEntry::File(file) => {
            let path = file.path().to_string_lossy();
            (path.ends_with(".js") || path.ends_with(".mjs"))
                && std::str::from_utf8(file.contents())
                    .map(|content| {
                        EMBED_VIEWER_ROUTE_MARKERS
                            .iter()
                            .any(|marker| content.contains(marker))
                    })
                    .unwrap_or(false)
        }
        DirEntry::Dir(sub_dir) => dir_has_embed_marker(sub_dir),
    })
}

#[cfg(feature = "embed-web-dist")]
pub async fn serve_embedded_spa(uri: Uri) -> Response {
    let request_path = normalize_request_path(uri.path());

    if let Some(response) = embedded_file_response(&request_path) {
        return response;
    }

    if let Some(index_response) = embedded_file_response("index.html") {
        return index_response;
    }

    StatusCode::NOT_FOUND.into_response()
}

#[cfg(feature = "embed-web-dist")]
fn normalize_request_path(raw_path: &str) -> String {
    let trimmed = raw_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return "index.html".to_string();
    }

    if trimmed.ends_with('/') {
        return format!("{trimmed}index.html");
    }

    trimmed.to_string()
}

#[cfg(feature = "embed-web-dist")]
fn embedded_file_response(path: &str) -> Option<Response> {
    let file = EMBEDDED_WEB_DIST.get_file(path)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type_for(path)),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );

    Some((headers, file.contents().to_vec()).into_response())
}

#[cfg(feature = "embed-web-dist")]
fn content_type_for(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".js") || path.ends_with(".mjs") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".json") || path.ends_with(".map") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".woff") {
        "font/woff"
    } else if path.ends_with(".ttf") {
        "font/ttf"
    } else if path.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    }
}

#[cfg(all(test, feature = "embed-web-dist"))]
mod tests {
    use super::*;
    use include_dir::File;

    #[test]
    fn dir_has_embed_marker_recurses_into_nested_dirs() {
        static NESTED_ENTRIES: [DirEntry<'_>; 1] = [DirEntry::File(File::new(
            "assets/app.js",
            b"console.log('/tiles/:slug/embed');",
        ))];
        static ROOT_ENTRIES: [DirEntry<'_>; 2] = [
            DirEntry::File(File::new("index.html", b"<html></html>")),
            DirEntry::Dir(Dir::new("assets", &NESTED_ENTRIES)),
        ];
        static ROOT_DIR: Dir<'_> = Dir::new("", &ROOT_ENTRIES);

        assert!(dir_has_embed_marker(&ROOT_DIR));
    }

    #[test]
    fn dir_has_embed_marker_returns_false_without_markers() {
        static ENTRIES: [DirEntry<'_>; 1] = [DirEntry::File(File::new(
            "assets/app.js",
            b"console.log('legacy bundle');",
        ))];
        static ROOT_DIR: Dir<'_> = Dir::new("", &ENTRIES);

        assert!(!dir_has_embed_marker(&ROOT_DIR));
    }
}
