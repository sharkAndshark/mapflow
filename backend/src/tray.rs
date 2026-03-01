#[cfg(windows)]
#[allow(dead_code)]
pub fn create_tray(
    shutdown_tx: tokio::sync::mpsc::UnboundedSender<()>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    use tray_item::{IconSource, TrayItem};

    let mut tray = TrayItem::new("MapFlow", IconSource::Resource("APP_ICON"))?;

    let url = format!("http://localhost:{}", port);
    let url_for_menu = url.clone();
    tray.add_menu_item("Open Web Interface", move || {
        if let Err(e) = open_browser(&url_for_menu) {
            tracing::error!(error = %e, "Failed to open browser");
        }
    })?;

    tray.add_menu_item("Exit", move || {
        tracing::info!("Exit menu item clicked");
        let _ = shutdown_tx.send(());
    })?;

    tracing::info!("System tray icon created");

    std::thread::spawn(move || {
        let _tray = tray;
        std::thread::park();
    });

    Ok(())
}

#[cfg(windows)]
#[allow(dead_code)]
fn open_browser(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    std::process::Command::new("cmd")
        .args(["/C", "start", url])
        .spawn()?;
    Ok(())
}
