use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_login::AuthSession;
use duckdb::OptionalExt;
use serde::Deserialize;
use tracing::info;

use crate::{
    http_errors::{bad_request, internal_error},
    models::{CreateMapRequest, ErrorResponse, MapItem, PreviewSourceItem, UpdateMapRequest},
    storage::create_id,
    workspace::get_active_workspace_id,
    AppState, AuthBackend,
};

fn read_map_row(row: &duckdb::Row) -> Result<MapItem, duckdb::Error> {
    let created_at: chrono::NaiveDateTime = row.get(5)?;
    let updated_at: chrono::NaiveDateTime = row.get(6)?;
    let published_at: Option<chrono::NaiveDateTime> = row.get(4)?;
    Ok(MapItem {
        id: row.get(0)?,
        name: row.get(1)?,
        style_json: row.get(2)?,
        slug: row.get(3)?,
        is_public: row.get(7)?,
        published_at: published_at.map(|t| t.and_utc().to_rfc3339()),
        created_at: created_at.and_utc().to_rfc3339(),
        updated_at: updated_at.and_utc().to_rfc3339(),
    })
}

pub async fn list_maps(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, style_json, slug, published_at, created_at, updated_at, is_public
             FROM maps
             WHERE workspace_id = ?
             ORDER BY updated_at DESC",
        )
        .map_err(internal_error)?;

    let maps_iter = stmt
        .query_map(duckdb::params![&workspace_id], read_map_row)
        .map_err(internal_error)?;

    let mut maps = Vec::new();
    for m in maps_iter {
        maps.push(m.map_err(internal_error)?);
    }

    drop(conn);
    Ok(Json(maps))
}

pub async fn create_map(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    Json(req): Json<CreateMapRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let name = req.name.trim();
    if name.is_empty() {
        return Err(bad_request("Name cannot be empty"));
    }

    let map_id = create_id();
    info!(map_id = %map_id, name = %name, "Map created");

    let conn = state.db.lock().await;
    conn.execute(
        "INSERT INTO maps (id, name, workspace_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        duckdb::params![&map_id, name, &workspace_id],
    )
    .map_err(internal_error)?;

    let map_item: MapItem = conn
        .query_row(
            "SELECT id, name, style_json, slug, published_at, created_at, updated_at, is_public
             FROM maps WHERE id = ?",
            duckdb::params![&map_id],
            read_map_row,
        )
        .map_err(internal_error)?;
    drop(conn);

    Ok((StatusCode::CREATED, Json(map_item)))
}

pub async fn get_map(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;
    let result: Option<MapItem> = conn
        .query_row(
            "SELECT id, name, style_json, slug, published_at, created_at, updated_at, is_public
             FROM maps WHERE id = ? AND workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            read_map_row,
        )
        .optional()
        .map_err(internal_error)?;
    drop(conn);

    match result {
        Some(map_item) => Ok(Json(map_item)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Map not found".to_string(),
            }),
        )),
    }
}

pub async fn update_map(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<UpdateMapRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    if req.name.as_deref().map(str::trim) == Some("") {
        return Err(bad_request("Name cannot be empty"));
    }

    let conn = state.db.lock().await;

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM maps WHERE id = ? AND workspace_id = ?",
            duckdb::params![&id, &workspace_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(internal_error)?
        > 0;

    if !exists {
        drop(conn);
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Map not found".to_string(),
            }),
        ));
    }

    if let Some(name) = &req.name {
        conn.execute(
            "UPDATE maps SET name = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            duckdb::params![name.trim(), &id],
        )
        .map_err(internal_error)?;
    }

    if let Some(style_json) = &req.style_json {
        conn.execute(
            "UPDATE maps SET style_json = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            duckdb::params![style_json, &id],
        )
        .map_err(internal_error)?;
    }

    let map_item: MapItem = conn
        .query_row(
            "SELECT id, name, style_json, slug, published_at, created_at, updated_at, is_public
             FROM maps WHERE id = ?",
            duckdb::params![&id],
            read_map_row,
        )
        .map_err(internal_error)?;
    drop(conn);

    info!(map_id = %id, "Map updated");
    Ok(Json(map_item))
}

pub async fn delete_map(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;
    let rows_affected = conn
        .execute(
            "DELETE FROM maps WHERE id = ? AND workspace_id = ?",
            duckdb::params![&id, &workspace_id],
        )
        .map_err(internal_error)?;
    drop(conn);

    if rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Map not found".to_string(),
            }),
        ));
    }

    info!(map_id = %id, "Map deleted");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_preview_sources(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, crs, crs_type, data_bounds, status
             FROM files
             WHERE workspace_id = ? AND status = 'ready'
             ORDER BY name",
        )
        .map_err(internal_error)?;

    let rows = stmt
        .query_map(duckdb::params![&workspace_id], |row| {
            Ok(PreviewSourceItem {
                id: row.get(0)?,
                name: row.get(1)?,
                crs: row.get(2)?,
                crs_type: row.get(3)?,
                data_bounds: row.get(4)?,
                status: row.get(5)?,
            })
        })
        .map_err(internal_error)?;

    let mut sources = Vec::new();
    for s in rows {
        sources.push(s.map_err(internal_error)?);
    }

    drop(conn);
    Ok(Json(sources))
}

#[derive(Debug, Deserialize)]
pub struct FieldValuesQuery {
    pub field: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    50
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldValuesResponse {
    pub field: String,
    pub values: Vec<serde_json::Value>,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sorted_values: Option<Vec<f64>>,
}

pub async fn get_field_values(
    auth_session: AuthSession<AuthBackend>,
    State(state): State<AppState>,
    AxumPath(source_id): AxumPath<String>,
    Query(query): Query<FieldValuesQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workspace_id = get_active_workspace_id(&auth_session, &state.db).await?;

    let conn = state.db.lock().await;

    let (status, table_name, tile_source): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, table_name, tile_source FROM files WHERE id = ? AND workspace_id = ?",
            duckdb::params![&source_id, &workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Source not found".to_string(),
                }),
            )
        })?;

    if status != "ready" {
        drop(conn);
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "Source is not ready".to_string(),
            }),
        ));
    }

    let table_name = table_name.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Source does not have an import table".to_string(),
            }),
        )
    })?;

    let tile_source = tile_source.unwrap_or_else(|| "duckdb".to_string());
    if tile_source != "duckdb" {
        drop(conn);
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Field values not available for tile files".to_string(),
            }),
        ));
    }

    let field = query.field.trim();
    if field.is_empty() {
        drop(conn);
        return Err(bad_request("field parameter is required"));
    }

    let mvt_type: Option<String> = conn
        .query_row(
            "SELECT mvt_type FROM dataset_columns WHERE source_id = ? AND normalized_name = ? LIMIT 1",
            duckdb::params![&source_id, &field],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal_error)?;

    let col_type = mvt_type.unwrap_or_else(|| "unknown".to_string());
    let is_numeric =
        col_type.contains("Int") || col_type.contains("Float") || col_type.contains("Double");

    let safe_field = format!("\"{}\"", field.replace('"', "\"\""));

    let limit = query.limit.min(500);

    let mut resp = FieldValuesResponse {
        field: field.to_string(),
        values: Vec::new(),
        r#type: col_type,
        min: None,
        max: None,
        sorted_values: None,
    };

    if is_numeric {
        let stats: Option<(Option<f64>, Option<f64>)> = conn
            .query_row(
                &format!("SELECT MIN({safe_field}::DOUBLE), MAX({safe_field}::DOUBLE) FROM \"{table_name}\" WHERE {safe_field} IS NOT NULL"),
                duckdb::params![],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(internal_error)?;

        if let Some((min_val, max_val)) = stats {
            resp.min = min_val;
            resp.max = max_val;
        }

        let mut stmt = conn
            .prepare(&format!(
                "SELECT {safe_field}::DOUBLE AS v FROM \"{table_name}\" WHERE {safe_field} IS NOT NULL ORDER BY v"
            ))
            .map_err(internal_error)?;

        let rows = stmt
            .query_map(duckdb::params![], |row| {
                let val: f64 = row.get(0)?;
                Ok(val)
            })
            .map_err(internal_error)?;

        let mut all_vals = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for r in rows {
            let v = r.map_err(internal_error)?;
            all_vals.push(v);
            if seen.len() < limit as usize && !seen.contains(&v.to_string()) {
                seen.insert(v.to_string());
                resp.values.push(serde_json::Value::from(v));
            }
        }
        resp.sorted_values = Some(all_vals);
    } else {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT DISTINCT {safe_field} FROM \"{table_name}\" WHERE {safe_field} IS NOT NULL ORDER BY {safe_field} LIMIT {limit}"
            ))
            .map_err(internal_error)?;

        let rows = stmt
            .query_map(duckdb::params![], |row| {
                let val: Option<String> = row.get(0)?;
                Ok(val)
            })
            .map_err(internal_error)?;

        for r in rows {
            if let Some(v) = r.map_err(internal_error)? {
                resp.values.push(serde_json::Value::String(v));
            }
        }
    }

    drop(conn);
    Ok(Json(resp))
}
