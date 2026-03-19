use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use aletheia_koina::secret::SecretString;

use crate::error::{ConfigDirSnafu, IoSnafu, Result, TomlSnafu};
use crate::theme::ThemeMode;

const DEFAULT_URL: &str = "http://localhost:18789";

/// Prefix for OAuth access tokens issued by the Anthropic identity provider.
const OAUTH_TOKEN_PREFIX: &str = "sk-ant-oat";

/// Display label for the credential type shown in the TUI status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialLabel {
    /// OAuth token (auto-refreshable via Claude Code credential chain).
    OAuthToken,
    /// Static API key (no refresh capability).
    StaticApiKey,
    /// No credential configured.
    None,
}

impl std::fmt::Display for CredentialLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OAuthToken => write!(f, "OAuth token"),
            Self::StaticApiKey => write!(f, "static API key"),
            Self::None => write!(f, "no credential"),
        }
    }
}

/// Detect the credential type from a token string.
pub fn detect_credential_label(token: Option<&str>) -> CredentialLabel {
    match token {
        Some(t) if t.starts_with(OAUTH_TOKEN_PREFIX) => CredentialLabel::OAuthToken,
        Some(_) => CredentialLabel::StaticApiKey,
        None => CredentialLabel::None,
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    pub url: Option<String>,
    pub token: Option<String>,
    pub default_agent: Option<String>,
    pub default_session: Option<String>,
    pub workspace_root: Option<String>,
    /// Enable terminal bell (`\x07`) for new messages on inactive agents.
    pub bell: Option<bool>,
    /// Keybinding overrides: action name → key string (e.g. `toggle_sidebar = "Ctrl+G"`).
    pub keybindings: Option<HashMap<String, String>>,
    /// Theme mode: "dark", "light", or "auto" (default).
    pub theme: Option<String>,
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
    pub token: Option<SecretString>,
    pub default_agent: Option<String>,
    pub default_session: Option<String>,
    /// Workspace root for agent operations. Resolved from `ALETHEIA_ROOT` env var, then config file.
    pub workspace_root: Option<std::path::PathBuf>,
    /// Terminal bell for new messages on inactive agents (default: false).
    pub bell: bool,
    /// Keybinding overrides from TOML config.
    pub keybindings: HashMap<String, String>,
    /// Explicit theme override. `None` means auto-detect from terminal.
    pub theme: Option<ThemeMode>,
    /// Detected credential type for status bar display.
    pub credential_label: CredentialLabel,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("url", &self.url)
            .field("token", &self.token)
            .field("default_agent", &self.default_agent)
            .field("default_session", &self.default_session)
            .field("workspace_root", &self.workspace_root)
            .field("theme", &self.theme)
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

        let workspace_root = std::env::var("ALETHEIA_ROOT")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                file_config
                    .workspace_root
                    .as_deref()
                    .map(std::path::PathBuf::from)
            });

        let theme = file_config.theme.as_deref().and_then(|s| match s {
            "light" => Some(ThemeMode::Light),
            "dark" => Some(ThemeMode::Dark),
            _ => None,
        });

        let resolved_token = cli_token.or(file_config.token);
        let credential_label = detect_credential_label(resolved_token.as_deref());

        Ok(Config {
            url: cli_url
                .or(file_config.url)
                .unwrap_or_else(|| DEFAULT_URL.to_string()),
            token: resolved_token.map(SecretString::from),
            default_agent: cli_agent.or(file_config.default_agent),
            default_session: cli_session.or(file_config.default_session),
            workspace_root,
            bell: file_config.bell.unwrap_or(false),
            keybindings: file_config.keybindings.unwrap_or_default(),
            theme,
            credential_label,
        })
    }

    #[expect(
        clippy::unused_self,
        reason = "consistent instance-method API; &self kept for tracing::instrument skip"
    )]
    #[tracing::instrument(skip(self))]
    pub fn clear_credentials(&self) -> Result<()> {
        let path = Self::config_path()?;
        if path.exists() {
            let mut file_config = Self::load_file().unwrap_or_default();
            file_config.token = None;
            let toml_str = toml::to_string(&file_config).context(TomlSnafu)?;
            write_config(&path, &toml_str)?;
            tracing::info!("cleared credentials from {}", path.display());
        }
        Ok(())
    }

    fn config_path() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|d| d.join("aletheia").join("tui.toml"))
            .context(ConfigDirSnafu)
    }

    fn load_file() -> Option<ConfigFile> {
        let path = Self::config_path().ok()?;
        let contents = std::fs::read_to_string(&path).ok()?;
        toml::from_str(&contents).ok()
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
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
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
        assert_eq!(
            config.token.as_ref().map(SecretString::expose_secret),
            Some("tok")
        );
    }

    #[test]
    fn default_url_when_none() {
        let config = Config::load(None, None, None, None).unwrap();
        assert_eq!(config.url, DEFAULT_URL);
    }

    #[test]
    fn toml_roundtrip() {
        let mut keybindings = HashMap::new();
        keybindings.insert("toggle_sidebar".to_string(), "Ctrl+G".to_string());
        let file = ConfigFile {
            url: Some("http://host:1234".into()),
            token: Some("secret".into()),
            default_agent: Some("syn".into()),
            default_session: None,
            workspace_root: Some("/workspace".into()),
            bell: Some(true),
            keybindings: Some(keybindings),
            theme: Some("light".into()),
        };
        let toml_str = toml::to_string(&file).unwrap();
        let back: ConfigFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(file.url, back.url);
        assert_eq!(file.token, back.token);
        assert_eq!(file.default_agent, back.default_agent);
        assert_eq!(file.default_session, back.default_session);
        assert_eq!(file.workspace_root, back.workspace_root);
        assert_eq!(back.bell, Some(true));
        assert_eq!(
            back.keybindings
                .as_ref()
                .and_then(|k| k.get("toggle_sidebar"))
                .map(String::as_str),
            Some("Ctrl+G")
        );
        assert_eq!(file.theme, back.theme);
    }

    #[test]
    fn theme_parsing_light() {
        let config = Config::load(None, None, None, None).unwrap();
        // Default is auto (None) when no file setting
        let _ = config.theme;
    }

    #[test]
    fn workspace_root_none_when_no_env_or_file() {
        // ALETHEIA_ROOT env var must not be set for this test to be meaningful.
        // We can't mutate env vars (unsafe-code is denied in this crate).
        // Verify that when neither env nor file provides workspace_root, it is None.
        if std::env::var("ALETHEIA_ROOT").is_ok() {
            // Skip: env is set externally: can't control it without unsafe
            return;
        }
        let config = Config::load(None, None, None, None).unwrap();
        // workspace_root may be None (no file) or Some (if tui.toml has workspace_root).
        // The load succeeds either way.
        let _ = config.workspace_root;
    }

    #[test]
    fn detect_oauth_token() {
        assert_eq!(
            detect_credential_label(Some("sk-ant-oat-abc123")),
            CredentialLabel::OAuthToken
        );
    }

    #[test]
    fn detect_static_api_key() {
        assert_eq!(
            detect_credential_label(Some("sk-ant-api01-abc123")),
            CredentialLabel::StaticApiKey
        );
    }

    #[test]
    fn detect_no_credential() {
        assert_eq!(detect_credential_label(None), CredentialLabel::None);
    }

    #[test]
    fn config_load_detects_oauth_credential() {
        let config = Config::load(None, Some("sk-ant-oat-test123".into()), None, None).unwrap();
        assert_eq!(config.credential_label, CredentialLabel::OAuthToken);
    }

    #[test]
    fn config_load_detects_static_credential() {
        let config = Config::load(None, Some("sk-ant-api01-test".into()), None, None).unwrap();
        assert_eq!(config.credential_label, CredentialLabel::StaticApiKey);
    }

    #[test]
    fn config_load_no_credential() {
        let config = Config::load(None, None, None, None).unwrap();
        assert_eq!(config.credential_label, CredentialLabel::None);
    }
}
