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

pub async fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let cred_path = oikos.credentials().join("anthropic.json");

    match action {
        Action::Status => {
            match CredentialFile::load(&cred_path) {
                Some(cred) => {
                    let token_preview = if cred.token.len() > 10 {
                        format!(
                            "{}...{}",
                            &cred.token[..10],
                            &cred.token[cred.token.len() - 3..]
                        )
                    } else {
                        "***".to_owned()
                    };
                    let cred_type = if cred.has_refresh_token() {
                        "OAuth (auto-refresh)"
                    } else {
                        "static API key"
                    };
                    println!("Source:        file ({})", cred_path.display());
                    println!("Type:          {cred_type}");
                    println!("Token:         {token_preview}");
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
                        if cred.has_refresh_token() {
                            "present"
                        } else {
                            "absent"
                        }
                    );
                }
                None => {
                    // Check env var fallback
                    match std::env::var("ANTHROPIC_API_KEY") {
                        Ok(key) if !key.is_empty() => {
                            let preview = if key.len() > 10 {
                                format!("{}...{}", &key[..10], &key[key.len() - 3..])
                            } else {
                                "***".to_owned()
                            };
                            println!("Source:        environment (ANTHROPIC_API_KEY)");
                            println!("Type:          static API key");
                            println!("Token:         {preview}");
                        }
                        _ => {
                            println!("No credential found.");
                            println!("Checked: {} (not found)", cred_path.display());
                            println!("Checked: ANTHROPIC_API_KEY (not set)");
                        }
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
