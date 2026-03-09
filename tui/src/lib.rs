//! TUI entry point for the Aletheia client.
//!
//! Error handling uses `anyhow` throughout this crate. The TUI is a thin
//! terminal UI layer: all errors surface to the user as display text (via
//! `eprintln!` in `run_tui` or the in-app toast system). No caller outside
//! this crate needs to match on error variants, so typed snafu errors would
//! add complexity with no benefit. Internal state errors use the app's own
//! `Msg::ShowError` path rather than `Result` propagation.

mod actions;
mod api;
mod app;
mod clipboard;
mod command;
mod config;
mod events;
mod highlight;
mod id;
mod keybindings;
mod mapping;
mod markdown;
mod msg;
mod sanitize;
mod state;
mod theme;
mod update;
mod view;

use anyhow::Result;
use crossterm::event::EventStream;
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt};

use crate::app::App;
use crate::config::Config;
use crate::events::Event;

/// Entry point for the TUI, callable from the main `aletheia` binary or standalone.
#[tracing::instrument(skip_all, fields(url, agent))]
pub async fn run_tui(
    url: Option<String>,
    token: Option<String>,
    agent: Option<String>,
    session: Option<String>,
    logout: bool,
) -> Result<()> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("aletheia");
    std::fs::create_dir_all(&log_dir)?;
    let file_appender = rolling::daily(&log_dir, "tui.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("aletheia_tui=debug".parse()?))
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    tracing::info!("starting aletheia-tui");

    let config = Config::load(url, token, agent, session)?;

    if logout {
        config.clear_credentials()?;
        println!("Credentials cleared.");
        return Ok(());
    }

    let mut app = App::init(config).await?;

    let terminal = ratatui::init();
    let result = run_loop(terminal, &mut app).await;
    ratatui::restore();

    if let Err(ref e) = result {
        eprintln!("Error: {e:#}");
    }

    result
}

async fn run_loop(mut terminal: DefaultTerminal, app: &mut App) -> Result<()> {
    let mut term_events = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(33));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        terminal.draw(|frame| app.view(frame))?;

        let mut sse_rx = app.take_sse();
        let mut stream_rx = app.take_stream();

        let event = tokio::select! {
            biased;

            Some(Ok(term_event)) = term_events.next() => {
                Event::Terminal(term_event)
            }

            Some(sse_event) = recv_sse(&mut sse_rx) => {
                Event::Sse(sse_event)
            }

            Some(stream_event) = recv_stream(&mut stream_rx) => {
                Event::Stream(stream_event)
            }

            _ = tick.tick() => {
                Event::Tick
            }
        };

        app.restore_sse(sse_rx);
        app.restore_stream(stream_rx);

        if let Some(msg) = app.map_event(event) {
            app.update(msg).await;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

async fn recv_sse(sse: &mut Option<api::sse::SseConnection>) -> Option<api::types::SseEvent> {
    match sse {
        Some(conn) => conn.next().await,
        None => std::future::pending().await,
    }
}

async fn recv_stream(
    rx: &mut Option<tokio::sync::mpsc::Receiver<events::StreamEvent>>,
) -> Option<events::StreamEvent> {
    match rx {
        Some(r) => r.recv().await,
        None => std::future::pending().await,
    }
}
