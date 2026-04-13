use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::{sync::Mutex, sync::RwLock};

use crate::{AuthBackend, DuckDBStore};

#[derive(Clone)]
pub struct AppState {
    pub upload_dir: PathBuf,
    pub upload_dir_canonical: PathBuf,
    pub db: Arc<Mutex<duckdb::Connection>>,
    pub max_size: Arc<RwLock<u64>>,
    pub max_size_label: Arc<RwLock<String>>,
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
    #[serde(rename = "crsType", skip_serializing_if = "Option::is_none")]
    pub crs_type: Option<String>,
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
    #[serde(rename = "tileFormat", skip_serializing_if = "Option::is_none")]
    pub tile_format: Option<String>,
    #[serde(rename = "minZoom", skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<i32>,
    #[serde(rename = "maxZoom", skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<i32>,
    #[serde(rename = "useAliases", skip_serializing_if = "Option::is_none")]
    pub use_aliases: Option<bool>,
    #[serde(rename = "tileSource", skip_serializing_if = "Option::is_none")]
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
    #[serde(rename = "crsType")]
    pub crs_type: String,
    pub bbox: Option<[f64; 4]>,
    #[serde(rename = "dataBounds", skip_serializing_if = "Option::is_none")]
    pub data_bounds: Option<[f64; 4]>,
    #[serde(rename = "tileFormat", skip_serializing_if = "Option::is_none")]
    pub tile_format: Option<String>,
    #[serde(rename = "minZoom", skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<i32>,
    #[serde(rename = "maxZoom", skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<i32>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct FeatureProperty {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LayerInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileSchemaResponse {
    pub layers: Vec<LayerInfo>,
}

#[derive(Debug, Deserialize)]
pub struct PublishRequest {
    pub slug: Option<String>,
    #[serde(rename = "minZoom")]
    pub min_zoom: Option<i32>,
    #[serde(rename = "maxZoom")]
    pub max_zoom: Option<i32>,
    #[serde(rename = "useAliases", default = "default_use_aliases")]
    pub use_aliases: bool,
}

fn default_use_aliases() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct PublishResponse {
    pub url: String,
    pub slug: String,
    pub is_public: bool,
    #[serde(rename = "useAliases", skip_serializing_if = "Option::is_none")]
    pub use_aliases: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PublicTileUrl {
    pub slug: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct PublicTileMeta {
    pub slug: String,
    pub name: String,
    #[serde(rename = "tileSource")]
    pub tile_source: String,
    #[serde(rename = "tileUrl")]
    pub tile_url: String,
    #[serde(rename = "viewerUrl", skip_serializing_if = "Option::is_none")]
    pub viewer_url: Option<String>,
    pub crs: Option<String>,
    #[serde(rename = "crsType")]
    pub crs_type: String,
    pub bbox: Option<[f64; 4]>,
    #[serde(rename = "dataBounds", skip_serializing_if = "Option::is_none")]
    pub data_bounds: Option<[f64; 4]>,
    #[serde(rename = "tileFormat", skip_serializing_if = "Option::is_none")]
    pub tile_format: Option<String>,
    #[serde(rename = "minZoom", skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<i32>,
    #[serde(rename = "maxZoom", skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCrsRequest {
    pub crs: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateZoomRequest {
    #[serde(rename = "minZoom")]
    pub min_zoom: Option<i32>,
    #[serde(rename = "maxZoom")]
    pub max_zoom: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePublishSettingsRequest {
    #[serde(rename = "useAliases")]
    pub use_aliases: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct FieldAliasUpdate {
    pub normalized_name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFieldAliasesRequest {
    pub fields: Vec<FieldAliasUpdate>,
}

#[derive(Debug, Deserialize)]
pub struct PostgisConnectionConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    #[serde(rename = "sslMode", default = "default_postgis_ssl_mode")]
    pub ssl_mode: String,
}

fn default_postgis_ssl_mode() -> String {
    "disable".to_string()
}

#[derive(Debug, Deserialize)]
pub struct PostgisConnectionTestRequest {
    pub connection: PostgisConnectionConfig,
}

#[derive(Debug, Serialize)]
pub struct PostgisConnectionTestResponse {
    pub success: bool,
    #[serde(rename = "serverVersion")]
    pub server_version: String,
    #[serde(rename = "postgisVersion")]
    pub postgis_version: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterPostgisSourceRequest {
    #[serde(rename = "connectionName")]
    pub connection_name: String,
    pub connection: PostgisConnectionConfig,
    pub schema: String,
    pub object: String,
    #[serde(rename = "geometryColumn")]
    pub geometry_column: String,
    #[serde(rename = "fidColumn")]
    pub fid_column: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegisterPostgisSourceResponse {
    #[serde(rename = "fileId")]
    pub file_id: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MapItem {
    pub id: String,
    pub name: String,
    pub style_json: Option<String>,
    pub slug: Option<String>,
    pub is_public: bool,
    pub published_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMapRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMapRequest {
    pub name: Option<String>,
    pub style_json: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSourceItem {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_bounds: Option<String>,
    pub status: String,
}
