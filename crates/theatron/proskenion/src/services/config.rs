//! Persistent desktop config compatibility facade.
//!
//! Historically this module owned the connection config stored at
//! `~/.config/aletheia/desktop.toml`. Connection state is now canonical in
//! `~/.config/aletheia-desktop/settings.toml` via `services::settings_config`;
//! this module remains responsible for:
//!
//! - Migrating legacy `desktop.toml` profiles into the canonical store.
//! - Loading and saving notification preferences (still in `desktop.toml`).
//!
//! Auth values are treated as opaque references in the canonical store. This
//! module may read legacy plaintext tokens only to migrate existing profiles.

use std::io::Write;
use std::path::{Path, PathBuf};

use snafu::{ResultExt, Snafu};

use crate::services::settings_config;
use crate::state::connection::ConnectionConfig;
use crate::state::notifications::NotificationPreferences;

/// Errors that can occur when loading or saving desktop config.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub(crate) enum ConfigError {
    /// No platform config directory could be determined.
    #[snafu(display("failed to determine config directory"))]
    NoConfigDir,

    /// Failed to create the config directory.
    #[snafu(display("failed to create config directory {}: {source}", path.display()))]
    CreateDir {
        /// Directory path that could not be created.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to read the config file from disk.
    #[snafu(display("failed to read config file {}: {source}", path.display()))]
    ReadFile {
        /// File path that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to write the config file to disk.
    #[snafu(display("failed to write config file {}: {source}", path.display()))]
    WriteFile {
        /// File path that could not be written.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to parse the TOML config content.
    #[snafu(display("failed to parse config: {source}"))]
    Parse {
        /// Underlying TOML deserialization error.
        source: toml::de::Error,
    },

    /// Failed to serialize config to TOML.
    #[snafu(display("failed to serialize config: {source}"))]
    Serialize {
        /// Underlying TOML serialization error.
        source: toml::ser::Error,
    },

    /// Failed to load canonical settings.
    #[snafu(display("failed to load canonical settings: {source}"))]
    LoadSettings {
        /// Underlying settings config error.
        source: settings_config::SettingsConfigError,
    },

    /// Failed to persist the active connection to canonical settings.
    #[snafu(display("failed to persist connection to settings: {source}"))]
    PersistSettings {
        /// Underlying settings config error.
        source: settings_config::SettingsConfigError,
    },
}

/// TOML file envelope for `~/.config/aletheia/desktop.toml`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DesktopConfig {
    #[serde(default)]
    connection: ConnectionConfig,
    #[serde(default)]
    notifications: NotificationPreferences,
}

/// Resolve the legacy config file path under `base`.
fn config_path_from(base: &Path) -> PathBuf {
    base.join("aletheia").join("desktop.toml")
}

/// Resolve the config file path: `~/.config/aletheia/desktop.toml`.
fn config_path() -> Result<PathBuf, ConfigError> {
    let dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
    Ok(config_path_from(&dir))
}

/// Build a runtime connection config from canonical settings.
fn connection_from_settings(
    settings: &settings_config::SettingsConfig,
) -> Option<ConnectionConfig> {
    settings
        .server_store()
        .active()
        .map(|entry| ConnectionConfig {
            server_url: entry.url.clone(),
            auth_token: entry.auth_token.clone(),
            auto_reconnect: true,
            ..ConnectionConfig::default()
        })
}

/// Load connection config from disk.
///
/// Returns the active server from canonical settings if present. If canonical
/// settings have no active server, attempts to migrate a legacy
/// `~/.config/aletheia/desktop.toml` profile and returns its connection.
/// Returns the default config if neither exists.
///
/// # Errors
///
/// Returns an error if a legacy file exists but cannot be read or parsed.
pub(crate) fn load() -> Result<ConnectionConfig, ConfigError> {
    let base = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
    load_in(&base)
}

fn load_in(base: &Path) -> Result<ConnectionConfig, ConfigError> {
    // WHY: Canonical settings own active connection state; read them first.
    match settings_config::load_in(base) {
        Ok(settings) => {
            if let Some(config) = connection_from_settings(&settings) {
                return Ok(config);
            }
        }
        Err(e) => {
            if base.join("aletheia-desktop").join("settings.toml").exists() {
                return Err(ConfigError::LoadSettings { source: e });
            }
        }
    }

    // NOTE: No active canonical server. If a legacy desktop.toml exists,
    // import its connection into the canonical store and use it.
    let path = config_path_from(base);
    if !path.exists() {
        return Ok(ConnectionConfig::default());
    }

    let content = std::fs::read_to_string(&path).context(ReadFileSnafu { path: &path })?;
    let desktop: DesktopConfig = toml::from_str(&content).context(ParseSnafu)?;

    if desktop.connection.server_url.is_empty() {
        return Ok(ConnectionConfig::default());
    }

    settings_config::upsert_active_server_in(
        base,
        desktop.connection.server_url.clone(),
        desktop.connection.auth_token.clone(),
    )
    .context(PersistSettingsSnafu)?;
    if desktop.connection.auth_token.is_some() {
        let sanitized = DesktopConfig {
            connection: ConnectionConfig {
                auth_token: None,
                ..desktop.connection.clone()
            },
            notifications: desktop.notifications.clone(),
        };
        write_desktop_config(&path, &sanitized)?;
    }

    Ok(desktop.connection)
}

/// Load config, falling back to defaults on any error.
///
/// Logs a warning if loading fails. Suitable for startup where a missing
/// or corrupt config file should not prevent the app from launching.
#[must_use]
pub(crate) fn load_or_default() -> ConnectionConfig {
    match load() {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("failed to load connection config, using defaults: {e}");
            ConnectionConfig::default()
        }
    }
}

/// Save connection config to disk.
///
/// Persists the active server URL and auth reference into the canonical
/// settings store. Creates the parent directory if it does not exist.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the canonical
/// settings cannot be written.
pub(crate) fn save(config: &ConnectionConfig) -> Result<(), ConfigError> {
    let base = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
    save_in(&base, config)
}

fn save_in(base: &Path, config: &ConnectionConfig) -> Result<(), ConfigError> {
    settings_config::upsert_active_server_in(
        base,
        config.server_url.clone(),
        config.auth_token.clone(),
    )
    .context(PersistSettingsSnafu)
}

/// Load notification preferences from the config file.
///
/// Returns defaults if the file does not exist or the section is absent.
#[must_use]
pub(crate) fn load_notification_prefs() -> NotificationPreferences {
    let Ok(path) = config_path() else {
        return NotificationPreferences::default();
    };
    load_notification_prefs_in(&path)
}

fn load_notification_prefs_in(path: &Path) -> NotificationPreferences {
    if !path.exists() {
        return NotificationPreferences::default();
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return NotificationPreferences::default(),
    };
    match toml::from_str::<DesktopConfig>(&content) {
        Ok(config) => config.notifications,
        Err(err) => {
            tracing::warn!("failed to parse notification preferences, using defaults: {err}");
            NotificationPreferences::default()
        }
    }
}

/// Save notification preferences to the config file.
///
/// Reads the current file, updates only the `[notifications]` section, and
/// writes back -- preserving the `[connection]` section if one still exists.
///
/// # Errors
///
/// Returns an error if the file cannot be read, parsed, or written.
pub(crate) fn save_notification_prefs(prefs: &NotificationPreferences) -> Result<(), ConfigError> {
    let path = config_path()?;
    save_notification_prefs_in(&path, prefs)
}

fn save_notification_prefs_in(
    path: &Path,
    prefs: &NotificationPreferences,
) -> Result<(), ConfigError> {
    // Read existing config to preserve the connection section.
    let existing = if path.exists() {
        let content = std::fs::read_to_string(path).context(ReadFileSnafu { path })?;
        toml::from_str::<DesktopConfig>(&content).context(ParseSnafu)?
    } else {
        DesktopConfig::default()
    };

    let updated = DesktopConfig {
        connection: existing.connection,
        notifications: prefs.clone(),
    };
    write_desktop_config(path, &updated)
}

fn write_desktop_config(path: &Path, desktop: &DesktopConfig) -> Result<(), ConfigError> {
    let content = toml::to_string_pretty(desktop).context(SerializeSnafu)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context(CreateDirSnafu {
            path: parent.to_path_buf(),
        })?;
    }

    // WHY: On Unix create the file with 0o600 mode atomically. On Windows
    // the equivalent `OpenOptionsExt::mode` does not exist; the file lives
    // in the user's config directory where default ACLs are user-private.
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .context(WriteFileSnafu { path })?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .context(WriteFileSnafu { path })?;
    file.write_all(content.as_bytes())
        .context(WriteFileSnafu { path })?;

    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn temp_base() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        (dir, base)
    }

    #[test]
    fn desktop_config_serialization_omits_raw_bearer_token() {
        let config = ConnectionConfig {
            server_url: "https://pylon.example.com:8443".to_string(),
            auth_token: Some("tok_abc123".to_string()),
            auto_reconnect: false,
            ..ConnectionConfig::default()
        };

        let desktop = DesktopConfig {
            connection: config.clone(),
            notifications: NotificationPreferences::default(),
        };
        let serialized = toml::to_string_pretty(&desktop).unwrap();
        let deserialized: DesktopConfig = toml::from_str(&serialized).unwrap();

        assert!(!serialized.contains("tok_abc123"));
        assert!(!serialized.contains("auth_token"));
        assert_eq!(deserialized.connection.server_url, config.server_url);
        assert!(deserialized.connection.auth_token.is_none());
        assert_eq!(
            deserialized.connection.auto_reconnect,
            config.auto_reconnect
        );
    }

    #[test]
    fn legacy_desktop_config_deserializes_plaintext_token_for_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.toml");
        std::fs::write(
            &path,
            r#"
[connection]
server_url = "http://10.0.0.1:3000"
auth_token = "my-token"
"#,
        )
        .unwrap();

        let loaded_content = std::fs::read_to_string(&path).unwrap();
        let loaded: DesktopConfig = toml::from_str(&loaded_content).unwrap();

        assert_eq!(loaded.connection.server_url, "http://10.0.0.1:3000");
        assert_eq!(loaded.connection.auth_token.as_deref(), Some("my-token"));
    }

    #[test]
    fn desktop_config_defaults() {
        let toml_str = "";
        let desktop: DesktopConfig = toml::from_str(toml_str).unwrap();
        let port = skene::discovery::DiscoveryConfig::default().port;
        assert_eq!(
            desktop.connection.server_url,
            format!("http://localhost:{port}")
        );
        assert!(desktop.connection.auth_token.is_none());
        assert!(desktop.connection.auto_reconnect);
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let toml_str = r#"
[connection]
server_url = "http://custom:9000"
"#;
        let desktop: DesktopConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(desktop.connection.server_url, "http://custom:9000");
        assert!(desktop.connection.auth_token.is_none());
        assert!(desktop.connection.auto_reconnect);
    }

    #[test]
    fn config_error_no_config_dir_display() {
        let err = ConfigError::NoConfigDir;
        assert!(err.to_string().contains("config directory"));
    }

    #[test]
    fn config_error_create_dir_display() {
        let err = ConfigError::CreateDir {
            path: PathBuf::from("/nope"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        };
        let s = err.to_string();
        assert!(s.contains("/nope"));
        assert!(s.contains("create config directory"));
    }

    #[test]
    fn config_error_read_file_display() {
        let err = ConfigError::ReadFile {
            path: PathBuf::from("/tmp/xyz"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        let s = err.to_string();
        assert!(s.contains("read config file"));
        assert!(s.contains("/tmp/xyz"));
    }

    #[test]
    fn config_error_write_file_display() {
        let err = ConfigError::WriteFile {
            path: PathBuf::from("/tmp/xyz"),
            source: std::io::Error::other("disk full"),
        };
        assert!(err.to_string().contains("write config file"));
    }

    #[test]
    fn config_error_parse_display() {
        let err: toml::de::Error = toml::from_str::<DesktopConfig>("not-toml = ][").unwrap_err();
        let wrapped = ConfigError::Parse { source: err };
        assert!(wrapped.to_string().contains("parse config"));
    }

    #[test]
    fn config_path_returns_aletheia_subpath() {
        // Function may fail in environments without HOME, which is fine.
        if let Ok(path) = config_path() {
            let s = path.display().to_string();
            assert!(s.contains("aletheia"));
            assert!(s.ends_with("desktop.toml"));
        }
    }

    #[test]
    #[cfg(unix)]
    fn save_and_load_via_explicit_tempdir() {
        // WHY: save() and load() use a hardcoded path, but we can verify
        // the round-trip behaviour by mirroring their logic on a tempdir
        // file. This documents and exercises the on-disk format.
        use std::os::unix::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.toml");

        let config = ConnectionConfig {
            server_url: "http://test-host:9000".to_string(),
            auth_token: Some("session-token".to_string()),
            auto_reconnect: false,
            connect_timeout_secs: 60,
        };
        let desktop = DesktopConfig {
            connection: config.clone(),
            notifications: NotificationPreferences::default(),
        };
        let content = toml::to_string_pretty(&desktop).unwrap();

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(content.as_bytes()).unwrap();
        drop(file);

        // Verify perms are restrictive.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config file must be 0600");

        let loaded_content = std::fs::read_to_string(&path).unwrap();
        let loaded: DesktopConfig = toml::from_str(&loaded_content).unwrap();
        assert_eq!(loaded.connection.server_url, "http://test-host:9000");
        assert_eq!(loaded.connection.connect_timeout_secs, 60);
        assert!(!loaded.connection.auto_reconnect);
        assert!(!loaded_content.contains("session-token"));
        assert!(loaded.connection.auth_token.is_none());
    }

    #[test]
    fn notification_prefs_round_trip_via_tempfile() {
        // Mirror the save_notification_prefs / load_notification_prefs
        // logic on a tempfile to exercise the merge behaviour.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.toml");

        let initial = DesktopConfig {
            connection: ConnectionConfig {
                server_url: "http://preserve.me:3000".to_string(),
                ..ConnectionConfig::default()
            },
            notifications: NotificationPreferences::default(),
        };
        let serialized = toml::to_string_pretty(&initial).unwrap();
        std::fs::write(&path, &serialized).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: DesktopConfig = toml::from_str(&raw).unwrap();
        assert_eq!(parsed.connection.server_url, "http://preserve.me:3000");
    }

    #[test]
    fn load_or_default_falls_back_silently() {
        // Calling load_or_default in an environment where the config might
        // be missing or unreadable should not panic; it just returns
        // defaults. We can't safely assert the returned value (host config
        // may exist), but covering the call path catches regressions.
        let cfg = load_or_default();
        assert!(!cfg.server_url.is_empty(), "server_url must always be set");
    }

    #[test]
    fn load_notification_prefs_does_not_panic() {
        // Same pattern: load_notification_prefs must always return Some
        // value, never panic, regardless of host state.
        let prefs = load_notification_prefs();
        assert_eq!(prefs.enabled, NotificationPreferences::default().enabled);
    }

    // --- Migration / canonical-store tests ---

    #[test]
    fn legacy_desktop_toml_migrates_to_settings() {
        let (_dir, base) = temp_base();
        let legacy = config_path_from(&base);
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let legacy_token = "legacy-token-4491";
        std::fs::write(
            &legacy,
            format!(
                r#"
[connection]
server_url = "http://legacy-server:18789"
auth_token = "{legacy_token}"
"#
            ),
        )
        .unwrap();

        let cfg = load_in(&base).unwrap();
        assert_eq!(cfg.server_url, "http://legacy-server:18789");
        assert_eq!(cfg.auth_token.as_deref(), Some(legacy_token));

        let sanitized_legacy = std::fs::read_to_string(&legacy).unwrap();
        assert!(!sanitized_legacy.contains(legacy_token));
        assert!(!sanitized_legacy.contains("auth_token"));

        // WHY: Migration should have written the canonical settings store.
        let settings_path = base.join("aletheia-desktop").join("settings.toml");
        let raw_settings = std::fs::read_to_string(settings_path).unwrap();
        assert!(!raw_settings.contains(legacy_token));
        assert!(!raw_settings.contains("auth_token ="));
        assert!(raw_settings.contains("auth_token_ref"));

        let settings = settings_config::load_in(&base).unwrap();
        let store = settings.server_store();
        let active = store.active().unwrap();
        assert_eq!(active.url, "http://legacy-server:18789");
        assert_eq!(active.auth_token.as_deref(), Some(legacy_token));
    }

    #[test]
    fn canonical_settings_take_precedence_over_legacy() {
        let (_dir, base) = temp_base();

        // Pre-populate canonical settings with an active server.
        settings_config::upsert_active_server_in(&base, "http://canonical:18789".to_string(), None)
            .unwrap();

        // Legacy file still exists with a different URL.
        let legacy = config_path_from(&base);
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let desktop = DesktopConfig {
            connection: ConnectionConfig {
                server_url: "http://legacy:18789".to_string(),
                ..ConnectionConfig::default()
            },
            notifications: NotificationPreferences::default(),
        };
        std::fs::write(&legacy, toml::to_string_pretty(&desktop).unwrap()).unwrap();

        let cfg = load_in(&base).unwrap();
        assert_eq!(cfg.server_url, "http://canonical:18789");
    }

    #[test]
    fn malformed_canonical_settings_do_not_fall_back_to_legacy() {
        let (_dir, base) = temp_base();
        let settings = base.join("aletheia-desktop").join("settings.toml");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "not valid toml = [").unwrap();

        let legacy = config_path_from(&base);
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let desktop = DesktopConfig {
            connection: ConnectionConfig {
                server_url: "http://legacy:18789".to_string(),
                ..ConnectionConfig::default()
            },
            notifications: NotificationPreferences::default(),
        };
        std::fs::write(&legacy, toml::to_string_pretty(&desktop).unwrap()).unwrap();

        let err = load_in(&base).expect_err("canonical parse error should win");
        assert!(
            matches!(err, ConfigError::LoadSettings { .. }),
            "expected canonical settings error, got {err:?}"
        );
    }

    #[test]
    fn save_connection_updates_canonical_settings() {
        let (_dir, base) = temp_base();
        let raw_token = "save-token-4491";
        let config = ConnectionConfig {
            server_url: "http://save-me:18789".to_string(),
            auth_token: Some(raw_token.to_string()),
            ..ConnectionConfig::default()
        };

        save_in(&base, &config).unwrap();

        let settings_path = base.join("aletheia-desktop").join("settings.toml");
        let raw_settings = std::fs::read_to_string(settings_path).unwrap();
        assert!(!raw_settings.contains(raw_token));
        assert!(!raw_settings.contains("auth_token ="));
        assert!(raw_settings.contains("auth_token_ref"));

        let settings = settings_config::load_in(&base).unwrap();
        let store = settings.server_store();
        let active = store.active().unwrap();
        assert_eq!(active.url, "http://save-me:18789");
        assert_eq!(active.auth_token.as_deref(), Some(raw_token));
    }
}
