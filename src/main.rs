mod models;
mod db;
mod services;

use axum::{
    extract::{Path, State, Multipart},
    http::{StatusCode, header},
    response::{IntoResponse, Response, Json},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer, services::ServeDir};
use tracing::{info, error, warn, Level};
use tracing_subscriber::{EnvFilter, fmt};

use models::{Config, Result as AppResult};
use db::Database;
use services::{ConfigService, UploadService, TileService};

#[derive(Clone)]
struct AppState {
    db: Database,
    config: Arc<tokio::sync::RwLock<Config>>,
    config_service: ConfigService,
    upload_service: UploadService,
    tile_service: TileService,
}

#[tokio::main]
async fn main() -> AppResult<()> {
    init_logging();

    info!("Starting MapFlow server...");

    if let Err(e) = std::fs::create_dir_all("data") {
        warn!("Failed to ensure data directory exists: {}", e);
    }

    let db = Database::new()?;
    let config_service = ConfigService::new(db.clone());
    let upload_service = UploadService::new(db.clone());
    let tile_service = TileService::new(db.clone());

    let config = config_service.load_config()?;
    let config = Arc::new(tokio::sync::RwLock::new(config));

    let state = AppState {
        db,
        config,
        config_service,
        upload_service,
        tile_service,
    };

    let app = create_router(state);

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);
    let bind_addr = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| {
            models::AppError::Io(format!("Failed to bind to port {}: {}", port, e))
        })?;

    info!("Server listening on http://{}", bind_addr);
    info!("Access the web interface at http://localhost:{}", port);

    axum::serve(listener, app).await
        .map_err(|e| models::AppError::Internal(format!("Server error: {}", e)))?;

    Ok(())
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("mapflow=debug,tower_http=debug,axum=debug"));

    fmt()
        .with_max_level(Level::DEBUG)
        .with_env_filter(filter)
        .init();
}

fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/config", get(get_config).post(post_config))
        .route("/verify", post(verify_config))
        .route("/upload", post(upload_file))
    .route("/tiles/:z/:x/:y", get(get_tile))
        .route("/resources/:id/metadata", get(get_resource_metadata))
        .route_service("/assets", ServeDir::new("dist/assets"))
        .fallback_service(ServeDir::new("dist").precompressed_gzip())
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CorsLayer::permissive()),
        )
}

async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    let config = state.config.read().await;
    Json(config.clone()).into_response()
}

async fn post_config(
    State(state): State<AppState>,
    Json(new_config): Json<Config>,
) -> impl IntoResponse {
    let verify_result = state.config_service.verify(&new_config);

    if !verify_result.valid {
        let errors: Vec<serde_json::Value> = verify_result.errors
            .into_iter()
            .map(|e| serde_json::json!({
                "code": 4000,
                "message": e.message,
                "node_id": e.node_id
            }))
            .collect();

        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "errors": errors
            }))
        ).into_response();
    }

    match state.config_service.save_config(&new_config) {
        Ok(_) => {
            let mut config = state.config.write().await;
            *config = new_config;

            info!("Configuration applied successfully");

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "message": "Configuration applied successfully"
                }))
            ).into_response()
        }
        Err(e) => {
            error!("Failed to save configuration: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "errors": [{
                        "code": e.code(),
                        "message": e.message(),
                        "detail": e.detail()
                    }]
                }))
            ).into_response()
        }
    }
}

async fn verify_config(
    State(state): State<AppState>,
    Json(config): Json<Config>,
) -> impl IntoResponse {
    let verify_result = state.config_service.verify(&config);

    let response = if verify_result.valid {
        serde_json::json!({
            "valid": true,
            "errors": []
        })
    } else {
        let errors: Vec<serde_json::Value> = verify_result.errors
            .into_iter()
            .map(|e| serde_json::json!({
                "code": 4000,
                "message": e.message,
                "node_id": e.node_id
            }))
            .collect();

        serde_json::json!({
            "valid": false,
            "errors": errors
        })
    };

    Json(response)
}

async fn get_resource_metadata(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let config = state.config.read().await;

    let node = match config.find_node(&id) {
        Some(node) => node,
        None => {
            return error_response(
                StatusCode::NOT_FOUND,
                "Resource not found",
                "No resource node with that id",
            );
        }
    };

    let resource = match node.get_resource_data() {
        Some(data) => data,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Invalid resource node",
                "Node is not a resource",
            );
        }
    };

    let fields = match state.db.get_table_info(&resource.duckdb_table_name) {
        Ok(columns) => columns
            .into_iter()
            .filter(|name| name != "geom")
            .collect::<Vec<_>>(),
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read table fields",
                &e.message(),
            );
        }
    };

    let bounds = match state.db.get_table_bounds(&resource.duckdb_table_name) {
        Ok(bounds) => bounds,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read table bounds",
                &e.message(),
            );
        }
    };

    let center = bounds.map(|(minx, miny, maxx, maxy)| {
        vec![(minx + maxx) / 2.0, (miny + maxy) / 2.0]
    });

    Json(serde_json::json!({
        "resource_id": node.id,
        "table_name": resource.duckdb_table_name,
        "srid": resource.srid,
        "fields": fields,
        "bounds": bounds,
        "center": center
    }))
    .into_response()
}

async fn upload_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_name = String::new();
    let mut file_path = String::new();
    let mut srid_override: Option<String> = None;

    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name().unwrap_or("").to_string();

                match name.as_str() {
                    "file" => {
                        file_name = field.file_name().unwrap_or("upload.zip").to_string();
                        file_path = format!("data/{}_{}", chrono::Utc::now().timestamp(), file_name);

                        if let Err(e) = std::fs::create_dir_all("data") {
                            return error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to create data directory",
                                &format!("{}", e),
                            );
                        }

                        let data = field.bytes().await;
                        match data {
                            Ok(bytes) => {
                                if let Err(e) = std::fs::write(&file_path, bytes) {
                                    return error_response(
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        "Failed to save uploaded file",
                                        &format!("{}", e),
                                    );
                                }
                            }
                            Err(e) => {
                                return error_response(
                                    StatusCode::BAD_REQUEST,
                                    "Failed to read uploaded file",
                                    &format!("{}", e),
                                );
                            }
                        }
                    }
                    "srid" => {
                        if let Ok(value) = field.text().await {
                            srid_override = Some(value);
                        }
                    }
                    _ => {}
                }
            }
            Ok(None) => break,
            Err(e) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "Failed to parse multipart form",
                    &format!("{}", e),
                );
            }
        }
    }

    if file_path.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "No file uploaded",
            "Please provide a shapefile ZIP archive",
        );
    }

    match state.upload_service.process_shapefile_upload(&file_path, &file_name, srid_override).await {
        Ok(node) => {
            let mut config = state.config.write().await;
            config.add_node(node.clone());

            if let Err(e) = state.config_service.save_config(&config) {
                error!("Failed to save config after upload: {}", e);
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "node": node
                }))
            ).into_response()
        }
        Err(e) => {
            error!("Failed to process shapefile: {}", e);
            error_response(
                StatusCode::BAD_REQUEST,
                "Failed to process shapefile",
                &e.message(),
            )
        }
    }
}

async fn get_tile(
    State(state): State<AppState>,
    Path((z, x, y)): Path<(u32, u32, String)>,
) -> impl IntoResponse {
    let config = state.config.read().await;

    let y = y.trim_end_matches(".pbf");
    let y: u32 = match y.parse() {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Invalid tile coordinate",
                "y coordinate must be a number",
            );
        }
    };

    match state.tile_service.generate_tile(&config, z, x, y).await {
        Ok(Some(tile)) => {
            let headers = [
                (header::CONTENT_TYPE, "application/x-protobuf"),
                (header::CONTENT_ENCODING, "gzip"),
            ];
            (headers, tile).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            error!("Failed to generate tile: {}", e);
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to generate tile",
                &e.message(),
            )
        }
    }
}

fn error_response(status: StatusCode, message: &str, detail: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "success": false,
            "error": {
                "code": status.as_u16(),
                "message": message,
                "detail": detail
            }
        }))
    ).into_response()
}
