//! `aletheia health`: HTTP health check against a running instance.

use anyhow::{Context, Result};
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct HealthArgs {
    /// Server URL to check
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    pub url: String,
}

#[must_use]
pub async fn run(args: &HealthArgs) -> Result<()> {
    let url = &args.url;
    let endpoint = format!("{url}/api/health");
    let resp = reqwest::get(&endpoint).await.map_err(|e| {
        if e.is_connect() {
            anyhow::anyhow!(
                "FAILED: cannot connect to {url}\n  \
                 Is the server running? Start it with: aletheia"
            )
        } else {
            anyhow::anyhow!("FAILED: {e}")
        }
    })?;
    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse health response")?;
    let health_status = body["status"].as_str().unwrap_or("unknown");
    let version = body["version"].as_str().unwrap_or("unknown");
    let uptime = body["uptime_seconds"].as_u64().unwrap_or(0);
    if status.is_success() {
        println!("OK — {health_status} | version {version} | uptime {uptime}s");
    } else {
        println!("{}", serde_json::to_string_pretty(&body)?);
        anyhow::bail!("FAILED: health check returned HTTP {status}");
    }
    Ok(())
}
