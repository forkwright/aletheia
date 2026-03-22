#![deny(missing_docs)]
//! TUI entry point for the Aletheia client.
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod actions;
mod api;
mod app;
mod clipboard;
mod command;
mod config;
pub(crate) mod diff;
/// TUI-specific error types and result alias.
pub mod error;
mod events;
mod highlight;
mod hyperlink;
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
use crossterm::{
    cursor,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use snafu::prelude::*;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt};

use crate::app::App;
use crate::config::Config;
use crate::error::{IoSnafu, LogDirectiveSnafu};
use crate::events::Event;
use crate::hyperlink::OscLink;

/// Entry point for the TUI, callable from the main `aletheia` binary or standalone.
///
/// Returns a typed snafu error. Binary callers may convert with `.map_err(anyhow::Error::from)`.
#[tracing::instrument(skip_all, fields(url, agent))]
pub async fn run_tui(
    url: Option<String>,
    token: Option<String>,
    agent: Option<String>,
    session: Option<String>,
    logout: bool,
) -> Result<(), error::Error> {
    run_tui_inner(url, token, agent, session, logout).await
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
    tokio::fs::create_dir_all(&log_dir).await.context(IoSnafu {
        context: "create log directory",
    })?;
    let file_appender = rolling::daily(&log_dir, "tui.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("theatron_tui=debug".parse().context(LogDirectiveSnafu)?),
        )
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    tracing::info!("starting aletheia-tui");

    let config = Config::load(url, token, agent, session)?;

    if logout {
        config.clear_credentials()?;
        tracing::info!("credentials cleared");
        return Ok(());
    }

    let mut app = App::init(config).await?;

    let terminal = ratatui::init();
    crossterm::execute!(std::io::stderr(), crossterm::event::EnableMouseCapture).context(
        IoSnafu {
            context: "enable mouse capture",
        },
    )?;
    let _ = crossterm::execute!(std::io::stderr(), cursor::SetCursorStyle::SteadyBlock);
    let result = run_loop(terminal, &mut app).await;
    let _ = crossterm::execute!(std::io::stderr(), crossterm::event::DisableMouseCapture);
    let _ = crossterm::execute!(std::io::stderr(), cursor::SetCursorStyle::DefaultUserShape);
    ratatui::restore();
    // Persist last-active sessions so the TUI resumes at the same place on relaunch.
    crate::app::save_session_state(&app.config, &app.dashboard.saved_sessions);

    if let Err(ref e) = result {
        tracing::error!(error = %e, "tui exited with error");
    }

    result
}

/// Tick interval in milliseconds: drives spinner animation and cursor blink (~60 fps).
const TICK_INTERVAL_MS: u64 = 16;

async fn run_loop(mut terminal: DefaultTerminal, app: &mut App) -> error::Result<()> {
    let mut term_events = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(TICK_INTERVAL_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        // NOTE: Cell lets the draw closure move link data out without needing &mut app.
        let link_cell: std::cell::Cell<Vec<hyperlink::OscLink>> = std::cell::Cell::new(Vec::new());
        terminal
            .draw(|frame| link_cell.set(app.view(frame)))
            .context(IoSnafu {
                context: "terminal draw",
            })?;
        let osc_links = link_cell.into_inner();

        // NOTE: Emit OSC 8 sequences post-render. OSC 8 is independent of visual
        // rendering: re-writing link text wrapped in open/close sequences marks those
        // cells clickable without altering their visual appearance.
        if !osc_links.is_empty() {
            emit_osc8_links(terminal.backend_mut(), &osc_links).context(IoSnafu {
                context: "emit osc8 links",
            })?;
        }

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

        // WHY: Every SSE event (including pings) proves the connection is alive.
        // Without this, only a few handlers update the timestamp, causing the
        // status bar to show "Stale" even when pings arrive regularly.
        if matches!(&event, Event::Sse(_)) {
            app.connection.sse_last_event_at = Some(std::time::Instant::now());
        }
        // WHY: Every stream event resets the stall clock. Any data from the server
        // (text deltas, tool starts, tool results) proves the agent is responsive.
        if matches!(&event, Event::Stream(_)) {
            app.connection.stream_last_event_at = Some(std::time::Instant::now());
        }

        if let Some(msg) = app.map_event(event) {
            app.update(msg).await;
        }

        // PERF: Drain all buffered stream events before the next frame.
        // At high token rates (50-100 tokens/sec) multiple TextDelta events
        // queue between frames. Processing them all here batches the text
        // appends so the next frame renders once with all accumulated deltas.
        drain_pending_stream_events(app).await;

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Drain all currently-buffered stream events without blocking.
///
/// This prevents one-event-per-frame bottlenecks during high-rate streaming.
/// The receiver is temporarily taken from the app and restored after draining.
async fn drain_pending_stream_events(app: &mut App) {
    let Some(mut rx) = app.take_stream() else {
        return;
    };
    while let Ok(stream_event) = rx.try_recv() {
        let event = Event::Stream(stream_event);
        if let Some(msg) = app.map_event(event) {
            app.update(msg).await;
        }
    }
    app.restore_stream(Some(rx));
}

/// Write OSC 8 hyperlink sequences to the terminal **after** ratatui has
/// flushed the frame.
///
/// For each [`OscLink`] we:
/// 1. Save the cursor position.
/// 2. Move to the link's screen coordinates.
/// 3. Emit `ESC ] 8 ;; URL BEL` (OSC 8 open).
/// 4. Re-write the display text with its accent styling.
/// 5. Emit `ESC ] 8 ;; BEL` (OSC 8 close).
/// 6. Reset attributes and restore the cursor.
///
/// This overwrites the same cells ratatui already rendered (visually
/// identical content), but the terminal now associates those cells with
/// the hyperlink, making them clickable in supported terminals.
fn emit_osc8_links<W: std::io::Write>(writer: &mut W, links: &[OscLink]) -> std::io::Result<()> {
    for link in links {
        let (r, g, b) = link.accent;
        crossterm::queue!(
            writer,
            cursor::SavePosition,
            cursor::MoveTo(link.screen_x, link.screen_y),
            SetForegroundColor(Color::Rgb { r, g, b }),
            SetAttribute(Attribute::Underlined),
            crossterm::style::Print(format!(
                "{}{}{}",
                crate::hyperlink::osc8_open(&link.url),
                link.text,
                crate::hyperlink::osc8_close(),
            )),
            SetAttribute(Attribute::Reset),
            ResetColor,
            cursor::RestorePosition,
        )?;
    }
    writer.flush()?;
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
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recv_sse_none_never_resolves() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tx.send(()).unwrap();
        let mut opt: Option<api::sse::SseConnection> = None;
        tokio::select! {
            biased;
            _ = rx => {} // ready immediately: proves recv_sse(None) did not resolve first
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
            _ = rx => {} // ready immediately: proves recv_stream(None) did not resolve first
            _ = recv_stream(&mut opt) => panic!("recv_stream(None) must not resolve"),
        }
    }
}
