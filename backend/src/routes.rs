use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use axum_login::AuthManagerLayerBuilder;
use tower_http::{
    cors::CorsLayer,
    request_id::{MakeRequestUuid, SetRequestIdLayer},
    trace::TraceLayer,
};
use tower_sessions::SessionManagerLayer;

use crate::{
    handlers::{
        check_is_initialized, get_feature_properties, get_file_schema, get_preview_meta,
        get_public_url, health_check, list_files, publish_file, unpublish_file,
    },
    public::{get_public_pmtiles, get_public_tile, get_public_tile_meta, head_public_pmtiles},
    upload::upload_file,
    AppState,
};

pub fn build_api_router(state: AppState) -> Router {
    build_api_router_with_auth(state, true)
}

pub fn build_test_router(state: AppState) -> Router {
    build_api_router_with_auth(state, false)
}

fn build_api_router_with_auth(state: AppState, with_auth: bool) -> Router {
    let allowed_origins = crate::config::read_cors_origins();

    let mut cors = CorsLayer::new()
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::ACCEPT,
            axum::http::header::AUTHORIZATION,
        ])
        .allow_credentials(true);

    for origin in allowed_origins {
        if let Ok(parsed) = origin.parse::<axum::http::HeaderValue>() {
            cors = cors.allow_origin(parsed);
        } else {
            tracing::warn!(origin = %origin, "Failed to parse CORS origin, skipping");
        }
    }

    let session_layer = SessionManagerLayer::new(state.session_store.clone())
        .with_secure(crate::config::read_cookie_secure())
        .with_same_site(tower_cookies::cookie::SameSite::Lax);

    let auth_layer =
        AuthManagerLayerBuilder::new(state.auth_backend.clone(), session_layer).build();

    let auth_router = crate::auth_routes::build_auth_router();
    let public_router = Router::new()
        .route("/health", get(health_check))
        .route("/api/test/is-initialized", get(check_is_initialized))
        .route("/tiles/{slug}/{z}/{x}/{y}", get(get_public_tile))
        .route(
            "/tiles/{slug}",
            get(get_public_pmtiles).head(head_public_pmtiles),
        )
        .route("/tiles/{slug}/meta", get(get_public_tile_meta));

    let mut api_router = Router::new()
        .route("/api/files", get(list_files))
        .route("/api/uploads", post(upload_file))
        .route("/api/files/{id}/preview", get(get_preview_meta))
        .route(
            "/api/files/{id}/tiles/{z}/{x}/{y}",
            get(crate::handlers::get_tile),
        )
        .route(
            "/api/files/{id}/features/{fid}",
            get(get_feature_properties),
        )
        .route("/api/files/{id}/schema", get(get_file_schema))
        .route("/api/files/{id}/publish", post(publish_file))
        .route("/api/files/{id}/unpublish", post(unpublish_file))
        .route("/api/files/{id}/public-url", get(get_public_url));

    if with_auth {
        api_router = api_router.route_layer(axum_login::login_required!(crate::AuthBackend));
    }

    let router = auth_router
        .merge(public_router)
        .merge(api_router)
        .merge(crate::test_routes::add_test_routes(Router::new()));

    let x_request_id = axum::http::header::HeaderName::from_static("x-request-id");

    router
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
        .layer(auth_layer)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
}
