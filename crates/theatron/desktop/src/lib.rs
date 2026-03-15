//! Dioxus desktop UI for the Aletheia distributed cognition system.

pub mod api;
pub mod app;
pub mod components;
pub mod layout;
pub mod state;
pub mod views;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Initialize file-based tracing and launch the Dioxus desktop app.
///
/// Sets up structured logging to `~/.local/share/aletheia/desktop.log`,
/// configures the Blitz native window, and starts the event loop.
pub fn run() {
    let _guard = init_tracing();
    tracing::info!("starting aletheia-desktop");

    let config = dioxus_native::Config::new().with_window_attributes(
        dioxus_native::WindowAttributes::default()
            .with_title("Aletheia")
            .with_inner_size(dioxus_native::LogicalSize::new(1200.0, 800.0)),
    );
    dioxus_native::launch_cfg(app::App, Vec::new(), vec![Box::new(config)]);
}

fn init_tracing() -> WorkerGuard {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("aletheia");
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "desktop.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| "theatron_desktop=debug".into());

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    guard
}
