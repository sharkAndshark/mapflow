use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, patch, post, put},
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
    font_handlers::{
        delete_font, get_font, get_public_glyph, list_fonts, publish_font, unpublish_font,
        upload_font,
    },
    handlers::{
        check_is_initialized, get_feature_properties, get_file_schema, get_preview_meta,
        get_public_url, get_settings, health_check, list_files, publish_file, unpublish_file,
        update_crs, update_field_aliases, update_publish_settings, update_settings,
        update_tile_zoom,
    },
    icon_handlers::{delete_icon, get_icon_file, list_icons, update_icon, upload_icon},
    map_handlers::{
        create_map, delete_map, get_field_values, get_map, list_maps, list_preview_sources,
        update_map,
    },
    postgis::{connect_postgis, register_postgis_source, test_postgis_connection},
    public::{get_public_pmtiles, get_public_tile, get_public_tile_meta, head_public_pmtiles},
    upload::upload_file,
    workspace_handlers::{
        create_workspace, delete_workspace, get_current_workspace, get_workspace, invite_member,
        leave_workspace, list_archived_workspaces, list_members, list_workspaces, remove_member,
        restore_workspace, switch_workspace, update_workspace,
    },
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
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::PATCH,
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
        .route("/tiles/{slug}/meta", get(get_public_tile_meta))
        .route(
            "/fonts/{workspace_slug}/{fontstack}/{*range}",
            get(get_public_glyph),
        );

    let mut api_router = Router::new()
        .route("/api/files", get(list_files))
        .route("/api/uploads", post(upload_file))
        .route("/api/settings", get(get_settings).patch(update_settings))
        .route(
            "/api/postgis/connections/test",
            post(test_postgis_connection),
        )
        .route("/api/postgis/connections/connect", post(connect_postgis))
        .route(
            "/api/postgis/sources/register",
            post(register_postgis_source),
        )
        .route(
            "/api/postgis/connections/discover-schemas",
            post(crate::postgis::discover_schemas),
        )
        .route(
            "/api/postgis/connections/discover-tables",
            post(crate::postgis::discover_tables),
        )
        .route(
            "/api/postgis/connections/discover-columns",
            post(crate::postgis::discover_columns),
        )
        .route(
            "/api/postgis/connections/discover-objects",
            post(crate::postgis::discover_objects),
        )
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
        .route("/api/files/{id}/public-url", get(get_public_url))
        .route("/api/files/{id}/crs", put(update_crs))
        .route("/api/files/{id}/zoom", patch(update_tile_zoom))
        .route("/api/files/{id}/field-aliases", patch(update_field_aliases))
        .route(
            "/api/files/{id}/publish-settings",
            patch(update_publish_settings),
        )
        .route(
            "/api/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route("/api/workspaces/archived", get(list_archived_workspaces))
        .route(
            "/api/workspaces/current",
            get(get_current_workspace).put(switch_workspace),
        )
        .route(
            "/api/workspaces/{id}",
            get(get_workspace)
                .put(update_workspace)
                .delete(delete_workspace),
        )
        .route("/api/workspaces/{id}/leave", post(leave_workspace))
        .route("/api/workspaces/{id}/restore", post(restore_workspace))
        .route(
            "/api/workspaces/{id}/members",
            get(list_members).post(invite_member),
        )
        .route(
            "/api/workspaces/{id}/members/{user_id}",
            delete(remove_member),
        )
        .route("/api/fonts", get(list_fonts).post(upload_font))
        .route("/api/fonts/{id}", get(get_font).delete(delete_font))
        .route("/api/fonts/{id}/publish", post(publish_font))
        .route("/api/fonts/{id}/unpublish", post(unpublish_font))
        .route("/api/icons", get(list_icons).post(upload_icon))
        .route("/api/icons/{id}", patch(update_icon).delete(delete_icon))
        .route("/api/icons/{id}/file", get(get_icon_file))
        .route("/api/maps", get(list_maps).post(create_map))
        .route("/api/maps/preview-sources", get(list_preview_sources))
        .route(
            "/api/maps/preview-sources/{sourceId}/field-values",
            get(get_field_values),
        )
        .route(
            "/api/maps/{id}",
            get(get_map).put(update_map).delete(delete_map),
        );

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
