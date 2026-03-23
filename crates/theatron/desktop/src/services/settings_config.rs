//! Settings config persistence: server list, appearance, keybindings.
//!
//! Reads and writes `~/.config/aletheia-desktop/settings.toml` using TOML
//! serialization. The file is restricted to owner-only access (0o600).

use std::collections::HashMap;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use crate::state::settings::{
    AppearanceSettings, KeyCombo, KeybindingStore, ServerConfigStore, ServerEntry, UiDensity,
};

/// Errors from settings config I/O.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub(crate) enum SettingsConfigError {
    /// Could not locate the user config directory.
    #[snafu(display("could not find user config directory"))]
    NoConfigDir,

    /// Failed to create the config directory.
    #[snafu(display("failed to create config directory: {source}"))]
    CreateDir { source: std::io::Error },

    /// Failed to read the settings file.
    #[snafu(display("failed to read settings file: {source}"))]
    ReadFile { source: std::io::Error },

    /// Failed to write the settings file.
    #[snafu(display("failed to write settings file: {source}"))]
    WriteFile { source: std::io::Error },

    /// TOML deserialization failed.
    #[snafu(display("failed to parse settings: {source}"))]
    TomlParse { source: toml::de::Error },

    /// TOML serialization failed.
    #[snafu(display("failed to serialize settings: {source}"))]
    TomlSerialize { source: toml::ser::Error },
}

// --- Serializable intermediate types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializedAppearance {
    #[serde(default = "default_theme")]
    theme: String,
    #[serde(default = "default_font_size")]
    font_size: u8,
    #[serde(default = "default_density")]
    density: String,
    #[serde(default = "default_accent")]
    accent_color: String,
}

fn default_theme() -> String {
    "system".to_string()
}
fn default_font_size() -> u8 {
    14
}
fn default_density() -> String {
    "comfortable".to_string()
}
fn default_accent() -> String {
    "#5b6af0".to_string()
}

impl Default for SerializedAppearance {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            font_size: default_font_size(),
            density: default_density(),
            accent_color: default_accent(),
        }
    }
}

impl From<&AppearanceSettings> for SerializedAppearance {
    fn from(s: &AppearanceSettings) -> Self {
        Self {
            theme: s.theme.clone(),
            font_size: s.font_size,
            density: match s.density {
                UiDensity::Compact => "compact",
                UiDensity::Comfortable => "comfortable",
                UiDensity::Spacious => "spacious",
            }
            .to_string(),
            accent_color: s.accent_color.clone(),
        }
    }
}

impl From<SerializedAppearance> for AppearanceSettings {
    fn from(s: SerializedAppearance) -> Self {
        Self {
            theme: s.theme,
            font_size: s.font_size.clamp(12, 20),
            density: match s.density.as_str() {
                "compact" => UiDensity::Compact,
                "spacious" => UiDensity::Spacious,
                _ => UiDensity::Comfortable,
            },
            accent_color: s.accent_color,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializedServer {
    id: String,
    name: String,
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auth_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_connected: Option<String>,
}

impl From<&ServerEntry> for SerializedServer {
    fn from(e: &ServerEntry) -> Self {
        Self {
            id: e.id.clone(),
            name: e.name.clone(),
            url: e.url.clone(),
            auth_token: e.auth_token.clone(),
            last_connected: e.last_connected.clone(),
        }
    }
}

impl From<SerializedServer> for ServerEntry {
    fn from(s: SerializedServer) -> Self {
        Self {
            id: s.id,
            name: s.name,
            url: s.url,
            auth_token: s.auth_token,
            last_connected: s.last_connected,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializedCombo {
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    shift: bool,
    key: String,
}

impl From<&KeyCombo> for SerializedCombo {
    fn from(c: &KeyCombo) -> Self {
        Self {
            ctrl: c.ctrl,
            alt: c.alt,
            shift: c.shift,
            key: c.key.clone(),
        }
    }
}

impl From<SerializedCombo> for KeyCombo {
    fn from(c: SerializedCombo) -> Self {
        Self {
            ctrl: c.ctrl,
            alt: c.alt,
            shift: c.shift,
            key: c.key,
        }
    }
}

// --- Public config type ---

/// Root on-disk settings structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct SettingsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_server: Option<String>,
    #[serde(default)]
    servers: Vec<SerializedServer>,
    #[serde(default)]
    appearance: SerializedAppearance,
    #[serde(default)]
    keybinding_overrides: HashMap<String, SerializedCombo>,
}

impl SettingsConfig {
    /// Build a `ServerConfigStore` from the persisted representation.
    pub(crate) fn server_store(&self) -> ServerConfigStore {
        ServerConfigStore {
            active_id: self.active_server.clone(),
            servers: self
                .servers
                .iter()
                .cloned()
                .map(ServerEntry::from)
                .collect(),
        }
    }

    /// Write a `ServerConfigStore` back into serializable form.
    pub(crate) fn apply_server_store(&mut self, store: &ServerConfigStore) {
        self.active_server = store.active_id.clone();
        self.servers = store.servers.iter().map(SerializedServer::from).collect();
    }

    /// Build `AppearanceSettings` from the persisted representation.
    pub(crate) fn appearance_settings(&self) -> AppearanceSettings {
        self.appearance.clone().into()
    }

    /// Build `KeybindingStore` from the persisted overrides.
    pub(crate) fn keybinding_store(&self) -> KeybindingStore {
        KeybindingStore {
            overrides: self
                .keybinding_overrides
                .iter()
                .map(|(k, v)| (k.clone(), KeyCombo::from(v.clone())))
                .collect(),
        }
    }

    /// Build a `SettingsConfig` from in-memory state (for saving).
    pub(crate) fn from_state(
        server_store: &ServerConfigStore,
        appearance: &AppearanceSettings,
        keybindings: &KeybindingStore,
    ) -> Self {
        let mut config = Self::default();
        config.apply_server_store(server_store);
        config.appearance = SerializedAppearance::from(appearance);
        config.keybinding_overrides = keybindings
            .overrides
            .iter()
            .map(|(k, v)| (k.clone(), SerializedCombo::from(v)))
            .collect();
        config
    }
}

// --- I/O functions ---

fn settings_path() -> Result<std::path::PathBuf, SettingsConfigError> {
    let dir = dirs::config_dir().ok_or(SettingsConfigError::NoConfigDir)?;
    Ok(dir.join("aletheia-desktop").join("settings.toml"))
}

/// Whether this is the first run (no settings file exists yet).
pub(crate) fn is_first_run() -> bool {
    settings_path().map_or(true, |p| !p.exists())
}

/// Load settings from disk.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub(crate) fn load() -> Result<SettingsConfig, SettingsConfigError> {
    let path = settings_path()?;
    let contents = std::fs::read_to_string(&path).context(ReadFileSnafu)?;
    toml::from_str(&contents).context(TomlParseSnafu)
}

/// Save settings to disk. Creates parent directory if needed.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot
/// be written.
pub(crate) fn save(config: &SettingsConfig) -> Result<(), SettingsConfigError> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context(CreateDirSnafu)?;
    }
    let contents = toml::to_string_pretty(config).context(TomlSerializeSnafu)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .context(WriteFileSnafu)?;
    file.write_all(contents.as_bytes()).context(WriteFileSnafu)
}

/// Load settings from disk, falling back to defaults on any error.
pub(crate) fn load_or_default() -> SettingsConfig {
    match load() {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("failed to load settings config, using defaults: {e}");
            SettingsConfig::default()
        }
    }
}

/// Save current in-memory state to disk.
///
/// Logs a warning on failure; non-fatal since settings are best-effort.
pub(crate) fn save_state(
    server_store: &ServerConfigStore,
    appearance: &AppearanceSettings,
    keybindings: &KeybindingStore,
) {
    let config = SettingsConfig::from_state(server_store, appearance, keybindings);
    if let Err(e) = save(&config) {
        tracing::warn!("failed to save settings: {e}");
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::state::settings::{AppearanceSettings, KeybindingStore, ServerConfigStore};

    #[test]
    fn appearance_round_trips_via_serialized_form() {
        let original = AppearanceSettings {
            theme: "dark".to_string(),
            font_size: 16,
            density: UiDensity::Spacious,
            accent_color: "#ff0000".to_string(),
        };
        let serialized = SerializedAppearance::from(&original);
        let restored = AppearanceSettings::from(serialized);
        assert_eq!(restored.theme, original.theme);
        assert_eq!(restored.font_size, original.font_size);
        assert_eq!(restored.density, original.density);
        assert_eq!(restored.accent_color, original.accent_color);
    }

    #[test]
    fn server_store_round_trips_via_config() {
        let mut store = ServerConfigStore::default();
        let id = store.add(
            "Test".to_string(),
            "http://test:3000".to_string(),
            Some("tok".to_string()),
        );
        store.set_active(&id);

        let mut config = SettingsConfig::default();
        config.apply_server_store(&store);
        let restored = config.server_store();

        assert_eq!(restored.active_id, store.active_id);
        assert_eq!(restored.servers.len(), 1);
        assert_eq!(restored.servers[0].name, "Test");
        assert_eq!(restored.servers[0].auth_token.as_deref(), Some("tok"));
    }

    #[test]
    fn from_state_round_trips() {
        let server_store = ServerConfigStore::default();
        let appearance = AppearanceSettings::default();
        let keybindings = KeybindingStore::default();
        let config = SettingsConfig::from_state(&server_store, &appearance, &keybindings);

        assert_eq!(config.appearance.theme, "system");
        assert_eq!(config.appearance.font_size, 14);
        assert!(config.keybinding_overrides.is_empty());
    }

    #[test]
    fn is_first_run_returns_bool() {
        // Just verifies it doesn't panic; actual value depends on host state.
        let _ = is_first_run();
    }

    #[test]
    fn default_settings_config_is_valid() {
        let config = SettingsConfig::default();
        assert!(config.servers.is_empty());
        assert!(config.active_server.is_none());
        assert_eq!(config.appearance.theme, "system");
    }

    #[test]
    fn toml_round_trip() {
        let mut store = ServerConfigStore::default();
        store.add(
            "Local".to_string(),
            "http://localhost:3000".to_string(),
            None,
        );
        let appearance = AppearanceSettings {
            theme: "dark".to_string(),
            font_size: 14,
            density: UiDensity::Comfortable,
            accent_color: "#5b6af0".to_string(),
        };
        let keybindings = KeybindingStore::default();
        let config = SettingsConfig::from_state(&store, &appearance, &keybindings);
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: SettingsConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.servers.len(), 1);
        assert_eq!(restored.servers[0].name, "Local");
        assert_eq!(restored.appearance.theme, "dark");
    }
}
