//! Persistent connection config stored at `~/.config/aletheia/desktop.toml`.
//!
//! Loads on startup and saves after each successful connection so the user
//! does not have to re-enter the server URL on every launch.
//!
//! NOTE: Auth tokens are currently stored in plaintext. Future versions should
//! integrate with the OS keyring (libsecret, Keychain, Credential Manager).

use std::io::Write;
use std::path::PathBuf;

use snafu::{ResultExt, Snafu};

use crate::state::connection::ConnectionConfig;
use crate::state::notifications::NotificationPreferences;

/// Errors that can occur when loading or saving desktop config.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum ConfigError {
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
}

/// TOML file envelope for `~/.config/aletheia/desktop.toml`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct DesktopConfig {
    #[serde(default)]
    connection: ConnectionConfig,
    #[serde(default)]
    notifications: NotificationPreferences,
}

/// Resolve the config file path: `~/.config/aletheia/desktop.toml`.
fn config_path() -> Result<PathBuf, ConfigError> {
    let dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
    Ok(dir.join("aletheia").join("desktop.toml"))
}

/// Load connection config from disk.
///
/// Returns the default config if the file does not exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
#[must_use]
pub(crate) fn load() -> Result<ConnectionConfig, ConfigError> {
    let path = config_path()?;

    if !path.exists() {
        return Ok(ConnectionConfig::default());
    }

    let content = std::fs::read_to_string(&path).context(ReadFileSnafu { path: &path })?;
    let desktop: DesktopConfig = toml::from_str(&content).context(ParseSnafu)?;
    Ok(desktop.connection)
}

/// Save connection config to disk.
///
/// Creates the parent directory if it does not exist.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot
/// be written.
#[must_use]
pub(crate) fn save(config: &ConnectionConfig) -> Result<(), ConfigError> {
    let path = config_path()?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context(CreateDirSnafu {
            path: parent.to_path_buf(),
        })?;
    }

    // NOTE: Preserve existing notification preferences when saving connection config.
    let existing_notifications = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|c| toml::from_str::<DesktopConfig>(&c).ok())
            .map(|d| d.notifications)
            .unwrap_or_default()
    } else {
        NotificationPreferences::default()
    };
    let desktop = DesktopConfig {
        connection: config.clone(),
        notifications: existing_notifications,
    };
    let content = toml::to_string_pretty(&desktop).context(SerializeSnafu)?;
    // SAFETY: Config may contain auth tokens; restrict to owner-only access.
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .context(WriteFileSnafu { path: &path })?;
        file.write_all(content.as_bytes())
            .context(WriteFileSnafu { path: &path })?;
    }

    Ok(())
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
            tracing::warn!("failed to load config, using defaults: {e}");
            ConnectionConfig::default()
        }
    }
}

/// Load notification preferences from the config file.
///
/// Returns defaults if the file does not exist or the section is absent.
#[must_use]
pub(crate) fn load_notification_prefs() -> NotificationPreferences {
    let Ok(path) = config_path() else {
        return NotificationPreferences::default();
    };
    if !path.exists() {
        return NotificationPreferences::default();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return NotificationPreferences::default(),
    };
    toml::from_str::<DesktopConfig>(&content)
        .map(|c| c.notifications)
        .unwrap_or_default()
}

/// Save notification preferences to the config file.
///
/// Reads the current file, updates only the `[notifications]` section, and
/// writes back — preserving the `[connection]` section.
///
/// # Errors
///
/// Returns an error if the file cannot be read, parsed, or written.
pub(crate) fn save_notification_prefs(prefs: &NotificationPreferences) -> Result<(), ConfigError> {
    let path = config_path()?;

    // Read existing config to preserve the connection section.
    let existing = if path.exists() {
        let content = std::fs::read_to_string(&path).context(ReadFileSnafu { path: &path })?;
        toml::from_str::<DesktopConfig>(&content).context(ParseSnafu)?
    } else {
        DesktopConfig::default()
    };

    let updated = DesktopConfig {
        connection: existing.connection,
        notifications: prefs.clone(),
    };
    let content = toml::to_string_pretty(&updated).context(SerializeSnafu)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context(CreateDirSnafu {
            path: parent.to_path_buf(),
        })?;
    }

    // SAFETY: Config may contain auth tokens; restrict to owner-only access.
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .context(WriteFileSnafu { path: &path })?;
        file.write_all(content.as_bytes())
            .context(WriteFileSnafu { path: &path })?;
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn round_trip_toml() {
        let config = ConnectionConfig {
            server_url: "https://pylon.example.com:8443".to_string(),
            auth_token: Some("tok_abc123".to_string()),
            auto_reconnect: false,
        };

        let desktop = DesktopConfig {
            connection: config.clone(),
            notifications: NotificationPreferences::default(),
        };
        let serialized = toml::to_string_pretty(&desktop).unwrap();
        let deserialized: DesktopConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.connection.server_url, config.server_url);
        assert_eq!(deserialized.connection.auth_token, config.auth_token);
        assert_eq!(
            deserialized.connection.auto_reconnect,
            config.auto_reconnect
        );
    }

    #[test]
    fn save_and_load_tempdir() {
        // Use a tempdir to avoid touching the real config.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.toml");

        let config = ConnectionConfig {
            server_url: "http://10.0.0.1:3000".to_string(),
            auth_token: Some("my-token".to_string()),
            auto_reconnect: true,
        };

        let desktop = DesktopConfig {
            connection: config.clone(),
            notifications: NotificationPreferences::default(),
        };
        let content = toml::to_string_pretty(&desktop).unwrap();
        std::fs::write(&path, &content).unwrap();

        let loaded_content = std::fs::read_to_string(&path).unwrap();
        let loaded: DesktopConfig = toml::from_str(&loaded_content).unwrap();

        assert_eq!(loaded.connection.server_url, config.server_url);
        assert_eq!(loaded.connection.auth_token, config.auth_token);
    }

    #[test]
    fn desktop_config_defaults() {
        let toml_str = "";
        let desktop: DesktopConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(desktop.connection.server_url, "http://localhost:3000");
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
}
