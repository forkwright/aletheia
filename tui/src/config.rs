use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_URL: &str = "http://localhost:18789";

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    pub url: Option<String>,
    pub token: Option<String>,
    pub default_agent: Option<String>,
    pub default_session: Option<String>,
}

impl std::fmt::Debug for ConfigFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigFile")
            .field("url", &self.url)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .field("default_agent", &self.default_agent)
            .field("default_session", &self.default_session)
            .finish()
    }
}

#[derive(Clone)]
pub struct Config {
    pub url: String,
    pub token: Option<String>,
    pub default_agent: Option<String>,
    pub default_session: Option<String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("url", &self.url)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .field("default_agent", &self.default_agent)
            .field("default_session", &self.default_session)
            .finish()
    }
}

impl Config {
    pub fn load(
        cli_url: Option<String>,
        cli_token: Option<String>,
        cli_agent: Option<String>,
        cli_session: Option<String>,
    ) -> Result<Self> {
        let file_config = Self::load_file().unwrap_or_default();

        // CLI args override config file, config file overrides defaults
        Ok(Config {
            url: cli_url
                .or(file_config.url)
                .unwrap_or_else(|| DEFAULT_URL.to_string()),
            token: cli_token.or(file_config.token),
            default_agent: cli_agent.or(file_config.default_agent),
            default_session: cli_session.or(file_config.default_session),
        })
    }

    pub fn clear_credentials(&self) -> Result<()> {
        let path = Self::config_path()?;
        if path.exists() {
            let mut file_config = Self::load_file().unwrap_or_default();
            file_config.token = None;
            let toml_str = toml::to_string_pretty(&file_config)?;
            write_config(&path, &toml_str)?;
            tracing::info!("cleared credentials from {}", path.display());
        }
        Ok(())
    }

    #[expect(dead_code, reason = "called from login flow")]
    pub fn save_token(&self, token: &str) -> Result<()> {
        let path = Self::config_path()?;
        let mut file_config = Self::load_file().unwrap_or_default();
        file_config.token = Some(token.to_string());
        let toml_str = toml::to_string_pretty(&file_config)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_config(&path, &toml_str)?;
        tracing::info!("saved token to {}", path.display());
        Ok(())
    }

    fn config_path() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|d| d.join("aletheia").join("tui.toml"))
            .context("could not determine config directory")
    }

    fn load_file() -> Option<ConfigFile> {
        let path = Self::config_path().ok()?;
        let contents = std::fs::read_to_string(&path).ok()?;
        toml::from_str(&contents).ok()
    }
}

fn write_config(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
