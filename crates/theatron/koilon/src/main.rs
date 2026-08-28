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
// kanon:ignore RUST/box-dyn-error — binary entry point uses Box<dyn Error> for ergonomic top-level error propagation
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // WHY(#7012): previously `.expect()`-ed, treating any prior installation
    // as a programming error. That disagreed with every other first-party
    // binary (aletheia, proskenion), which treat an already-installed
    // provider as steady state. koina::crypto::install_default_provider()
    // is the shared policy: it never panics on a prior installation.
    let _ = koina::crypto::install_default_provider();

    let cli = Cli::parse();
    koilon::run_tui(cli.url, cli.token, cli.agent, cli.session, cli.logout).await?;
    Ok(())
}
