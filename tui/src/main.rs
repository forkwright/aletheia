use anyhow::Result;
use clap::Parser;

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
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install ring crypto provider");

    let cli = Cli::parse();
    aletheia_tui::run_tui(cli.url, cli.token, cli.agent, cli.session, cli.logout).await
}
