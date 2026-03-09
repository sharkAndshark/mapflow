use anyhow::{bail, Result};
use clap::Parser;
use std::{path::PathBuf, sync::Arc};
use tokio::fs;
use tokio::sync::{Mutex, RwLock};
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

    #[cfg(windows)]
    windows_console::install_close_handler()?;

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

    let web_dist = std::env::var("WEB_DIST").unwrap_or_else(|_| "frontend/dist".to_string());
    let web_dist_path = PathBuf::from(&web_dist);
    let public_viewer_available = backend::detect_public_viewer_available(&web_dist_path);
    backend::set_public_viewer_available(public_viewer_available);
    let web_dist_index = web_dist_path.join("index.html");

    let mut app = backend::build_api_router(state.clone());

    if web_dist_index.is_file() {
        tracing::info!(web_dist = %web_dist, "Serving static files");
        app = app.fallback_service(
            ServeDir::new(&web_dist_path).not_found_service(ServeFile::new(web_dist_index)),
        );
    } else {
        #[cfg(feature = "embed-web-dist")]
        {
            if backend::embedded_spa_available() {
                if web_dist_path.exists() {
                    tracing::warn!(
                        web_dist = %web_dist,
                        "WEB_DIST exists but index.html is missing, serving embedded frontend bundle"
                    );
                } else {
                    tracing::info!(
                        web_dist = %web_dist,
                        "WEB_DIST not found, serving embedded frontend bundle"
                    );
                }
                app = app.fallback(backend::serve_embedded_spa);
            } else {
                tracing::warn!(
                    web_dist = %web_dist,
                    "Embedded frontend bundle is unavailable; frontend routes unavailable"
                );
            }
        }

        #[cfg(not(feature = "embed-web-dist"))]
        {
            tracing::warn!(
                web_dist = %web_dist,
                "WEB_DIST is missing index.html and embedded frontend is disabled; frontend routes unavailable"
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
        #[cfg(windows)]
        windows_console::mark_shutdown_complete();
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}

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

    #[cfg(unix)]
    let hangup = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    #[cfg(not(unix))]
    let hangup = std::future::pending::<()>();

    #[cfg(windows)]
    let console_close = windows_console::wait_for_close_signal();

    #[cfg(not(windows))]
    let console_close = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
        _ = hangup => {},
        _ = console_close => {},
    }
}

#[cfg(windows)]
mod windows_console {
    use anyhow::Result;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};
    use tokio::sync::Notify;
    use windows_sys::Win32::System::Console::{
        SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT, CTRL_LOGOFF_EVENT,
        CTRL_SHUTDOWN_EVENT,
    };

    static CLOSE_NOTIFY: OnceLock<Notify> = OnceLock::new();
    static CLOSE_SIGNALLED: AtomicBool = AtomicBool::new(false);
    static SHUTDOWN_COMPLETE: AtomicBool = AtomicBool::new(false);

    pub fn install_close_handler() -> Result<()> {
        CLOSE_NOTIFY.get_or_init(Notify::new);
        CLOSE_SIGNALLED.store(false, Ordering::SeqCst);
        SHUTDOWN_COMPLETE.store(false, Ordering::SeqCst);
        let registered = unsafe { SetConsoleCtrlHandler(Some(handle_console_ctrl), 1) };
        if registered == 0 {
            anyhow::bail!("Failed to register Windows console close handler");
        }
        Ok(())
    }

    pub fn mark_shutdown_complete() {
        SHUTDOWN_COMPLETE.store(true, Ordering::SeqCst);
    }

    pub async fn wait_for_close_signal() {
        let notify = CLOSE_NOTIFY.get_or_init(Notify::new);
        loop {
            if CLOSE_SIGNALLED.load(Ordering::SeqCst) {
                return;
            }
            notify.notified().await;
        }
    }

    unsafe extern "system" fn handle_console_ctrl(ctrl_type: u32) -> i32 {
        let is_close_like_event = matches!(
            ctrl_type,
            CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT
        );
        let should_handle =
            matches!(ctrl_type, CTRL_C_EVENT | CTRL_BREAK_EVENT) || is_close_like_event;

        if !should_handle {
            return 0;
        }

        CLOSE_SIGNALLED.store(true, Ordering::SeqCst);
        if let Some(notify) = CLOSE_NOTIFY.get() {
            notify.notify_one();
        }

        if is_close_like_event {
            wait_for_shutdown_completion();
        }
        1
    }

    fn wait_for_shutdown_completion() {
        const WAIT_SLICE_MS: u64 = 25;
        const MAX_WAIT_MS: u64 = 4_000;

        let deadline = Instant::now() + Duration::from_millis(MAX_WAIT_MS);
        while !SHUTDOWN_COMPLETE.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(WAIT_SLICE_MS));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_permission_denied_error_message(message: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        lower.contains("permission denied")
            || lower.contains("operation not permitted")
            || lower.contains("due to permission error")
    }

    fn should_skip_port_bind_tests(err: &str) -> bool {
        is_permission_denied_error_message(err)
    }

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
            let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
                Ok(v) => v,
                Err(e) => {
                    if is_permission_denied_error_message(&e.to_string()) {
                        eprintln!("Skipping port bind test due to sandbox restrictions: {}", e);
                        return;
                    }
                    panic!("failed to acquire test port: {}", e);
                }
            };
            let bound_port = listener.local_addr().unwrap().port();

            let (_, actual_port) =
                match bind_with_fallback("127.0.0.1", bound_port, bound_port + 10).await {
                    Ok(v) => v,
                    Err(e) if should_skip_port_bind_tests(&e.to_string()) => {
                        eprintln!("Skipping port bind test due to sandbox restrictions: {}", e);
                        return;
                    }
                    Err(e) => panic!("bind_with_fallback should succeed: {}", e),
                };

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
            let (_, actual_port) =
                match bind_with_fallback("127.0.0.1", base_port, base_port + 10).await {
                    Ok(v) => v,
                    Err(e) if should_skip_port_bind_tests(&e.to_string()) => {
                        eprintln!("Skipping port bind test due to sandbox restrictions: {}", e);
                        return;
                    }
                    Err(e) => panic!("bind_with_fallback should succeed: {}", e),
                };
            assert_eq!(actual_port, base_port);
        });
    }

    #[test]
    fn test_port_exhausted() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut listeners = vec![];
            for port in 46000..=46002 {
                match tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await {
                    Ok(l) => listeners.push(l),
                    Err(e) if is_permission_denied_error_message(&e.to_string()) => {
                        eprintln!("Skipping port bind test due to sandbox restrictions: {}", e);
                        return;
                    }
                    Err(_) => {}
                }
            }
            let result = bind_with_fallback("127.0.0.1", 46000, 46002).await;
            if let Err(e) = &result {
                if should_skip_port_bind_tests(&e.to_string()) {
                    eprintln!("Skipping port bind test due to sandbox restrictions: {}", e);
                    return;
                }
            }
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
