//! `aletheia credential`: credential status and OAuth refresh.

use std::path::PathBuf;

use snafu::prelude::*;

use clap::Subcommand;

use aletheia_symbolon::credential::CredentialFile;
use aletheia_taxis::oikos::Oikos;

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Show current credential source, expiry, and token prefix
    Status,
    /// Force-refresh OAuth token now
    Refresh,
}

fn token_preview(s: &str) -> String {
    if s.len() > 10 {
        format!(
            "{}...{}",
            s.get(..10).unwrap_or(s),
            s.get(s.len() - 3..).unwrap_or("")
        )
    } else {
        "***".to_owned()
    }
}

pub(crate) async fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let cred_path = oikos.credentials().join("anthropic.json");

    match action {
        Action::Status => {
            let mut found_any = false;

            if let Some(cred) = CredentialFile::load(&cred_path) {
                found_any = true;
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
                    if cred.has_refresh_token() {
                        "present"
                    } else {
                        "absent"
                    }
                );
            }

            // WHY: always check provider env vars, regardless of credential file presence
            let env_vars: &[(&str, &str)] = &[
                ("ANTHROPIC_AUTH_TOKEN", "OAuth token"),
                ("ANTHROPIC_API_KEY", "static API key"),
                ("OPENAI_API_KEY", "static API key"),
            ];
            for (var, key_type) in env_vars {
                if let Ok(val) = std::env::var(var)
                    && !val.is_empty()
                {
                    if found_any {
                        println!();
                    }
                    found_any = true;
                    println!("Source:        env ({var})");
                    println!("Type:          {key_type}");
                    println!("Token:         {}", token_preview(&val));
                }
            }

            if !found_any {
                println!("No credential found.");
                println!("Checked: {} (not found)", cred_path.display());
                println!("Checked: ANTHROPIC_AUTH_TOKEN (not set)");
                println!("Checked: ANTHROPIC_API_KEY (not set)");
                println!("Checked: OPENAI_API_KEY (not set)");
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
                Err(e) => whatever!("refresh failed: {e}"),
            }
        }
    }
    Ok(())
}
