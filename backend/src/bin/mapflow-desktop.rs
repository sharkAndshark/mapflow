#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("mapflow-desktop is only supported on Windows.");
    std::process::exit(1);
}

#[cfg(windows)]
use anyhow::{bail, Result};
#[cfg(windows)]
use clap::Parser;
#[cfg(windows)]
use std::{path::PathBuf, sync::Arc};
#[cfg(windows)]
use tokio::fs;
#[cfg(windows)]
use tokio::sync::{Mutex, RwLock};
#[cfg(windows)]
use tower_http::services::{ServeDir, ServeFile};
#[cfg(windows)]
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(windows)]
#[derive(Parser)]
#[command(version, about = "MapFlow desktop (tray) mode")]
struct Cli {
    #[arg(long, env = "LISTEN", default_value = ":3000")]
    listen: String,

    #[arg(long, env = "LISTEN_MAX_PORT")]
    listen_max_port: Option<u16>,
}

#[cfg(windows)]
fn parse_listen_addr(listen: &str) -> Result<(String, u16)> {
    let (host, port_str) = if let Some(stripped) = listen.strip_prefix(':') {
        ("0.0.0.0", stripped)
    } else if let Some(colon_pos) = listen.rfind(':') {
        (&listen[..colon_pos], &listen[colon_pos + 1..])
    } else {
        bail!(
            "Invalid LISTEN format: '{}'. Expected '[host]:port' or ':port'",
            listen
        );
    };

    let port: u16 = port_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid port in LISTEN: '{}'", port_str))?;

    Ok((host.to_string(), port))
}

#[cfg(windows)]
async fn bind_with_fallback(
    host: &str,
    base_port: u16,
    max_port: u16,
) -> Result<(tokio::net::TcpListener, u16)> {
    if max_port < base_port {
        bail!(
            "LISTEN_MAX_PORT ({}) must be >= listen port ({})",
            max_port,
            base_port
        );
    }
    let mut first_bind_error_kind: Option<std::io::ErrorKind> = None;
    let mut first_bind_error_message: Option<String> = None;
    for port in base_port..=max_port {
        let addr = format!("{}:{}", host, port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => return Ok((listener, port)),
            Err(e) if port == base_port => {
                if first_bind_error_kind.is_none() {
                    first_bind_error_kind = Some(e.kind());
                    first_bind_error_message = Some(e.to_string());
                }
                tracing::debug!(error = %e, port, "Port {} in use, trying next...", port);
            }
            Err(e) => {
                if first_bind_error_kind.is_none() {
                    first_bind_error_kind = Some(e.kind());
                    first_bind_error_message = Some(e.to_string());
                }
            }
        }
    }
    if first_bind_error_kind == Some(std::io::ErrorKind::PermissionDenied) {
        let detail = first_bind_error_message
            .unwrap_or_else(|| "permission denied while binding TCP port".to_string());
        bail!(
            "Failed to bind any port in range {}-{} due to permission error: {}",
            base_port,
            max_port,
            detail
        );
    }
    bail!("No available port in range {}-{}", base_port, max_port);
}

#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,backend=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| backend::DEFAULT_DB_PATH.to_string());
    tracing::info!(db_path = %db_path, "Initializing database");
    let db_path = PathBuf::from(db_path);
    let conn = backend::init_database(&db_path);

    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string());
    tracing::info!(upload_dir = %upload_dir, "Using upload directory");
    let upload_dir = PathBuf::from(upload_dir);
    let _ = fs::create_dir_all(&upload_dir).await;

    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let (max_size, max_size_label) = backend::init_max_size_config(&conn);
    tracing::info!(max_size, max_size_label, "Upload size limit configured");

    let db = Arc::new(Mutex::new(conn));
    let auth_backend = backend::AuthBackend::new(db.clone());
    let session_store = backend::DuckDBStore::new(db.clone());

    let state = backend::AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size: Arc::new(RwLock::new(max_size)),
        max_size_label: Arc::new(RwLock::new(max_size_label)),
        auth_backend,
        session_store,
    };

    match backend::reconcile_processing_files(&state.db).await {
        Ok(count) => tracing::info!(reconciled = count, "Reconciled processing files on startup"),
        Err(e) => tracing::warn!(error = %e, "Failed to reconcile processing files on startup"),
    }

    let mut app = backend::build_api_router(state.clone());
    let web_dist = std::env::var("WEB_DIST").unwrap_or_else(|_| "frontend/dist".to_string());
    let web_dist_path = PathBuf::from(&web_dist);
    if web_dist_path.exists() {
        tracing::info!(web_dist = %web_dist, "Serving static files");
        let index_path = web_dist_path.join("index.html");
        app = app.fallback_service(
            ServeDir::new(&web_dist_path).not_found_service(ServeFile::new(index_path)),
        );
    } else {
        #[cfg(feature = "embed-web-dist")]
        {
            tracing::info!(
                web_dist = %web_dist,
                "WEB_DIST not found, serving embedded frontend bundle"
            );
            app = app.fallback(backend::serve_embedded_spa);
        }

        #[cfg(not(feature = "embed-web-dist"))]
        {
            tracing::warn!(
                web_dist = %web_dist,
                "WEB_DIST not found and embedded frontend disabled; frontend routes unavailable"
            );
        }
    }

    let cli = Cli::parse();
    let (host, base_port) = parse_listen_addr(&cli.listen)?;
    let max_port = cli
        .listen_max_port
        .unwrap_or_else(|| base_port.saturating_add(99));
    tracing::info!(listen = %cli.listen, host = %host, base_port, max_port, "Server starting");

    let (listener, actual_port) = bind_with_fallback(&host, base_port, max_port).await?;
    if actual_port != base_port {
        tracing::warn!(
            original_port = base_port,
            actual_port,
            "Port {} was in use, using port {} instead",
            base_port,
            actual_port
        );
    }
    tracing::info!("Listening on http://{}:{}", host, actual_port);

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    if let Err(e) = backend::tray::create_tray(shutdown_tx, actual_port) {
        tracing::error!(error = ?e, "Failed to create system tray - refusing to run desktop mode");
        return Err(e);
    }

    let db_for_shutdown = db.clone();
    let shutdown = async move {
        let ctrl_c = async {
            match tokio::signal::ctrl_c().await {
                Ok(()) => tracing::info!("Ctrl+C received"),
                Err(e) => {
                    tracing::debug!(
                        error = %e,
                        "Ctrl+C handler unavailable (likely no console attached); relying on tray exit"
                    );
                    std::future::pending::<()>().await;
                }
            }
        };

        let tray_exit = async {
            match shutdown_rx.recv().await {
                Some(_) => tracing::info!("Tray exit received"),
                None => tracing::warn!("Tray channel closed unexpectedly"),
            }
        };

        tokio::select! {
            _ = ctrl_c => {},
            _ = tray_exit => {},
        }

        tracing::info!("Shutdown signal received, checkpointing database...");
        let conn = db_for_shutdown.lock().await;
        if let Err(e) = conn.execute("CHECKPOINT", []) {
            tracing::error!(error = %e, "Failed to checkpoint database during shutdown");
        } else {
            tracing::info!("Database checkpoint completed");
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}
