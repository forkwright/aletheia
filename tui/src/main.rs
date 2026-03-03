mod api;
mod app;
mod clipboard;
mod command;
mod config;
mod events;
mod highlight;
mod markdown;
mod msg;
mod theme;
mod view;

use anyhow::Result;
use clap::Parser;
use crossterm::event::EventStream;
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt};

use crate::app::App;
use crate::config::Config;
use crate::events::Event;

#[derive(Parser, Debug)]
#[command(name = "aletheia-tui", about = "Aletheia terminal dashboard")]
struct Cli {
    /// Gateway URL (e.g., http://localhost:18789)
    #[arg(short, long, env = "ALETHEIA_URL")]
    url: Option<String>,

    /// Bearer token for authentication
    #[arg(short, long, env = "ALETHEIA_TOKEN")]
    token: Option<String>,

    /// Agent to focus on startup
    #[arg(short, long)]
    agent: Option<String>,

    /// Session key to open
    #[arg(short, long)]
    session: Option<String>,

    /// Log out and clear saved credentials
    #[arg(long)]
    logout: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // File-based logging — never write to stdout (that's the terminal)
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

    let config = Config::load(cli.url, cli.token, cli.agent, cli.session)?;

    if cli.logout {
        config.clear_credentials()?;
        println!("Credentials cleared.");
        return Ok(());
    }

    let mut app = App::init(config).await?;

    let terminal = ratatui::init();
    let result = run(terminal, &mut app).await;
    ratatui::restore();

    if let Err(ref e) = result {
        eprintln!("Error: {e:#}");
    }

    result
}

async fn run(mut terminal: DefaultTerminal, app: &mut App) -> Result<()> {
    let mut term_events = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(33)); // ~30fps
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        // Render
        terminal.draw(|frame| app.view(frame))?;

        // Take receivers out of app to avoid double &mut borrow in select!
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

        // Put receivers back
        app.restore_sse(sse_rx);
        app.restore_stream(stream_rx);

        // Map event to message, update state
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
