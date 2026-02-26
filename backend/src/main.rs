use anyhow::{bail, Result};
use clap::Parser;
use std::{path::PathBuf, sync::Arc};
use tokio::{fs, sync::Mutex};
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, env = "LISTEN", default_value = ":3000")]
    listen: String,

    #[arg(long, env = "LISTEN_MAX_PORT")]
    listen_max_port: Option<u16>,
}

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
    for port in base_port..=max_port {
        let addr = format!("{}:{}", host, port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => return Ok((listener, port)),
            Err(e) if port == base_port => {
                tracing::debug!(error = %e, port, "Port {} in use, trying next...", port);
            }
            Err(_) => {}
        }
    }
    bail!("No available port in range {}-{}", base_port, max_port);
}

#[cfg(windows)]
fn main() -> Result<()> {
    use std::sync::mpsc;

    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

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

    let rt = tokio::runtime::Runtime::new()?;
    let _guard = rt.enter();

    let _ = std::fs::create_dir_all(&upload_dir);
    let upload_dir_canonical = upload_dir
        .canonicalize()
        .unwrap_or_else(|_| upload_dir.clone());

    let (max_size, max_size_label) = backend::read_max_size_config();
    tracing::info!(max_size, max_size_label, "Upload size limit configured");

    let db = Arc::new(Mutex::new(conn));

    let auth_backend = backend::AuthBackend::new(db.clone());
    let session_store = backend::DuckDBStore::new(db.clone());

    let state = backend::AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size,
        max_size_label,
        auth_backend,
        session_store,
    };

    rt.block_on(async {
        match backend::reconcile_processing_files(&state.db).await {
            Ok(count) => {
                tracing::info!(reconciled = count, "Reconciled processing files on startup")
            }
            Err(e) => tracing::warn!(error = %e, "Failed to reconcile processing files on startup"),
        }
    });

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

    let (listener, actual_port) = rt.block_on(bind_with_fallback(&host, base_port, max_port))?;

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

    if let Err(e) = backend::tray::create_tray(shutdown_tx, actual_port) {
        tracing::error!(error = ?e, "Failed to create system tray");
    }

    let db_for_shutdown = db.clone();
    let shutdown = async move {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
            tracing::info!("Ctrl+C received");
        };

        let tray_exit = async {
            shutdown_rx.recv().ok();
            tracing::info!("Tray exit received");
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

    rt.block_on(axum::serve(listener, app).with_graceful_shutdown(shutdown))?;

    Ok(())
}

#[cfg(not(windows))]
#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleOutputCP(65001);
    }

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

    let (max_size, max_size_label) = backend::read_max_size_config();
    tracing::info!(max_size, max_size_label, "Upload size limit configured");

    let db = Arc::new(Mutex::new(conn));

    let auth_backend = backend::AuthBackend::new(db.clone());
    let session_store = backend::DuckDBStore::new(db.clone());

    let state = backend::AppState {
        upload_dir,
        upload_dir_canonical,
        db: db.clone(),
        max_size,
        max_size_label,
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

    let db_for_shutdown = db.clone();
    let shutdown = async move {
        shutdown_signal().await;
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

#[cfg(not(windows))]
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_listen_addr_port_only() {
        let (host, port) = parse_listen_addr(":3000").unwrap();
        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 3000);
    }

    #[test]
    fn test_listen_addr_host_and_port() {
        let (host, port) = parse_listen_addr("127.0.0.1:8080").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_listen_addr_localhost() {
        let (host, port) = parse_listen_addr("localhost:3000").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 3000);
    }

    #[test]
    fn test_listen_addr_ipv6() {
        let (host, port) = parse_listen_addr("[::1]:8080").unwrap();
        assert_eq!(host, "[::1]");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_listen_addr_missing_colon() {
        let result = parse_listen_addr("3000");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid LISTEN format"), "Error was: {}", err);
    }

    #[test]
    fn test_listen_addr_invalid_port() {
        let result = parse_listen_addr(":abc");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid port"), "Error was: {}", err);
    }

    #[test]
    fn test_port_fallback_finds_next_available() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let bound_port = listener.local_addr().unwrap().port();

            let (_, actual_port) = bind_with_fallback("127.0.0.1", bound_port, bound_port + 10)
                .await
                .unwrap();

            drop(listener);

            assert_ne!(actual_port, bound_port);
            assert!(actual_port > bound_port);
            assert!(actual_port <= bound_port + 10);
        });
    }

    #[test]
    fn test_port_fallback_uses_base_if_available() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let base_port = 45000;
            let (_, actual_port) = bind_with_fallback("127.0.0.1", base_port, base_port + 10)
                .await
                .unwrap();
            assert_eq!(actual_port, base_port);
        });
    }

    #[test]
    fn test_port_exhausted() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut listeners = vec![];
            for port in 46000..=46002 {
                if let Ok(l) = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await {
                    listeners.push(l);
                }
            }
            let result = bind_with_fallback("127.0.0.1", 46000, 46002).await;
            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("No available port in range"),
                "Error was: {}",
                err
            );
        });
    }

    #[test]
    fn test_max_port_less_than_base_port() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = bind_with_fallback("127.0.0.1", 4000, 3000).await;
            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("LISTEN_MAX_PORT (3000) must be >= listen port (4000)"),
                "Error was: {}",
                err
            );
        });
    }
}
