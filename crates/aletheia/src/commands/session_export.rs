//! `aletheia export <session-id>` — export a session as Markdown or JSON.

use std::io::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;

#[derive(Debug, Clone, Args)]
pub struct SessionExportArgs {
    /// Session ID to export
    pub session_id: String,

    /// Output format: `md` (default) or `json`
    #[arg(long, default_value = "md")]
    pub format: ExportFormat,

    /// Write output to this file instead of stdout
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Server URL
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    pub url: String,

    /// Bearer token for authenticated endpoints
    #[arg(long, env = "ALETHEIA_TOKEN")]
    pub token: Option<String>,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ExportFormat {
    Md,
    Json,
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    id: String,
    session_key: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct HistoryResponse {
    messages: Vec<HistoryMessage>,
}

#[derive(Debug, Deserialize)]
struct HistoryMessage {
    role: String,
    content: String,
    tool_name: Option<String>,
    created_at: String,
}

pub async fn run(args: &SessionExportArgs) -> Result<()> {
    let client = build_client(args.token.as_deref())?;

    let session = fetch_session(&client, &args.url, &args.session_id).await?;
    let history = fetch_history(&client, &args.url, &args.session_id).await?;

    let rendered = match args.format {
        ExportFormat::Md => render_markdown(&session, &history),
        ExportFormat::Json => render_json(&session, &history)?,
    };

    write_output(&rendered, args.output.as_deref())
}

fn build_client(token: Option<&str>) -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(tok) = token {
        let value = reqwest::header::HeaderValue::from_str(&format!("Bearer {tok}"))
            .context("invalid token value")?;
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .context("failed to build HTTP client")
}

async fn fetch_session(
    client: &reqwest::Client,
    base_url: &str,
    session_id: &str,
) -> Result<SessionResponse> {
    let url = format!("{base_url}/api/v1/sessions/{session_id}");
    let resp = client.get(&url).send().await.map_err(|e| {
        if e.is_connect() {
            anyhow::anyhow!(
                "cannot connect to {base_url}\n  Is the server running? Start it with: aletheia"
            )
        } else {
            anyhow::anyhow!("request failed: {e}")
        }
    })?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("session not found: {session_id}");
    }
    if !resp.status().is_success() {
        anyhow::bail!("server returned HTTP {}", resp.status());
    }

    resp.json::<SessionResponse>()
        .await
        .context("failed to parse session response")
}

async fn fetch_history(
    client: &reqwest::Client,
    base_url: &str,
    session_id: &str,
) -> Result<HistoryResponse> {
    let url = format!("{base_url}/api/v1/sessions/{session_id}/history");
    let resp = client
        .get(&url)
        .send()
        .await
        .context("failed to fetch session history")?;

    if !resp.status().is_success() {
        anyhow::bail!("history endpoint returned HTTP {}", resp.status());
    }

    resp.json::<HistoryResponse>()
        .await
        .context("failed to parse history response")
}

fn render_markdown(session: &SessionResponse, history: &HistoryResponse) -> String {
    let mut out = String::new();

    out.push_str(&format!("# Session: {}\n", session.session_key));
    out.push_str(&format!("Started: {}\n", session.created_at));

    for msg in &history.messages {
        out.push_str("\n---\n\n");
        match msg.role.as_str() {
            "tool" => {
                let name = msg.tool_name.as_deref().unwrap_or("unknown");
                out.push_str(&format!("## Tool Call: {name} — {}\n", msg.created_at));
                out.push_str(&format!("**Output:** {}\n", msg.content));
            }
            role => {
                let heading = capitalize_first(role);
                out.push_str(&format!("## {heading} — {}\n", msg.created_at));
                out.push_str(&msg.content);
                out.push('\n');
            }
        }
    }

    out
}

fn render_json(session: &SessionResponse, history: &HistoryResponse) -> Result<String> {
    let payload = serde_json::json!({
        "id": session.id,
        "session_key": session.session_key,
        "created_at": session.created_at,
        "messages": history.messages.iter().map(|m| serde_json::json!({
            "role": m.role,
            "content": m.content,
            "tool_name": m.tool_name,
            "created_at": m.created_at,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&payload).context("failed to serialize session to JSON")
}

fn write_output(content: &str, path: Option<&std::path::Path>) -> Result<()> {
    match path {
        Some(p) => std::fs::write(p, content)
            .with_context(|| format!("failed to write to {}", p.display())),
        None => std::io::stdout()
            .write_all(content.as_bytes())
            .context("failed to write to stdout"),
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}
