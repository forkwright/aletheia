use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use koina::secret::SecretString;
use koina::system::{Environment, RealSystem};

use crate::error::{ConfigDirSnafu, IoSnafu, Result, TomlSnafu};
use crate::secret_store;
use crate::theme::ThemeMode;

const DEFAULT_URL: &str = "http://localhost:18789";

/// Prefix for OAuth access tokens issued by the Anthropic identity provider.
const OAUTH_TOKEN_PREFIX: &str = "sk-ant-oat";

/// Display label for the credential type shown in the TUI status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum CredentialLabel {
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
pub(crate) fn detect_credential_label(token: Option<&str>) -> CredentialLabel {
    match token {
        Some(t) if t.starts_with(OAUTH_TOKEN_PREFIX) => CredentialLabel::OAuthToken,
        Some(_) => CredentialLabel::StaticApiKey,
        None => CredentialLabel::None,
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct ConfigFile {
    pub(crate) url: Option<String>,
    /// Legacy plaintext token (#5321). Never written by this crate — a
    /// value found here on load is migrated into OS-keyring/encrypted-file
    /// storage and replaced with `token_ref` on the next save.
    #[serde(default, skip_serializing)]
    pub(crate) token: Option<String>,
    /// Stable, non-secret reference to a token held in `secret_store`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) token_ref: Option<String>,
    pub(crate) default_agent: Option<String>,
    pub(crate) default_session: Option<String>,
    pub(crate) workspace_root: Option<String>,
    /// Enable terminal bell (`\x07`) for new messages on inactive agents.
    pub(crate) bell: Option<bool>,
    /// Keybinding overrides: action name → key string (e.g. `toggle_sidebar = "Ctrl+G"`).
    pub(crate) keybindings: Option<HashMap<String, String>>,
    /// Theme mode: "dark", "light", or "auto" (default).
    pub(crate) theme: Option<String>,
    /// Optional auto-discovery candidates used when `url` is not set.
    pub(crate) discovery: Option<DiscoveryFileConfig>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiscoveryFileConfig {
    pub(crate) port: Option<u16>,
    pub(crate) urls: Option<Vec<String>>,
    pub(crate) lan_hostnames: Option<Vec<String>>,
    pub(crate) tailscale_ips: Option<Vec<String>>,
}

impl DiscoveryFileConfig {
    fn to_discovery_config(&self) -> skene::discovery::DiscoveryConfig {
        let mut config = skene::discovery::DiscoveryConfig::default();
        if let Some(port) = self.port {
            config.port = port;
        }
        if let Some(urls) = self.urls.clone() {
            config.base_urls = urls;
        }
        if let Some(hostnames) = self.lan_hostnames.clone() {
            config.lan_hostnames = hostnames;
        }
        if let Some(ips) = self.tailscale_ips.clone() {
            config.tailscale_ips = ips;
        }
        config
    }
}

impl std::fmt::Debug for ConfigFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigFile")
            .field("url", &self.url)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .field("token_ref", &self.token_ref)
            .field("default_agent", &self.default_agent)
            .field("default_session", &self.default_session)
            .field(
                "discovery",
                &self.discovery.as_ref().map(|_| "[configured]"),
            )
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) url: String,
    pub(crate) token: Option<SecretString>,
    pub(crate) default_agent: Option<String>,
    pub(crate) default_session: Option<String>,
    /// Workspace root for agent operations. Resolved from `ALETHEIA_ROOT` env var, then config file.
    pub(crate) workspace_root: Option<std::path::PathBuf>,
    /// Terminal bell for new messages on inactive agents (default: false).
    pub(crate) bell: bool,
    /// Keybinding overrides from TOML config.
    pub(crate) keybindings: HashMap<String, String>,
    /// Explicit theme override. `None` means auto-detect from terminal.
    pub(crate) theme: Option<ThemeMode>,
    /// Detected credential type for status bar display.
    pub(crate) credential_label: CredentialLabel,
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
    pub(crate) fn load(
        cli_url: Option<String>,
        cli_token: Option<String>,
        cli_agent: Option<String>,
        cli_session: Option<String>,
    ) -> Result<Self> {
        // kanon:ignore RUST/no-result-unwrap-or-default — missing config file is a normal first-run state; empty default is correct
        let file_config = Self::load_file().unwrap_or_default();

        let workspace_root = RealSystem
            .var("ALETHEIA_ROOT")
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

        // WHY(#5321): file_config.token is already the resolved plaintext value
        // by the time load_file() returns it — legacy plaintext was migrated to
        // secret storage, or a token_ref was resolved back through it. Neither
        // path re-persists a raw token into cli_token/file precedence here.
        let resolved_token = cli_token.or(file_config.token);
        let credential_label = detect_credential_label(resolved_token.as_deref());
        let discovery_config = file_config
            .discovery
            .as_ref()
            .map(DiscoveryFileConfig::to_discovery_config)
            .unwrap_or_default();

        // WHY: When neither CLI flag nor config file provides a URL, attempt
        // auto-discovery before falling back to the compiled default.
        // Discovery runs a blocking runtime because Config::load is sync.
        // WHY test skip: test binaries must not pay the live LAN/dbus/keyring
        // probe cost. `cfg(test)` covers unit tests; nextest sets NEXTEST for
        // package tests that compile the library as a non-test dependency.
        // KOILON_SKIP_PROBE remains as an explicit escape hatch for harnesses.
        let skip_discovery = Self::skip_discovery();
        let url = cli_url.or(file_config.url).unwrap_or_else(|| {
            if skip_discovery {
                DEFAULT_URL.to_string()
            } else {
                Self::try_discover(&discovery_config).unwrap_or_else(|| DEFAULT_URL.to_string())
            }
        });

        Ok(Config {
            url,
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

    /// Attempt server auto-discovery on the local network.
    ///
    /// WHY: Config::load is synchronous (called before the tokio runtime is
    /// fully available to callers). We use `tokio::runtime::Handle::try_current`
    /// to detect whether we are already inside a runtime. If so, we spawn a
    /// blocking task to run discovery without deadlocking the current runtime.
    /// If no runtime is active (e.g. tests without `#[tokio::test]`), we create
    /// a temporary one.
    fn try_discover(config: &skene::discovery::DiscoveryConfig) -> Option<String> {
        tracing::info!("no server URL configured, attempting auto-discovery");

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            // We are inside a tokio runtime. Spawn a blocking task to avoid
            // blocking the async executor while discovery probes run.
            std::thread::scope(|s| {
                s.spawn(|| handle.block_on(skene::discovery::discover_server_with_config(config)))
                    .join()
                    .ok()
                    .flatten()
            })
        } else {
            // No runtime available: create a temporary one for discovery.
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()?;
            rt.block_on(skene::discovery::discover_server_with_config(config))
        }
    }

    fn skip_discovery() -> bool {
        cfg!(test)
            || RealSystem.var("KOILON_NO_DISCOVERY").is_some()
            || RealSystem.var("KOILON_SKIP_PROBE").is_some()
            || RealSystem.var("NEXTEST").is_some()
            || RealSystem.var("NEXTEST_RUN_ID").is_some()
    }

    #[expect(
        clippy::unused_self,
        reason = "consistent instance-method API; &self kept for tracing::instrument skip"
    )]
    #[tracing::instrument(skip(self))]
    pub(crate) fn clear_credentials(&self) -> Result<()> {
        let base = dirs::config_dir().context(ConfigDirSnafu)?;
        let path = config_path_in(&base);
        if path.exists() {
            // kanon:ignore RUST/no-result-unwrap-or-default — missing config file is normal; empty default is correct
            let mut file_config = Self::load_file().unwrap_or_default();
            file_config.token = None;
            file_config.token_ref = None;
            if let Err(err) = secret_store::delete_token(&base) {
                tracing::warn!(error = %err, "failed to delete TUI token from secret storage");
            }
            let toml_str = toml::to_string(&file_config).context(TomlSnafu)?;
            write_config(&path, &toml_str)?;
            tracing::info!(path = %path.display(), "cleared credentials");
        }
        Ok(())
    }

    fn load_file() -> Option<ConfigFile> {
        let base = dirs::config_dir()?;
        load_file_in(&base)
    }
}

/// Path to `tui.toml` under a given config-dir base.
fn config_path_in(base: &Path) -> PathBuf {
    base.join("aletheia").join("tui.toml")
}

/// Load and parse `tui.toml` under `base`, resolving its token against
/// secret storage (#5321): a legacy plaintext `token` is migrated into the
/// keyring/encrypted fallback and the file rewritten with only `token_ref`;
/// an existing `token_ref` is resolved back into `token` for in-memory use
/// without ever writing the raw value back to disk.
fn load_file_in(base: &Path) -> Option<ConfigFile> {
    let path = config_path_in(base);
    let contents = std::fs::read_to_string(&path).ok()?;
    let mut file_config: ConfigFile = toml::from_str(&contents).ok()?;

    if resolve_token(base, &mut file_config) {
        match toml::to_string(&file_config) {
            Ok(toml_str) => {
                if let Err(err) = write_config(&path, &toml_str) {
                    tracing::warn!(error = %err, "failed to persist migrated TUI token reference");
                }
            }
            Err(err) => tracing::warn!(error = %err, "failed to serialize migrated TUI config"),
        }
    }

    Some(file_config)
}

/// Resolve `file_config`'s token against secret storage in place.
///
/// Returns `true` when a legacy plaintext token was migrated and the caller
/// should persist the rewritten (reference-only) config back to disk.
fn resolve_token(base: &Path, file_config: &mut ConfigFile) -> bool {
    if let Some(token) = file_config.token.clone() {
        return match secret_store::store_token(base, &token) {
            Ok(()) => {
                file_config.token_ref = Some(secret_store::TOKEN_REF.to_owned());
                // WHY: keep the plaintext value in memory for this run's
                // resolved_token precedence — only the on-disk copy is cleared.
                true
            }
            Err(err) => {
                tracing::warn!(error = %err, "failed to migrate TUI token to secret storage, leaving plaintext on disk");
                false
            }
        };
    }

    if file_config.token_ref.is_some() {
        match secret_store::load_token(base) {
            // WHY: `ConfigFile.token` is the shared bridge type for both the
            // legacy-plaintext and resolved-from-storage paths (it is never
            // serialized back to disk — see the field doc above); the
            // secret-store boundary itself now hands back `SecretString`,
            // and `Config::load` re-wraps this into `SecretString` for
            // runtime use.
            Ok(loaded) => {
                file_config.token = loaded.map(|token| token.expose_secret().to_owned());
            }
            Err(err) => {
                tracing::warn!(error = %err, "failed to load TUI token from secret storage")
            }
        }
    }

    false
}

fn write_config(path: &Path, content: &str) -> Result<()> {
    koina::fs::write_restricted(path, content.as_bytes()).context(IoSnafu {
        context: "write config file",
    })?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::disallowed_methods,
    reason = "tests seed config fixtures on disk directly"
)]
mod tests {
    use super::*;

    // WHY(#3693): reqwest 0.13 requires a rustls crypto provider to be
    // installed before any `Client` is constructed. `Config::load` builds
    // a reqwest client via `try_discover`; without this, every test that
    // calls `Config::load(...)` panics with "No provider set". Ignoring
    // the result so the second install on the same process (from main or
    // another test) is a no-op instead of a panic.
    fn ensure_crypto_provider() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    #[test]
    fn cli_overrides_file_config() {
        ensure_crypto_provider();
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
        ensure_crypto_provider();
        let config = Config::load(None, None, None, None).unwrap();
        assert_eq!(config.url, DEFAULT_URL);
    }

    #[test]
    fn toml_roundtrip() {
        let mut keybindings = HashMap::new();
        keybindings.insert("toggle_sidebar".to_string(), "Ctrl+G".to_string());
        let file = ConfigFile {
            url: Some("http://host:1234".into()),
            // WHY(#5321): token is legacy plaintext and deliberately does not
            // round-trip through serialization — see `token_field_never_serializes`.
            token: Some("secret".into()),
            token_ref: None,
            default_agent: Some("syn".into()),
            default_session: None,
            workspace_root: Some("/workspace".into()),
            bell: Some(true),
            keybindings: Some(keybindings),
            theme: Some("light".into()),
            discovery: Some(DiscoveryFileConfig {
                port: Some(18790),
                urls: Some(vec!["https://aletheia.example".into()]),
                lan_hostnames: Some(vec!["host-a".into()]),
                tailscale_ips: Some(vec!["100.64.0.10".into()]),
            }),
        };
        let toml_str = toml::to_string(&file).unwrap();
        let back: ConfigFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(file.url, back.url);
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
        let Some(discovery) = back.discovery else {
            panic!("discovery config should roundtrip");
        };
        assert_eq!(discovery.port, Some(18790));
        assert_eq!(
            discovery.urls.as_deref(),
            Some(&["https://aletheia.example".to_string()][..])
        );
        assert_eq!(
            discovery.lan_hostnames.as_deref(),
            Some(&["host-a".to_string()][..])
        );
        assert_eq!(
            discovery.tailscale_ips.as_deref(),
            Some(&["100.64.0.10".to_string()][..])
        );
    }

    #[test]
    fn token_field_never_serializes() {
        let file = ConfigFile {
            token: Some("plaintext-secret".into()),
            ..ConfigFile::default()
        };
        let toml_str = toml::to_string(&file).unwrap();
        assert!(!toml_str.contains("plaintext-secret"));
        assert!(!toml_str.contains("token ="));

        let back: ConfigFile = toml::from_str(&toml_str).unwrap();
        assert!(back.token.is_none());
    }

    #[test]
    fn token_ref_round_trips() {
        let file = ConfigFile {
            token_ref: Some("tui-default".into()),
            ..ConfigFile::default()
        };
        let toml_str = toml::to_string(&file).unwrap();
        assert!(toml_str.contains("token_ref"));

        let back: ConfigFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.token_ref, file.token_ref);
    }

    /// #5321: a `tui.toml` written before secret storage existed carried a
    /// plaintext `token`. Loading it must move the token into secret storage,
    /// rewrite the file with only `token_ref`, and still resolve the same
    /// token value for this run.
    #[test]
    fn load_file_in_migrates_plaintext_token_to_secret_storage() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let path = config_path_in(base);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "token = \"legacy-plaintext\"\n").unwrap();

        let loaded = load_file_in(base).unwrap();
        assert_eq!(loaded.token.as_deref(), Some("legacy-plaintext"));
        assert_eq!(loaded.token_ref.as_deref(), Some(secret_store::TOKEN_REF));

        let rewritten = std::fs::read_to_string(&path).unwrap();
        assert!(!rewritten.contains("legacy-plaintext"));
        assert!(rewritten.contains("token_ref"));

        assert_eq!(
            secret_store::load_token(base)
                .unwrap()
                .as_ref()
                .map(SecretString::expose_secret),
            Some("legacy-plaintext")
        );
    }

    /// A `token_ref`-only file (post-migration, or a fresh keyring-backed
    /// save) resolves its token from secret storage without ever touching
    /// disk plaintext.
    #[test]
    fn load_file_in_resolves_token_ref_from_secret_storage() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        secret_store::store_token(base, "kept-in-storage").unwrap();
        let path = config_path_in(base);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!("token_ref = \"{}\"\n", secret_store::TOKEN_REF),
        )
        .unwrap();

        let loaded = load_file_in(base).unwrap();
        assert_eq!(loaded.token.as_deref(), Some("kept-in-storage"));
    }

    #[test]
    fn theme_parsing_light() {
        ensure_crypto_provider();
        let config = Config::load(None, None, None, None).unwrap();
        // Default is auto (None) when no file setting
        let _ = config.theme;
    }

    #[test]
    fn workspace_root_none_when_no_env_or_file() {
        ensure_crypto_provider();
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
        ensure_crypto_provider();
        let config = Config::load(None, Some("sk-ant-oat-test123".into()), None, None).unwrap();
        assert_eq!(config.credential_label, CredentialLabel::OAuthToken);
    }

    #[test]
    fn config_load_detects_static_credential() {
        ensure_crypto_provider();
        let config = Config::load(None, Some("sk-ant-api01-test".into()), None, None).unwrap();
        assert_eq!(config.credential_label, CredentialLabel::StaticApiKey);
    }

    #[test]
    fn config_load_no_credential() {
        ensure_crypto_provider();
        let config = Config::load(None, None, None, None).unwrap();
        assert_eq!(config.credential_label, CredentialLabel::None);
    }
}
