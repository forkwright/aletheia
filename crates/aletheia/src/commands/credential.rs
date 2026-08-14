//! `aletheia credential`: credential status, OAuth refresh, and keyring storage.

use std::path::{Path, PathBuf};

use snafu::prelude::*;

use clap::Subcommand;

use koina::system::{Environment, RealSystem};
use symbolon::credential::CredentialFile;
use taxis::oikos::Oikos;

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Show current credential source, expiry, and provider
    Status {
        /// Show full credential file paths (default: home-relative)
        #[arg(long)]
        verbose: bool,
    },
    /// Force-refresh OAuth token now
    Refresh {
        /// Show full credential file paths in troubleshooting output (default: home-relative)
        #[arg(long)]
        verbose: bool,
    },
    /// Store an API token in the OS keyring
    #[cfg(feature = "keyring")]
    Store {
        /// Token value (reads from stdin if omitted)
        #[arg(long)]
        token: Option<String>,
    },
    /// Remove the stored credential from the OS keyring
    #[cfg(feature = "keyring")]
    Delete,
}

/// Render a credential-related path home-relative unless `verbose` requests the full path.
///
/// WHY: credential status/refresh output is easy to paste into logs, issues, or
/// screenshots — home-relative paths avoid leaking the operator's username/home
/// layout by default while staying useful for troubleshooting.
fn display_path(path: &Path, verbose: bool, env: &impl Environment) -> String {
    if verbose {
        return path.display().to_string();
    }
    let Some(home) = env.var("HOME") else {
        return path.display().to_string();
    };
    match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "credential action dispatch: each action branch is a sequential display sequence; splitting adds indirection"
)]
pub(crate) async fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let cred_path = oikos.credentials().join("anthropic.json");

    match action {
        Action::Status { verbose } => {
            let mut found_any = false;

            if let Some(cred) = CredentialFile::load(&cred_path) {
                found_any = true;
                let cred_type = if cred.has_refresh_token() {
                    "OAuth (auto-refresh)"
                } else {
                    "static API key"
                };
                println!(
                    "Source:        file ({})",
                    display_path(&cred_path, verbose, &RealSystem)
                );
                println!("Type:          {cred_type}");
                println!(
                    "Token:         {}",
                    if cred.token.expose_secret().is_empty() {
                        "MISSING"
                    } else {
                        "present"
                    }
                );
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

            #[cfg(feature = "keyring")]
            {
                use koina::credential::CredentialProvider;
                let keyring = symbolon::credential::KeyringCredentialProvider::for_instance(
                    oikos.root(),
                    "anthropic",
                );
                if let Some(cred) = keyring.get_credential() {
                    if found_any {
                        println!();
                    }
                    found_any = true;
                    println!("Source:        keyring (OS)");
                    println!("Type:          static API key");
                    println!(
                        "Token:         {}",
                        if cred.secret.expose_secret().is_empty() {
                            "MISSING"
                        } else {
                            "present"
                        }
                    );
                }
            }

            // WHY: always check provider env vars, regardless of credential file presence
            let env_vars: &[(&str, &str)] = &[
                ("ANTHROPIC_AUTH_TOKEN", "OAuth token"),
                ("ANTHROPIC_API_KEY", "static API key"),
                ("OPENAI_API_KEY", "static API key"),
            ];
            for (var, key_type) in env_vars {
                if let Some(val) = RealSystem.var(var)
                    && !val.is_empty()
                {
                    if found_any {
                        println!();
                    }
                    found_any = true;
                    println!("Source:        env ({var})");
                    println!("Type:          {key_type}");
                    println!("Token:         present");
                }
            }

            // Check CC provider availability
            if let Some(cc_path) = symbolon::credential::claude_code_default_path() {
                if cc_path.exists() {
                    println!(
                        "CC provider: Claude Code credentials found at {}",
                        display_path(&cc_path, verbose, &RealSystem)
                    );
                    found_any = true;
                } else {
                    println!(
                        "CC provider: {} (not found)",
                        display_path(&cc_path, verbose, &RealSystem)
                    );
                }
            }

            if !found_any {
                println!("No credential found.");
                println!(
                    "Checked: {} (not found)",
                    display_path(&cred_path, verbose, &RealSystem)
                );
                #[cfg(feature = "keyring")]
                println!("Checked: OS keyring (empty)");
                println!("Checked: ANTHROPIC_AUTH_TOKEN (not set)");
                println!("Checked: ANTHROPIC_API_KEY (not set)");
                println!("Checked: OPENAI_API_KEY (not set)");
            }
        }
        Action::Refresh { verbose } => {
            // WHY: static API keys have no refresh token; attempting refresh
            // produces a confusing OAuth troubleshooting message
            if let Some(cred) = CredentialFile::load(&cred_path)
                && !cred.has_refresh_token()
            {
                println!("Credential is a static API key; refresh is not applicable.");
                return Ok(());
            }

            println!("Refreshing OAuth token...");
            match symbolon::credential::force_refresh(&cred_path).await {
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
                Err(e) => whatever!(
                    "refresh failed for credential file at {}: {e}\n\n\
                     Troubleshooting:\n  \
                     1. Verify the file exists: ls -la {}\n  \
                     2. Check it contains a refresh_token: aletheia credential status\n  \
                     3. Ensure network access to console.anthropic.com",
                    display_path(&cred_path, verbose, &RealSystem),
                    display_path(&cred_path, verbose, &RealSystem)
                ),
            }
        }
        #[cfg(feature = "keyring")]
        Action::Store { token } => {
            let token_value = if let Some(t) = token {
                t
            } else {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf).map_err(|e| {
                    crate::error::Error::msg(format!("failed to read token from stdin: {e}"))
                })?;
                buf.trim().to_owned()
            };

            if token_value.is_empty() {
                whatever!("token is empty, nothing to store");
            }

            let keyring = symbolon::credential::KeyringCredentialProvider::for_instance(
                oikos.root(),
                "anthropic",
            );
            keyring.store(&token_value).map_err(|e| {
                crate::error::Error::msg(format!("failed to store credential in OS keyring: {e}"))
            })?;
            println!(
                "Token stored in OS keyring, namespaced to this instance ({})",
                display_path(oikos.root(), false, &RealSystem)
            );
        }
        #[cfg(feature = "keyring")]
        Action::Delete => {
            let keyring = symbolon::credential::KeyringCredentialProvider::for_instance(
                oikos.root(),
                "anthropic",
            );
            keyring.delete().map_err(|e| {
                crate::error::Error::msg(format!(
                    "failed to delete credential from OS keyring: {e}"
                ))
            })?;
            println!("Credential removed from OS keyring");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;

    use super::display_path;
    use koina::system::Environment;

    #[derive(Default)]
    struct TestEnv {
        vars: HashMap<String, String>,
    }

    impl TestEnv {
        fn new() -> Self {
            Self::default()
        }

        fn with_env(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_owned(), value.to_owned());
            self
        }
    }

    impl Environment for TestEnv {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }

        fn var_os(&self, name: &str) -> Option<OsString> {
            self.vars.get(name).map(Into::into)
        }

        fn vars(&self) -> Vec<(String, String)> {
            self.vars
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        }

        fn current_dir(&self) -> std::io::Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from("/test"))
        }

        fn temp_dir(&self) -> std::path::PathBuf {
            std::path::PathBuf::from("/tmp")
        }

        fn current_exe(&self) -> std::io::Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from("/test/bin/aletheia"))
        }

        fn args(&self) -> Vec<String> {
            vec!["aletheia".to_owned()]
        }
    }

    #[test]
    fn display_path_defaults_to_home_relative() {
        let env = TestEnv::new().with_env("HOME", "/home/alice");
        let path =
            std::path::Path::new("/home/alice/aletheia/instance/config/credentials/anthropic.json");

        assert_eq!(
            display_path(path, false, &env),
            "~/aletheia/instance/config/credentials/anthropic.json"
        );
    }

    #[test]
    fn display_path_verbose_shows_full_path() {
        let env = TestEnv::new().with_env("HOME", "/home/alice");
        let path =
            std::path::Path::new("/home/alice/aletheia/instance/config/credentials/anthropic.json");

        assert_eq!(
            display_path(path, true, &env),
            "/home/alice/aletheia/instance/config/credentials/anthropic.json"
        );
    }

    #[test]
    fn display_path_falls_back_to_full_path_outside_home() {
        let env = TestEnv::new().with_env("HOME", "/home/alice");
        let path = std::path::Path::new("/etc/aletheia/anthropic.json");

        assert_eq!(
            display_path(path, false, &env),
            "/etc/aletheia/anthropic.json"
        );
    }

    #[test]
    fn display_path_falls_back_to_full_path_when_home_unset() {
        let env = TestEnv::new();
        let path =
            std::path::Path::new("/home/alice/aletheia/instance/config/credentials/anthropic.json");

        assert_eq!(
            display_path(path, false, &env),
            "/home/alice/aletheia/instance/config/credentials/anthropic.json"
        );
    }
}
