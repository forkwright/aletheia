use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_URL: &str = "http://localhost:18789";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    pub url: Option<String>,
    pub token: Option<String>,
    pub default_agent: Option<String>,
    pub default_session: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub token: Option<String>,
    pub default_agent: Option<String>,
    pub default_session: Option<String>,
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
            std::fs::write(&path, toml_str)?;
            tracing::info!("cleared credentials from {}", path.display());
        }
        Ok(())
    }

    pub fn save_token(&self, token: &str) -> Result<()> {
        let path = Self::config_path()?;
        let mut file_config = Self::load_file().unwrap_or_default();
        file_config.token = Some(token.to_string());
        let toml_str = toml::to_string_pretty(&file_config)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml_str)?;
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
