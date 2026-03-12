//! `aletheia credential` — credential status and OAuth refresh.

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;

use aletheia_symbolon::credential::CredentialFile;
use aletheia_taxis::oikos::Oikos;

#[derive(Debug, Clone, Subcommand)]
pub enum Action {
    /// Show current credential source, expiry, and token prefix
    Status,
    /// Force-refresh OAuth token now
    Refresh,
}

fn token_preview(s: &str) -> String {
    if s.len() > 10 {
        format!("{}...{}", &s[..10], &s[s.len() - 3..])
    } else {
        "***".to_owned()
    }
}

pub async fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let cred_path = oikos.credentials().join("anthropic.json");

    match action {
        Action::Status => {
            if let Some(cred) = CredentialFile::load(&cred_path) {
                let cred_type = if cred.has_refresh_token() {
                    "OAuth (auto-refresh)"
                } else {
                    "static API key"
                };
                println!("Source:        file ({})", cred_path.display());
                println!("Type:          {cred_type}");
                println!("Token:         {}", token_preview(&cred.token));
                if let Some(remaining) = cred.seconds_remaining() {
                    let hours = remaining / 3600;
                    let mins = (remaining % 3600) / 60;
                    if remaining > 0 {
                        println!("Expires:       {hours}h {mins}m remaining");
                    } else {
                        println!("Expires:       EXPIRED");
                    }
                } else {
                    println!("Expires:       no expiry set");
                }
                println!(
                    "Refresh token: {}",
                    if cred.has_refresh_token() { "present" } else { "absent" }
                );
            } else {
                // Check env var fallbacks in resolution order.
                let auth_token =
                    std::env::var("ANTHROPIC_AUTH_TOKEN").ok().filter(|v| !v.is_empty());
                let api_key =
                    std::env::var("ANTHROPIC_API_KEY").ok().filter(|v| !v.is_empty());

                match (auth_token, api_key) {
                    (Some(token), _) => {
                        println!("Source:        environment (ANTHROPIC_AUTH_TOKEN)");
                        println!("Type:          OAuth token");
                        println!("Token:         {}", token_preview(&token));
                    }
                    (None, Some(key)) => {
                        println!("Source:        environment (ANTHROPIC_API_KEY)");
                        println!("Type:          static API key");
                        println!("Token:         {}", token_preview(&key));
                    }
                    (None, None) => {
                        println!("No credential found.");
                        println!("Checked: {} (not found)", cred_path.display());
                        println!("Checked: ANTHROPIC_AUTH_TOKEN (not set)");
                        println!("Checked: ANTHROPIC_API_KEY (not set)");
                    }
                }
            }
        }
        Action::Refresh => {
            println!("Refreshing OAuth token...");
            match aletheia_symbolon::credential::force_refresh(&cred_path).await {
                Ok(updated) => {
                    if let Some(remaining) = updated.seconds_remaining() {
                        println!(
                            "Token refreshed — expires in {}h {}m",
                            remaining / 3600,
                            (remaining % 3600) / 60
                        );
                    } else {
                        println!("Token refreshed");
                    }
                }
                Err(e) => anyhow::bail!("refresh failed: {e}"),
            }
        }
    }
    Ok(())
}
