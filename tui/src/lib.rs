//! TUI entry point for the Aletheia client.

mod actions;
mod api;
mod app;
mod clipboard;
mod command;
mod config;
pub(crate) mod error;
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

use crossterm::event::EventStream;
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use snafu::prelude::*;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt};

use crate::app::App;
use crate::config::Config;
use crate::error::{IoSnafu, LogDirectiveSnafu};
use crate::events::Event;

/// Entry point for the TUI, callable from the main `aletheia` binary or standalone.
///
/// Returns `anyhow::Result` as the public boundary so the binary crate can use
/// anyhow for top-level error reporting. All internal code uses `crate::error::Error`.
#[tracing::instrument(skip_all, fields(url, agent))]
pub async fn run_tui(
    url: Option<String>,
    token: Option<String>,
    agent: Option<String>,
    session: Option<String>,
    logout: bool,
) -> anyhow::Result<()> {
    run_tui_inner(url, token, agent, session, logout)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

async fn run_tui_inner(
    url: Option<String>,
    token: Option<String>,
    agent: Option<String>,
    session: Option<String>,
    logout: bool,
) -> error::Result<()> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("aletheia");
    std::fs::create_dir_all(&log_dir).context(IoSnafu {
        context: "create log directory",
    })?;
    let file_appender = rolling::daily(&log_dir, "tui.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("aletheia_tui=debug".parse().context(LogDirectiveSnafu)?),
        )
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
        eprintln!("Error: {e}");
    }

    result
}

async fn run_loop(mut terminal: DefaultTerminal, app: &mut App) -> error::Result<()> {
    let mut term_events = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(33));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        terminal.draw(|frame| app.view(frame)).context(IoSnafu {
            context: "terminal draw",
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recv_sse_none_never_resolves() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tx.send(()).unwrap();
        let mut opt: Option<api::sse::SseConnection> = None;
        tokio::select! {
            biased;
            _ = rx => {} // ready immediately — proves recv_sse(None) did not resolve first
            _ = recv_sse(&mut opt) => panic!("recv_sse(None) must not resolve"),
        }
    }

    #[tokio::test]
    async fn recv_stream_none_never_resolves() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tx.send(()).unwrap();
        let mut opt: Option<tokio::sync::mpsc::Receiver<events::StreamEvent>> = None;
        tokio::select! {
            biased;
            _ = rx => {} // ready immediately — proves recv_stream(None) did not resolve first
            _ = recv_stream(&mut opt) => panic!("recv_stream(None) must not resolve"),
        }
    }
}
