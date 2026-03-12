use serde::{Deserialize, Serialize};
use snafu::prelude::*;
use std::path::{Path, PathBuf};

use crate::error::{ConfigDirSnafu, IoSnafu, Result, YamlSnafu};

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
    #[tracing::instrument(skip(cli_token))]
    pub fn load(
        cli_url: Option<String>,
        cli_token: Option<String>,
        cli_agent: Option<String>,
        cli_session: Option<String>,
    ) -> Result<Self> {
        let file_config = Self::load_file().unwrap_or_default();

        Ok(Config {
            url: cli_url
                .or(file_config.url)
                .unwrap_or_else(|| DEFAULT_URL.to_string()),
            token: cli_token.or(file_config.token),
            default_agent: cli_agent.or(file_config.default_agent),
            default_session: cli_session.or(file_config.default_session),
        })
    }

    #[expect(clippy::unused_self, reason = "consistent instance-method API; &self kept for tracing::instrument skip")]
    #[tracing::instrument(skip(self))]
    pub fn clear_credentials(&self) -> Result<()> {
        let path = Self::config_path()?;
        if path.exists() {
            let mut file_config = Self::load_file().unwrap_or_default();
            file_config.token = None;
            let yaml_str = serde_yaml::to_string(&file_config).context(YamlSnafu)?;
            write_config(&path, &yaml_str)?;
            tracing::info!("cleared credentials from {}", path.display());
        }
        Ok(())
    }

    #[expect(dead_code, reason = "called from login flow")]
    #[expect(clippy::unused_self, reason = "consistent instance-method API; &self kept for tracing::instrument skip")]
    #[tracing::instrument(skip(self, token))]
    pub fn save_token(&self, token: &str) -> Result<()> {
        let path = Self::config_path()?;
        let mut file_config = Self::load_file().unwrap_or_default();
        file_config.token = Some(token.to_string());
        let yaml_str = serde_yaml::to_string(&file_config).context(YamlSnafu)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context(IoSnafu {
                context: "create config directory",
            })?;
        }
        write_config(&path, &yaml_str)?;
        tracing::info!("saved token to {}", path.display());
        Ok(())
    }

    fn config_path() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|d| d.join("aletheia").join("tui.yaml"))
            .context(ConfigDirSnafu)
    }

    fn legacy_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("aletheia").join("tui.toml"))
    }

    fn load_file() -> Option<ConfigFile> {
        let path = Self::config_path().ok()?;

        // Try YAML first
        if let Ok(contents) = std::fs::read_to_string(&path) {
            return serde_yaml::from_str(&contents).ok();
        }

        // Fall back to legacy TOML and auto-migrate
        let legacy = Self::legacy_config_path()?;
        let contents = std::fs::read_to_string(&legacy).ok()?;
        let config: ConfigFile = toml::from_str(&contents).ok()?;

        // Auto-migrate: write YAML, rename TOML to .bak
        if let Ok(yaml_str) = serde_yaml::to_string(&config) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if write_config(&path, &yaml_str).is_ok() {
                let backup = legacy.with_extension("toml.bak");
                let _ = std::fs::rename(&legacy, &backup);
                tracing::info!(
                    "migrated config from {} to {}",
                    legacy.display(),
                    path.display()
                );
            }
        }

        Some(config)
    }
}

fn write_config(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content).context(IoSnafu {
        context: "write config file",
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).context(
            IoSnafu {
                context: "set config file permissions",
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_file_config() {
        let config = Config::load(
            Some("http://custom:9999".into()),
            Some("tok".into()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.url, "http://custom:9999");
        assert_eq!(config.token.as_deref(), Some("tok"));
    }

    #[test]
    fn default_url_when_none() {
        let config = Config::load(None, None, None, None).unwrap();
        assert_eq!(config.url, DEFAULT_URL);
    }

    #[test]
    fn yaml_roundtrip() {
        let file = ConfigFile {
            url: Some("http://host:1234".into()),
            token: Some("secret".into()),
            default_agent: Some("syn".into()),
            default_session: None,
        };
        let yaml = serde_yaml::to_string(&file).unwrap();
        let back: ConfigFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(file.url, back.url);
        assert_eq!(file.token, back.token);
        assert_eq!(file.default_agent, back.default_agent);
        assert_eq!(file.default_session, back.default_session);
    }
}
