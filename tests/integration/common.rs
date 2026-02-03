use reqwest::Client;
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;
use zip::{ZipWriter, write::FileOptions};
use tokio::process::Command;

pub async fn start_test_server() -> Client {
    // Kill any existing server on port 3001
    let _ = Command::new("lsof")
        .args(["-ti:3001"])
        .output()
        .await;

    // Start the server in background
    let _ = Command::new("cargo")
        .args(["run", "--"])
        .env("PORT", "3001")
        .spawn()
        .expect("Failed to start server");

    // Wait for server to be ready
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    Client::new()
}

pub fn create_test_shapefile(name: &str) -> String {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let zip_path = temp_dir.path().join(format!("{}.zip", name));

    let file = File::create(&zip_path).expect("Failed to create zip file");
    let mut zip = ZipWriter::new(file);

    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let base = "gis_osm_buildings_a_free_1";
    let parts = ["shp", "shx", "dbf", "prj", "cpg"];

    for ext in parts {
        let path = format!("{}.{}", base, ext);
        let bytes = fs::read(&path).unwrap_or_else(|_| {
            panic!("Missing test shapefile part: {}", path);
        });

        zip.start_file(format!("{}.{}", base, ext), options)
            .expect("Failed to create zip entry");
        zip.write_all(&bytes).expect("Failed to write zip entry");
    }

    zip.finish().expect("Failed to finish zip");

    let final_path = format!("data/{}.zip", name);
    fs::create_dir_all("data").expect("Failed to create data directory");
    fs::copy(&zip_path, &final_path).expect("Failed to copy zip file");

    final_path
}

pub fn create_invalid_zip() -> Vec<u8> {
    b"This is not a valid ZIP file".to_vec()
}

pub fn create_valid_config() -> Value {
    serde_json::json!({
        "version": "0.1.0",
        "nodes": [
            {
                "id": "RES_TEST_1234567890ABC",
                "type": "resource",
                "name": "test_resource",
                "description": "Test resource node",
                "resource_type": "shapefile",
                "file_path": ["data/test.shp"],
                "size": 1024,
                "create_timestamp": 1234567890,
                "hash": "test_hash",
                "srid": "4326",
                "duckdb_table_name": "test_1234567890ABCDEF",
                "readonly": true
            },
            {
                "id": "LAYER_TEST_0987654321DEF",
                "type": "layer",
                "name": "test_layer",
                "description": "Test layer node",
                "source_resource_id": "RES_TEST_1234567890ABC",
                "fields": ["name", "id"],
                "minzoom": 10,
                "maxzoom": 18,
                "readonly": false
            },
            {
                "id": "XYZ_TEST_FEDCBA0987654",
                "type": "xyz",
                "name": "test_xyz",
                "description": "Test XYZ tile service",
                "center": [121.4737, 31.2304, 12.0],
                "min_zoom": 0,
                "max_zoom": 22,
                "fillzoom": 12,
                "bounds": [-180.0, -85.0511, 180.0, 85.0511],
                "layers": [
                    {
                        "id": "layer_test",
                        "source_layer_id": "LAYER_TEST_0987654321DEF"
                    }
                ],
                "readonly": false
            }
        ],
        "edges": [
            {
                "id": "EDGE_1",
                "source": "RES_TEST_1234567890ABC",
                "target": "LAYER_TEST_0987654321DEF"
            },
            {
                "id": "EDGE_2",
                "source": "LAYER_TEST_0987654321DEF",
                "target": "XYZ_TEST_FEDCBA0987654"
            }
        ]
    })
}
