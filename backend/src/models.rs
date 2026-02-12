use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{AuthBackend, DuckDBStore};

#[derive(Clone)]
pub struct AppState {
    pub upload_dir: PathBuf,
    pub upload_dir_canonical: PathBuf, // Pre-computed for security checks
    pub db: Arc<Mutex<duckdb::Connection>>,
    pub max_size: u64,
    pub max_size_label: String,
    pub auth_backend: AuthBackend,
    pub session_store: DuckDBStore,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub size: u64,
    #[serde(rename = "uploadedAt")]
    pub uploaded_at: String,
    pub status: String,
    pub crs: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(rename = "isPublic")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
    #[serde(rename = "publicSlug")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_slug: Option<String>,
    #[serde(rename = "tileSource")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile_source: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize)]
pub struct PreviewMeta {
    pub id: String,
    pub name: String,
    pub crs: Option<String>,
    pub bbox: Option<[f64; 4]>, // minx, miny, maxx, maxy in WGS84
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile_format: Option<String>, // "mvt", "png", or null
    pub tile_source: Option<String>, // "duckdb", "mbtiles", "pmtiles"
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct FeatureProperty {
    pub key: String,
    pub value: serde_json::Value,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct FeaturePropertiesResponse {
    pub fid: i64,
    pub properties: Vec<FeatureProperty>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileSchemaResponse {
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Deserialize)]
pub struct PublishRequest {
    pub slug: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublishResponse {
    pub url: String,
    pub slug: String,
    pub is_public: bool,
}

#[derive(Debug, Serialize)]
pub struct PublicTileUrl {
    pub slug: String,
    pub url: String,
}

#[derive(Serialize)]
pub struct PublicTileMeta {
    pub slug: String,
    pub name: String,
    pub tile_source: String,
    pub tile_url: String,
    pub viewer_url: String,
}
