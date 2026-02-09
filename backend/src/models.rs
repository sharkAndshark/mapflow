use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub upload_dir: PathBuf,
    pub db: Arc<Mutex<duckdb::Connection>>,
    pub max_size: u64,
    pub max_size_label: String,
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
