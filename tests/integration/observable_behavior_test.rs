use reqwest::multipart;
use serde_json::Value;
use std::fs;

mod common;

#[tokio::test]
async fn test_server_starts_successfully() {
    let client = common::start_test_server().await;
    
    let response = client
        .get("http://localhost:3001/")
        .send()
        .await
        .expect("Failed to get index page");

    assert_eq!(response.status(), 200, "Server should return 200 OK");
    
    let body = response.text().await.expect("Failed to read response body");
    assert!(body.contains("MapFlow Studio"), "Response should contain the app title");
    assert!(body.contains("id=\"root\""), "Response should include root container");
}

#[tokio::test]
async fn test_get_empty_config_returns_default() {
    let client = common::start_test_server().await;
    
    let response = client
        .get("http://localhost:3001/config")
        .send()
        .await
        .expect("Failed to get config");

    assert_eq!(response.status(), 200, "GET /config should return 200 OK");
    
    let config: Value = response.json().await.expect("Failed to parse JSON response");
    assert_eq!(config["version"], "0.1.0", "Version should be 0.1.0");
    assert_eq!(config["nodes"].as_array().map(|a| a.len()), Some(0), "Should have no nodes initially");
    assert_eq!(config["edges"].as_array().map(|a| a.len()), Some(0), "Should have no edges initially");
}

#[tokio::test]
async fn test_upload_shapefile_creates_resource_node() {
    let client = common::start_test_server().await;
    
    // Create a minimal shapefile ZIP for testing
    let zip_path = common::create_test_shapefile("test_upload");
    
    let file_part = multipart::Part::bytes(fs::read(&zip_path).unwrap())
        .file_name("test_upload.zip")
        .mime_str("application/zip")
        .unwrap();

    let srid_part = multipart::Part::text("4326");

    let form = multipart::Form::new()
        .part("file", file_part)
        .part("srid", srid_part);

    let response = client
        .post("http://localhost:3001/upload")
        .multipart(form)
        .send()
        .await
        .expect("Failed to upload file");

    assert_eq!(response.status(), 200, "Upload should succeed");
    
    let upload_response: Value = response.json().await.expect("Failed to parse upload response");
    assert_eq!(upload_response["success"], true, "Upload should be successful");
    
    let node = &upload_response["node"];
    assert_eq!(node["type"], "resource", "Created node should be resource type");
    assert_eq!(node["readonly"], true, "Resource nodes should be readonly");
    assert!(node["id"].as_str().unwrap().starts_with("RES_"), "Node ID should start with RES_");
    
    // Verify node appears in config
    let config_response = client
        .get("http://localhost:3001/config")
        .send()
        .await
        .expect("Failed to get config");

    let config: Value = config_response.json().await.expect("Failed to parse config");
    assert_eq!(config["nodes"].as_array().map(|a| a.len()), Some(1), "Config should have 1 node");

    // Fetch resource metadata
    let resource_id = node["id"].as_str().unwrap();
    let metadata_response = client
        .get(&format!("http://localhost:3001/resources/{}/metadata", resource_id))
        .send()
        .await
        .expect("Failed to get resource metadata");

    assert_eq!(metadata_response.status(), 200, "Metadata endpoint should return 200 OK");
    let metadata: Value = metadata_response.json().await.expect("Failed to parse metadata response");
    assert_eq!(metadata["resource_id"], node["id"], "Metadata should match resource id");
    assert!(metadata["fields"].is_array(), "Metadata should include fields array");
}

#[tokio::test]
async fn test_upload_invalid_file_returns_error() {
    let client = common::start_test_server().await;
    
    let invalid_zip = common::create_invalid_zip();
    
    let file_part = multipart::Part::bytes(invalid_zip)
        .file_name("invalid.zip")
        .mime_str("application/zip")
        .unwrap();

    let form = multipart::Form::new()
        .part("file", file_part);

    let response = client
        .post("http://localhost:3001/upload")
        .multipart(form)
        .send()
        .await
        .expect("Failed to upload file");

    assert_eq!(response.status(), 400, "Invalid upload should return 400");
    
    let error_response: Value = response.json().await.expect("Failed to parse error response");
    assert_eq!(error_response["success"], false, "Upload should fail");
    assert!(error_response["error"]["message"].as_str().unwrap().contains("Failed"), "Should contain error message");
}

#[tokio::test]
async fn test_verify_valid_config_passes() {
    let client = common::start_test_server().await;
    
    // Create a valid config
    let config = common::create_valid_config();
    
    let response = client
        .post("http://localhost:3001/verify")
        .json(&config)
        .send()
        .await
        .expect("Failed to verify config");

    assert_eq!(response.status(), 200, "Verify should return 200");
    
    let verify_response: Value = response.json().await.expect("Failed to parse verify response");
    assert_eq!(verify_response["valid"], true, "Valid config should pass verification");
    assert_eq!(verify_response["errors"].as_array().map(|a| a.len()), Some(0), "Should have no errors");
}

#[tokio::test]
async fn test_verify_invalid_zoom_levels_fails() {
    let client = common::start_test_server().await;
    
    let mut config = common::create_valid_config();
    
    // Set invalid zoom levels (min >= max)
    if let Some(layer) = config["nodes"].as_array_mut() {
        for node in layer {
            if node["type"] == "layer" {
                node["minzoom"] = serde_json::json!(18);
                node["maxzoom"] = serde_json::json!(10);
                break;
            }
        }
    }
    
    let response = client
        .post("http://localhost:3001/verify")
        .json(&config)
        .send()
        .await
        .expect("Failed to verify config");

    assert_eq!(response.status(), 200, "Verify should return 200");
    
    let verify_response: Value = response.json().await.expect("Failed to parse verify response");
    assert_eq!(verify_response["valid"], false, "Invalid config should fail");
    assert!(verify_response["errors"].as_array().unwrap().len() > 0, "Should have errors");
}

#[tokio::test]
async fn test_verify_invalid_edge_type_fails() {
    let client = common::start_test_server().await;
    
    let mut config = common::create_valid_config();
    
    // Create an invalid edge (Layer -> Layer)
    config["edges"].as_array_mut().unwrap().push(serde_json::json!({
        "id": "EDGE_INVALID",
        "source": config["nodes"][1]["id"],
        "target": config["nodes"][1]["id"]
    }));
    
    let response = client
        .post("http://localhost:3001/verify")
        .json(&config)
        .send()
        .await
        .expect("Failed to verify config");

    assert_eq!(response.status(), 200, "Verify should return 200");
    
    let verify_response: Value = response.json().await.expect("Failed to parse verify response");
    assert_eq!(verify_response["valid"], false, "Invalid edge should fail");
}

#[tokio::test]
async fn test_apply_config_updates_state() {
    let client = common::start_test_server().await;
    
    let config = common::create_valid_config();
    
    // Apply config
    let apply_response = client
        .post("http://localhost:3001/config")
        .json(&config)
        .send()
        .await
        .expect("Failed to apply config");

    assert_eq!(apply_response.status(), 200, "Apply should succeed");
    
    let apply_result: Value = apply_response.json().await.expect("Failed to parse apply response");
    assert_eq!(apply_result["success"], true, "Apply should be successful");
    
    // Verify config is persisted
    let get_response = client
        .get("http://localhost:3001/config")
        .send()
        .await
        .expect("Failed to get config");

    let persisted_config: Value = get_response.json().await.expect("Failed to parse persisted config");
    assert_eq!(persisted_config["nodes"].as_array().map(|a| a.len()), Some(3), "Should have 3 nodes");
}

#[tokio::test]
async fn test_apply_invalid_config_returns_error() {
    let client = common::start_test_server().await;
    
    let mut config = common::create_valid_config();
    config["version"] = "invalid_version"; // Invalid version
    
    let response = client
        .post("http://localhost:3001/config")
        .json(&config)
        .send()
        .await
        .expect("Failed to apply config");

    assert_eq!(response.status(), 400, "Invalid config should return 400");
    
    let error_response: Value = response.json().await.expect("Failed to parse error response");
    assert_eq!(error_response["success"], false, "Apply should fail");
}

#[tokio::test]
async fn test_tile_endpoint_returns_valid_format() {
    let client = common::start_test_server().await;
    
    // Apply a valid config first
    let config = common::create_valid_config();
    client
        .post("http://localhost:3001/config")
        .json(&config)
        .send()
        .await
        .ok();
    
    // Request a tile
    let response = client
        .get("http://localhost:3001/tiles/10/100/200.pbf")
        .send()
        .await
        .expect("Failed to get tile");

    // Tile may be 404 if no data at that location, but the endpoint should work
    assert!(response.status() == 200 || response.status() == 404, "Tile endpoint should respond");
    
    if response.status() == 200 {
        let content_type = response.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok());
        assert_eq!(content_type, Some("application/x-protobuf"), "Tile should be protobuf format");
    }
}

#[tokio::test]
async fn test_config_validation_error_details_are_meaningful() {
    let client = common::start_test_server().await;
    
    // Create a config with invalid bounds (wrong array length)
    let mut config = common::create_valid_config();
    if let Some(nodes) = config["nodes"].as_array_mut() {
        for node in nodes {
            if node["type"] == "xyz" {
                node["bounds"] = serde_json::json!([0.0, 0.0]); // Only 2 elements, should be 4
                break;
            }
        }
    }
    
    let response = client
        .post("http://localhost:3001/verify")
        .json(&config)
        .send()
        .await
        .expect("Failed to verify config");

    let verify_response: Value = response.json().await.expect("Failed to parse verify response");
    assert_eq!(verify_response["valid"], false, "Invalid bounds should fail");
    
    let errors = verify_response["errors"].as_array().unwrap();
    assert!(errors.iter().any(|e| e["message"].as_str().unwrap().contains("Bounds")), 
            "Error should mention bounds");
}
