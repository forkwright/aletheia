//! Aletheia cognitive agent runtime — binary entrypoint.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, warn};
use tracing_subscriber::{fmt, EnvFilter};

use aletheia_agora::listener::ChannelListener;
use aletheia_agora::semeion::client::SignalClient;
use aletheia_agora::semeion::SignalProvider;
use aletheia_hermeneus::anthropic::AnthropicProvider;
use aletheia_hermeneus::provider::{ProviderConfig, ProviderRegistry};
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::manager::NousManager;
use aletheia_nous::session::SessionManager;
use aletheia_organon::builtins;
use aletheia_organon::registry::ToolRegistry;
use aletheia_pylon::router::build_router;
use aletheia_pylon::state::AppState;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_taxis::config::resolve_nous;
use aletheia_taxis::loader::load_config;
use aletheia_taxis::oikos::Oikos;

#[derive(Debug, Parser)]
#[command(name = "aletheia", about = "Cognitive agent runtime")]
struct Cli {
    /// Path to instance root directory
    #[arg(short = 'r', long)]
    instance_root: Option<PathBuf>,

    /// Log level (default: info)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Bind address
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Port
    #[arg(short, long, default_value_t = 18789)]
    port: u16,

    /// Emit JSON-structured logs
    #[arg(long)]
    json_logs: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check if the server is running
    Health {
        /// Server URL to check
        #[arg(long, default_value = "http://127.0.0.1:18789")]
        url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(Command::Health { url }) = cli.command {
        return health(&url).await;
    }

    serve(cli).await
}

async fn serve(cli: Cli) -> Result<()> {
    init_tracing(&cli.log_level, cli.json_logs);

    info!("aletheia starting");

    // Oikos — instance directory resolution
    let oikos = match &cli.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    info!(root = %oikos.root().display(), "instance discovered");

    // Config cascade: defaults → YAML → env
    let config = load_config(&oikos).context("failed to load config")?;
    info!(
        port = config.gateway.port,
        agents = config.agents.list.len(),
        "config loaded"
    );

    // Session store
    let db_path = oikos.sessions_db();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create data dir {}", parent.display()))?;
    }
    let session_store = SessionStore::open(&db_path)
        .with_context(|| format!("failed to open session store at {}", db_path.display()))?;
    info!(path = %db_path.display(), "session store opened");

    // JWT manager
    let jwt_manager = JwtManager::new(JwtConfig::default());

    // Build registries for nous actors (NousManager takes Arc ownership)
    let nous_providers = Arc::new(build_provider_registry());
    let nous_tools = Arc::new(build_tool_registry()?);
    let oikos_arc = Arc::new(oikos);

    // Spawn nous actors
    let mut nous_manager = NousManager::new(
        Arc::clone(&nous_providers),
        Arc::clone(&nous_tools),
        Arc::clone(&oikos_arc),
    );

    if config.agents.list.is_empty() {
        warn!("no agents configured — starting with zero actors");
    } else {
        for agent_def in &config.agents.list {
            let resolved = resolve_nous(&config, &agent_def.id);
            let nous_config = NousConfig {
                id: resolved.id,
                model: resolved.model,
                context_window: resolved.context_tokens,
                max_output_tokens: resolved.max_output_tokens,
                bootstrap_max_tokens: resolved.bootstrap_max_tokens,
                thinking_enabled: resolved.thinking_enabled,
                thinking_budget: resolved.thinking_budget,
                max_tool_iterations: resolved.max_tool_iterations,
                loop_detection_threshold: 3,
            };
            nous_manager
                .spawn(nous_config, PipelineConfig::default())
                .await;
        }
        info!(count = nous_manager.count(), "nous actors spawned");
    }

    // Signal channel listener (optional)
    let _listener = start_signal_listener(&config.channels.signal);

    // Pylon HTTP gateway — separate registries since AppState takes owned values
    let state = Arc::new(AppState {
        session_store: Mutex::new(session_store),
        session_manager: SessionManager::new(NousConfig::default()),
        provider_registry: build_provider_registry(),
        tool_registry: build_tool_registry()?,
        oikos: (*oikos_arc).clone(),
        jwt_manager: Arc::new(jwt_manager),
        start_time: Instant::now(),
    });

    let app = build_router(state);

    let bind_addr = format!("{}:{}", cli.bind, cli.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed to bind to {bind_addr}"))?;

    info!(addr = %bind_addr, "pylon listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    info!("shutting down");
    nous_manager.shutdown_all().await;
    info!("shutdown complete");

    Ok(())
}

/// Build a provider registry with Anthropic if API key is available.
fn build_provider_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    match std::env::var("ANTHROPIC_API_KEY") {
        Ok(api_key) if !api_key.is_empty() => {
            let config = ProviderConfig {
                api_key: Some(api_key),
                ..ProviderConfig::default()
            };
            match AnthropicProvider::from_config(&config) {
                Ok(provider) => {
                    registry.register(Box::new(provider));
                    info!("anthropic provider registered");
                }
                Err(e) => warn!(error = %e, "failed to init anthropic provider"),
            }
        }
        _ => warn!("ANTHROPIC_API_KEY not set — no LLM provider"),
    }

    registry
}

/// Build a tool registry with all builtins.
fn build_tool_registry() -> Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    builtins::register_all(&mut registry).context("failed to register builtin tools")?;
    info!(count = registry.definitions().len(), "tools registered");
    Ok(registry)
}

fn start_signal_listener(
    signal_config: &aletheia_taxis::config::SignalConfig,
) -> Option<ChannelListener> {
    if !signal_config.enabled {
        info!("signal channel disabled");
        return None;
    }

    if signal_config.accounts.is_empty() {
        warn!("signal enabled but no accounts configured");
        return None;
    }

    let mut provider = SignalProvider::new();
    for (account_id, account_cfg) in &signal_config.accounts {
        if !account_cfg.enabled {
            continue;
        }
        let base_url = format!(
            "http://{}:{}",
            account_cfg.http_host, account_cfg.http_port
        );
        match SignalClient::new(&base_url) {
            Ok(client) => {
                provider.add_account(account_id.clone(), client);
                info!(account = %account_id, "signal account added");
            }
            Err(e) => {
                warn!(account = %account_id, error = %e, "failed to create signal client");
            }
        }
    }

    let listener = ChannelListener::start(&provider, None);
    info!("signal channel listener started");
    Some(listener)
}

fn init_tracing(log_level: &str, json: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("aletheia={log_level},{log_level}"))
    });

    if json {
        fmt().with_env_filter(filter).json().with_target(true).init();
    } else {
        fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received ctrl+c"),
        () = terminate => info!("received SIGTERM"),
    }
}

async fn health(url: &str) -> Result<()> {
    let endpoint = format!("{url}/api/health");
    let resp = reqwest::get(&endpoint)
        .await
        .with_context(|| format!("failed to connect to {endpoint}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse health response")?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    if !status.is_success() {
        anyhow::bail!("health check returned {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_help_works() {
        let result = Cli::try_parse_from(["aletheia", "--help"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn cli_defaults() {
        let cli = Cli::parse_from(["aletheia"]);
        assert_eq!(cli.port, 18789);
        assert_eq!(cli.bind, "127.0.0.1");
        assert_eq!(cli.log_level, "info");
        assert!(!cli.json_logs);
        assert!(cli.command.is_none());
    }

    #[test]
    fn health_subcommand_parses() {
        let cli = Cli::parse_from(["aletheia", "health", "--url", "http://localhost:9999"]);
        assert!(matches!(cli.command, Some(Command::Health { .. })));
    }
}
