use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use chrono::Utc;
use rand::RngCore;
use regex::Regex;
use sha2::{Digest, Sha256};
use tokio_postgres::NoTls;
use tracing::warn;
use uuid::Uuid;

use crate::{
    http_errors::{bad_request, internal_error},
    models::{
        ErrorResponse, PostgisConnectionConfig, PostgisConnectionTestRequest,
        PostgisConnectionTestResponse, RegisterPostgisSourceRequest, RegisterPostgisSourceResponse,
    },
    AppState,
};

pub const TILE_SOURCE_POSTGIS: &str = "postgis";
const APP_SECRET_ENV: &str = "APP_SECRET";

#[derive(Debug, Clone)]
pub struct PostgisSourceConfig {
    pub host: String,
    pub port: u16,
    pub database_name: String,
    pub username: String,
    pub password: String,
    pub ssl_mode: String,
    pub schema_name: String,
    pub object_name: String,
    pub geom_column: String,
    pub fid_column: String,
}

#[derive(Debug, Clone)]
pub struct PostgisPropertyColumn {
    pub original_name: String,
    pub alias: Option<String>,
}

#[derive(Debug)]
struct ColumnMetadata {
    original_name: String,
    normalized_name: String,
    ordinal: i64,
    mvt_type: String,
}

pub async fn test_postgis_connection(
    State(_state): State<AppState>,
    Json(req): Json<PostgisConnectionTestRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let cfg = validate_connection_config(req.connection).map_err(|e| bad_request(&e))?;
    let (server_version, postgis_version) = probe_postgis_versions(&cfg).await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("PostGIS connection test failed: {e}"),
            }),
        )
    })?;

    Ok(Json(PostgisConnectionTestResponse {
        success: true,
        server_version,
        postgis_version,
    }))
}

pub async fn register_postgis_source(
    State(state): State<AppState>,
    Json(req): Json<RegisterPostgisSourceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let connection_name = req.connection_name.trim().to_string();
    if connection_name.is_empty() {
        return Err(bad_request("connectionName is required"));
    }

    let cfg = validate_connection_config(req.connection).map_err(|e| bad_request(&e))?;
    let schema_name = validate_identifier(req.schema.trim(), "schema")?;
    let object_name = validate_identifier(req.object.trim(), "object")?;
    let geom_column = validate_identifier(req.geometry_column.trim(), "geometryColumn")?;
    let fid_column = validate_identifier(req.fid_column.trim(), "fidColumn")?;
    let display_name = req
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| object_name.clone());

    let relation =
        introspect_relation(&cfg, &schema_name, &object_name, &geom_column, &fid_column).await?;

    let app_secret = std::env::var(APP_SECRET_ENV).map_err(|_| {
        internal_error(format!(
            "Missing required {} environment variable for secure credential storage",
            APP_SECRET_ENV
        ))
    })?;

    let encrypted_password =
        encrypt_secret(&app_secret, &cfg.password).map_err(|e| internal_error(e.as_str()))?;

    let connection_id = Uuid::new_v4().to_string();
    let file_id = Uuid::new_v4().to_string();
    let now = Utc::now().naive_utc();
    let tile_bounds_json = relation
        .bbox_wgs84
        .map(|bbox| serde_json::json!(bbox).to_string());
    let crs = Some(format!("EPSG:{}", relation.srid));
    let source_path = format!(
        "postgis://{}/{}.{}",
        connection_name, schema_name, object_name
    );

    let conn = state.db.lock().await;
    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(internal_error)?;

    let register_result: Result<(), String> = (|| {
        conn.execute(
            "INSERT INTO files (id, name, type, size, uploaded_at, status, crs, path, table_name, error, is_public, tile_source, crs_type, tile_bounds)
             VALUES (?, ?, 'postgis', 0, ?, 'ready', ?, ?, NULL, NULL, FALSE, ?, 'standard', ?)",
            duckdb::params![
                &file_id,
                &display_name,
                &now,
                &crs,
                &source_path,
                TILE_SOURCE_POSTGIS,
                tile_bounds_json
            ],
        )
        .map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT INTO postgis_connections (id, name, host, port, database_name, username, password_encrypted, ssl_mode, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                &connection_id,
                &connection_name,
                &cfg.host,
                i64::from(cfg.port),
                &cfg.database,
                &cfg.username,
                &encrypted_password,
                &cfg.ssl_mode,
                &now,
                &now
            ],
        )
        .map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT INTO postgis_sources (file_id, connection_id, schema_name, object_name, geom_column, fid_column, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                &file_id,
                &connection_id,
                &schema_name,
                &object_name,
                &geom_column,
                &fid_column,
                &now,
                &now
            ],
        )
        .map_err(|e| e.to_string())?;

        conn.execute(
            "DELETE FROM dataset_columns WHERE source_id = ?",
            duckdb::params![&file_id],
        )
        .map_err(|e| e.to_string())?;

        for column in &relation.columns {
            conn.execute(
                "INSERT INTO dataset_columns (source_id, normalized_name, original_name, alias, ordinal, mvt_type)
                 VALUES (?, ?, ?, NULL, ?, ?)",
                duckdb::params![
                    &file_id,
                    &column.normalized_name,
                    &column.original_name,
                    column.ordinal,
                    &column.mvt_type
                ],
            )
            .map_err(|e| e.to_string())?;
        }

        Ok(())
    })();

    match register_result {
        Ok(()) => {
            conn.execute_batch("COMMIT").map_err(internal_error)?;
            drop(conn);
            Ok((
                StatusCode::CREATED,
                Json(RegisterPostgisSourceResponse {
                    file_id,
                    status: "ready".to_string(),
                }),
            ))
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            drop(conn);
            Err(internal_error(format!(
                "Failed to register PostGIS source: {error}"
            )))
        }
    }
}

pub fn fetch_postgis_source_config(
    conn: &duckdb::Connection,
    file_id: &str,
) -> Result<Option<PostgisSourceConfig>, String> {
    let row = conn
        .query_row(
            "SELECT pc.host, pc.port, pc.database_name, pc.username, pc.password_encrypted, pc.ssl_mode,
                    ps.schema_name, ps.object_name, ps.geom_column, ps.fid_column
             FROM postgis_sources ps
             JOIN postgis_connections pc ON ps.connection_id = pc.id
             WHERE ps.file_id = ?",
            duckdb::params![file_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )
        .ok();

    let Some((
        host,
        port,
        database_name,
        username,
        password_encrypted,
        ssl_mode,
        schema_name,
        object_name,
        geom_column,
        fid_column,
    )) = row
    else {
        return Ok(None);
    };

    let app_secret = std::env::var(APP_SECRET_ENV)
        .map_err(|_| format!("Missing required {} environment variable", APP_SECRET_ENV))?;
    let password = decrypt_secret(&app_secret, &password_encrypted)?;

    let port_u16 =
        u16::try_from(port).map_err(|_| format!("Invalid PostGIS port value: {port}"))?;

    Ok(Some(PostgisSourceConfig {
        host,
        port: port_u16,
        database_name,
        username,
        password,
        ssl_mode,
        schema_name,
        object_name,
        geom_column,
        fid_column,
    }))
}

pub async fn query_mvt_tile(
    config: &PostgisSourceConfig,
    properties: &[PostgisPropertyColumn],
    z: i32,
    x: i32,
    y: i32,
    use_aliases: bool,
) -> Result<Option<Vec<u8>>, String> {
    let client = connect_postgis_client(config).await?;
    let relation = qualified_relation_name(&config.schema_name, &config.object_name)?;
    let geom = quote_ident(&config.geom_column)?;
    let fid = quote_ident(&config.fid_column)?;

    let mut property_sql = Vec::new();
    for prop in properties {
        let source = quote_ident(&prop.original_name)?;
        let key = if use_aliases {
            prop.alias
                .as_deref()
                .filter(|value| !value.is_empty())
                .unwrap_or(&prop.original_name)
        } else {
            &prop.original_name
        };
        property_sql.push(format!("{source} AS {}", quote_ident(key)?));
    }

    let extra_props = if property_sql.is_empty() {
        String::new()
    } else {
        format!(", {}", property_sql.join(", "))
    };

    let sql = format!(
        "SELECT ST_AsMVT(tile, 'layer', 4096, 'geom', 'fid')
         FROM (
            SELECT
                ST_AsMVTGeom(
                    ST_Transform({geom}, 3857),
                    ST_TileEnvelope($1, $2, $3),
                    4096, 256, true
                ) AS geom,
                {fid} AS fid
                {extra_props}
            FROM {relation}
            WHERE ST_Intersects(
                ST_Transform({geom}, 3857),
                ST_TileEnvelope($1, $2, $3)
            )
         ) tile"
    );

    let row = client
        .query_opt(&sql, &[&z, &x, &y])
        .await
        .map_err(|e| e.to_string())?;

    let bytes: Option<Vec<u8>> = row.and_then(|r| r.try_get(0).ok());
    Ok(bytes.filter(|blob| !blob.is_empty()))
}

pub async fn query_feature_properties_json(
    config: &PostgisSourceConfig,
    properties: &[PostgisPropertyColumn],
    fid: i64,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    let client = connect_postgis_client(config).await?;
    let relation = qualified_relation_name(&config.schema_name, &config.object_name)?;
    let fid_col = quote_ident(&config.fid_column)?;

    let mut select_columns = Vec::new();
    for prop in properties {
        let source = quote_ident(&prop.original_name)?;
        select_columns.push(format!("{source} AS {}", quote_ident(&prop.original_name)?));
    }

    let select_sql = if select_columns.is_empty() {
        format!("{} AS _placeholder", quote_ident(&config.fid_column)?)
    } else {
        select_columns.join(", ")
    };

    let sql = format!(
        "SELECT row_to_json(t)::text
         FROM (
            SELECT {select_sql}
            FROM {relation}
            WHERE {fid_col} = $1
            LIMIT 1
         ) t"
    );

    let row = client
        .query_opt(&sql, &[&fid])
        .await
        .map_err(|e| e.to_string())?;

    let json_text: Option<String> = row.and_then(|r| r.try_get(0).ok());
    let Some(json_text) = json_text else {
        return Ok(None);
    };

    let value: serde_json::Value = serde_json::from_str(&json_text).map_err(|e| e.to_string())?;
    match value {
        serde_json::Value::Object(map) => Ok(Some(map)),
        _ => Ok(None),
    }
}

fn validate_connection_config(
    config: PostgisConnectionConfig,
) -> Result<PostgisConnectionConfig, String> {
    let PostgisConnectionConfig {
        host,
        port,
        database,
        username,
        password,
        ssl_mode,
    } = config;

    let host = host.trim();
    let database = database.trim();
    let username = username.trim();
    let ssl_mode = ssl_mode.trim().to_ascii_lowercase();

    if host.is_empty() || database.is_empty() || username.is_empty() || password.is_empty() {
        return Err("Connection fields host/database/username/password are required".to_string());
    }
    if port == 0 {
        return Err("Connection port must be greater than 0".to_string());
    }
    if ssl_mode != "disable" {
        return Err("Only sslMode=disable is supported in current MVP".to_string());
    }

    Ok(PostgisConnectionConfig {
        host: host.to_string(),
        port,
        database: database.to_string(),
        username: username.to_string(),
        password,
        ssl_mode,
    })
}

async fn probe_postgis_versions(
    config: &PostgisConnectionConfig,
) -> Result<(String, String), String> {
    let client = connect_postgis_client_from_connection(config).await?;
    let version_row = client
        .query_one("SHOW server_version", &[])
        .await
        .map_err(|e| e.to_string())?;
    let server_version: String = version_row.try_get(0).map_err(|e| e.to_string())?;

    let postgis_row = client
        .query_one("SELECT PostGIS_Full_Version()", &[])
        .await
        .map_err(|e| format!("PostGIS extension unavailable: {e}"))?;
    let postgis_version: String = postgis_row.try_get(0).map_err(|e| e.to_string())?;

    Ok((server_version, postgis_version))
}

struct IntrospectedRelation {
    srid: i32,
    bbox_wgs84: Option<[f64; 4]>,
    columns: Vec<ColumnMetadata>,
}

async fn introspect_relation(
    config: &PostgisConnectionConfig,
    schema_name: &str,
    object_name: &str,
    geom_column: &str,
    fid_column: &str,
) -> Result<IntrospectedRelation, (StatusCode, Json<ErrorResponse>)> {
    let client = connect_postgis_client_from_connection(config)
        .await
        .map_err(|e| bad_request(&format!("Cannot connect to PostGIS: {e}")))?;

    let relation = client
        .query_opt(
            "SELECT c.oid, c.relkind::TEXT
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2
               AND c.relkind IN ('r', 'v', 'm', 'f', 'p')",
            &[&schema_name, &object_name],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to introspect relation: {e}")))?;

    let Some(relation_row) = relation else {
        return Err(bad_request("Target table/view not found"));
    };
    let relation_oid: u32 = relation_row.get(0);
    let relation_kind: String = relation_row.get(1);

    let geom_type_row = client
        .query_opt(
            "SELECT t.typname
             FROM pg_attribute a
             JOIN pg_type t ON t.oid = a.atttypid
             WHERE a.attrelid = $1::OID
               AND a.attnum > 0
               AND NOT a.attisdropped
               AND a.attname = $2",
            &[&relation_oid, &geom_column],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to inspect geometry column: {e}")))?;

    let Some(geom_type_row) = geom_type_row else {
        return Err(bad_request("geometryColumn not found"));
    };
    let geom_type: String = geom_type_row.get(0);
    if geom_type != "geometry" {
        return Err(bad_request(
            "geometryColumn must be a PostGIS geometry column",
        ));
    }

    let fid_type_row = client
        .query_opt(
            "SELECT t.typname
             FROM pg_attribute a
             JOIN pg_type t ON t.oid = a.atttypid
             WHERE a.attrelid = $1::OID
               AND a.attnum > 0
               AND NOT a.attisdropped
               AND a.attname = $2",
            &[&relation_oid, &fid_column],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to inspect fid column: {e}")))?;

    let Some(fid_type_row) = fid_type_row else {
        return Err(bad_request("fidColumn not found"));
    };
    let fid_type: String = fid_type_row.get(0);
    if !matches!(fid_type.as_str(), "int2" | "int4" | "int8") {
        return Err(bad_request("fidColumn must be int2/int4/int8"));
    }

    let relation_name =
        qualified_relation_name(schema_name, object_name).map_err(|e| bad_request(&e))?;
    let geom_ident = quote_ident(geom_column).map_err(|e| bad_request(&e))?;
    let fid_ident = quote_ident(fid_column).map_err(|e| bad_request(&e))?;

    if matches!(relation_kind.as_str(), "r" | "p") {
        let fid_unique_row = client
            .query_one(
                "SELECT EXISTS(
                    SELECT 1
                    FROM pg_index i
                    JOIN pg_attribute a
                      ON a.attrelid = i.indrelid
                     AND a.attnum = ANY(i.indkey)
                    WHERE i.indrelid = $1::OID
                      AND (i.indisunique OR i.indisprimary)
                      AND i.indnkeyatts = 1
                      AND i.indpred IS NULL
                      AND a.attname = $2
                )",
                &[&relation_oid, &fid_column],
            )
            .await
            .map_err(|e| internal_error(format!("Failed to validate fid uniqueness: {e}")))?;
        let fid_has_single_unique_index: bool = fid_unique_row.get(0);
        if !fid_has_single_unique_index {
            return Err(bad_request(
                "fidColumn must be backed by a single-column UNIQUE/PRIMARY KEY index",
            ));
        }
    }

    let fid_data_sql = format!(
        "SELECT
            EXISTS(SELECT 1 FROM {relation_name} WHERE {fid_ident} IS NULL LIMIT 1),
            EXISTS(
                SELECT 1
                FROM (
                    SELECT {fid_ident}
                    FROM {relation_name}
                    GROUP BY {fid_ident}
                    HAVING COUNT(*) > 1
                    LIMIT 1
                ) d
            )"
    );

    let fid_data_row = client
        .query_one(&fid_data_sql, &[])
        .await
        .map_err(|e| internal_error(format!("Failed to validate fid uniqueness: {e}")))?;
    let has_null_fid: bool = fid_data_row.get(0);
    let has_duplicate_fid: bool = fid_data_row.get(1);
    if has_null_fid || has_duplicate_fid {
        if matches!(relation_kind.as_str(), "r" | "p") {
            return Err(bad_request(
                "fidColumn must be unique and non-null across all rows",
            ));
        }
        return Err(bad_request(
            "fidColumn must be unique and non-null for view/foreign sources",
        ));
    }

    let srid_sql = format!(
        "SELECT ST_SRID({geom_ident}) FROM {relation_name} WHERE {geom_ident} IS NOT NULL LIMIT 1"
    );
    let srid_row = client
        .query_opt(&srid_sql, &[])
        .await
        .map_err(|e| internal_error(format!("Failed to detect SRID: {e}")))?;
    let srid = srid_row
        .and_then(|row| row.try_get::<_, Option<i32>>(0).ok().flatten())
        .unwrap_or(0);
    if srid <= 0 {
        return Err(bad_request("Geometry SRID must be a positive EPSG code"));
    }

    let bbox_sql = format!(
        "SELECT ST_XMin(ext), ST_YMin(ext), ST_XMax(ext), ST_YMax(ext)
         FROM (
            SELECT ST_Extent(ST_Transform({geom_ident}, 4326)) AS ext
            FROM {relation_name}
         ) s"
    );

    let bbox_row = client
        .query_opt(&bbox_sql, &[])
        .await
        .map_err(|e| internal_error(format!("Failed to calculate source bbox: {e}")))?;

    let bbox_wgs84 = bbox_row.and_then(|row| {
        let minx = row.try_get::<_, Option<f64>>(0).ok().flatten();
        let miny = row.try_get::<_, Option<f64>>(1).ok().flatten();
        let maxx = row.try_get::<_, Option<f64>>(2).ok().flatten();
        let maxy = row.try_get::<_, Option<f64>>(3).ok().flatten();
        match (minx, miny, maxx, maxy) {
            (Some(a), Some(b), Some(c), Some(d)) => Some([a, b, c, d]),
            _ => None,
        }
    });

    let rows = client
        .query(
            "SELECT column_name, data_type, udt_name, ordinal_position::INT8
             FROM information_schema.columns
             WHERE table_schema = $1 AND table_name = $2
             ORDER BY ordinal_position",
            &[&schema_name, &object_name],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to read source columns: {e}")))?;

    let mut columns = Vec::new();
    let mut used_names = std::collections::HashSet::new();
    used_names.insert("fid".to_string());
    used_names.insert("geom".to_string());

    for row in rows {
        let original_name: String = row.get(0);
        let data_type: String = row.get(1);
        let udt_name: String = row.get(2);
        let ordinal: i64 = row.get(3);

        if original_name == geom_column || original_name == fid_column {
            continue;
        }

        let base =
            normalize_column_name(&original_name).unwrap_or_else(|| format!("col_{ordinal}"));
        let mut normalized_name = base.clone();
        let mut suffix = 2;
        while used_names.contains(&normalized_name) {
            normalized_name = format!("{base}_{suffix}");
            suffix += 1;
        }
        used_names.insert(normalized_name.clone());

        columns.push(ColumnMetadata {
            original_name,
            normalized_name,
            ordinal,
            mvt_type: map_postgres_type_to_mvt(&data_type, &udt_name),
        });
    }

    Ok(IntrospectedRelation {
        srid,
        bbox_wgs84,
        columns,
    })
}

async fn connect_postgis_client_from_connection(
    config: &PostgisConnectionConfig,
) -> Result<tokio_postgres::Client, String> {
    if config.ssl_mode != "disable" {
        return Err("Only sslMode=disable is currently supported".to_string());
    }

    let mut pg = tokio_postgres::Config::new();
    pg.host(&config.host);
    pg.port(config.port);
    pg.user(&config.username);
    pg.password(&config.password);
    pg.dbname(&config.database);
    pg.connect_timeout(std::time::Duration::from_secs(10));

    let (client, connection) = pg.connect(NoTls).await.map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            warn!(error = %e, "PostGIS connection closed with error");
        }
    });
    Ok(client)
}

async fn connect_postgis_client(
    config: &PostgisSourceConfig,
) -> Result<tokio_postgres::Client, String> {
    let request = PostgisConnectionConfig {
        host: config.host.clone(),
        port: config.port,
        database: config.database_name.clone(),
        username: config.username.clone(),
        password: config.password.clone(),
        ssl_mode: config.ssl_mode.clone(),
    };
    connect_postgis_client_from_connection(&request).await
}

fn qualified_relation_name(schema_name: &str, object_name: &str) -> Result<String, String> {
    Ok(format!(
        "{}.{}",
        quote_ident(schema_name)?,
        quote_ident(object_name)?
    ))
}

fn validate_identifier(
    raw: &str,
    field_name: &str,
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(bad_request(&format!("{field_name} is required")));
    }
    let pattern = Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$")
        .map_err(|e| internal_error(format!("Regex init failed: {e}")))?;
    if !pattern.is_match(trimmed) {
        return Err(bad_request(&format!(
            "{field_name} must match [A-Za-z_][A-Za-z0-9_]*"
        )));
    }
    Ok(trimmed.to_string())
}

fn quote_ident(raw: &str) -> Result<String, String> {
    if raw.is_empty() {
        return Err("Invalid identifier: empty".to_string());
    }
    if raw.chars().any(|ch| ch == '\0') {
        return Err("Invalid identifier: contains NUL byte".to_string());
    }
    Ok(format!("\"{}\"", raw.replace('"', "\"\"")))
}

fn derive_key(secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0_u8; 32];
    key.copy_from_slice(&digest);
    key
}

fn encrypt_secret(app_secret: &str, plaintext: &str) -> Result<String, String> {
    let key = derive_key(app_secret);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let mut nonce = [0_u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|e| format!("Encryption failed: {e}"))?;

    let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64_STANDARD.encode(combined))
}

fn decrypt_secret(app_secret: &str, encoded: &str) -> Result<String, String> {
    let decoded = BASE64_STANDARD
        .decode(encoded.as_bytes())
        .map_err(|e| format!("Invalid encrypted payload: {e}"))?;
    if decoded.len() < 13 {
        return Err("Encrypted payload is too short".to_string());
    }

    let (nonce, ciphertext) = decoded.split_at(12);
    let key = derive_key(app_secret);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|e| format!("Decryption failed: {e}"))?;
    String::from_utf8(plaintext).map_err(|e| format!("Decrypted payload is not UTF-8: {e}"))
}

fn map_postgres_type_to_mvt(data_type: &str, udt_name: &str) -> String {
    let udt = udt_name.to_ascii_lowercase();
    if udt == "bool" {
        return "BOOLEAN".to_string();
    }
    if matches!(udt.as_str(), "int2" | "int4") {
        return "INTEGER".to_string();
    }
    if udt == "int8" {
        return "BIGINT".to_string();
    }
    if matches!(udt.as_str(), "float4" | "float8" | "numeric") {
        return "DOUBLE".to_string();
    }

    match data_type.to_ascii_lowercase().as_str() {
        "boolean" => "BOOLEAN".to_string(),
        "smallint" | "integer" => "INTEGER".to_string(),
        "bigint" => "BIGINT".to_string(),
        "real" | "double precision" | "numeric" => "DOUBLE".to_string(),
        _ => "VARCHAR".to_string(),
    }
}

fn normalize_column_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }

    while out.contains("__") {
        out = out.replace("__", "_");
    }

    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        return None;
    }

    let first = out.chars().next().unwrap_or('_');
    let mut out = if first.is_ascii_alphabetic() || first == '_' {
        out
    } else {
        format!("col_{out}")
    };

    if matches!(
        out.as_str(),
        "select" | "from" | "where" | "group" | "order"
    ) {
        out.push('_');
    }
    Some(out)
}

pub fn build_property_columns_for_query(
    conn: &duckdb::Connection,
    source_id: &str,
) -> Result<Vec<PostgisPropertyColumn>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT original_name, alias
             FROM dataset_columns
             WHERE source_id = ?
             ORDER BY ordinal",
        )
        .map_err(|e| e.to_string())?;

    let iter = stmt
        .query_map(duckdb::params![source_id], |row| {
            Ok(PostgisPropertyColumn {
                original_name: row.get(0)?,
                alias: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut properties = Vec::new();
    for item in iter {
        properties.push(item.map_err(|e| e.to_string())?);
    }
    Ok(properties)
}

pub fn build_feature_properties(
    props: &[PostgisPropertyColumn],
    values: &serde_json::Map<String, serde_json::Value>,
) -> Vec<crate::models::FeatureProperty> {
    props
        .iter()
        .map(|prop| crate::models::FeatureProperty {
            key: prop.original_name.clone(),
            value: values
                .get(&prop.original_name)
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            alias: prop.alias.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{quote_ident, validate_connection_config, PostgisConnectionConfig};

    #[test]
    fn quote_ident_allows_quoted_postgres_identifiers() {
        assert_eq!(quote_ident("road name").expect("quoted"), "\"road name\"");
        assert_eq!(quote_ident("1st-class").expect("quoted"), "\"1st-class\"");
        assert_eq!(quote_ident("ab\"cd").expect("quoted"), "\"ab\"\"cd\"");
    }

    #[test]
    fn quote_ident_rejects_empty_or_nul() {
        assert!(quote_ident("").is_err());
        assert!(quote_ident("a\0b").is_err());
    }

    #[test]
    fn validate_connection_config_preserves_password_whitespace() {
        let cfg = PostgisConnectionConfig {
            host: " localhost ".to_string(),
            port: 5432,
            database: " mapflow ".to_string(),
            username: " user ".to_string(),
            password: "  secret with space  ".to_string(),
            ssl_mode: " disable ".to_string(),
        };

        let validated = validate_connection_config(cfg).expect("validated");
        assert_eq!(validated.host, "localhost");
        assert_eq!(validated.database, "mapflow");
        assert_eq!(validated.username, "user");
        assert_eq!(validated.password, "  secret with space  ");
        assert_eq!(validated.ssl_mode, "disable");
    }

    #[test]
    fn validate_connection_config_accepts_whitespace_only_password_but_rejects_empty() {
        let whitespace_password = PostgisConnectionConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "mapflow".to_string(),
            username: "user".to_string(),
            password: "   ".to_string(),
            ssl_mode: "disable".to_string(),
        };
        assert!(validate_connection_config(whitespace_password).is_ok());

        let empty_password = PostgisConnectionConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "mapflow".to_string(),
            username: "user".to_string(),
            password: String::new(),
            ssl_mode: "disable".to_string(),
        };
        assert!(validate_connection_config(empty_password).is_err());
    }
}
