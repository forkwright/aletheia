//! Settings config persistence: active server, server list, appearance, keybindings.
//!
//! Reads and writes `~/.config/aletheia-desktop/settings.toml` using TOML
//! serialization. This is the canonical owner for desktop connection settings;
//! `services::config` is a compatibility/migration facade for legacy
//! `~/.config/aletheia/desktop.toml` profiles and notification preferences.
//! The file is restricted to owner-only access (0o600).

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use crate::services::secret_store::{self, SecretStoreError};
use crate::state::settings::{
    AppearanceSettings, KeyCombo, KeybindingStore, ServerConfigStore, ServerEntry, UiDensity,
    server_token_ref,
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

    /// Secret storage failed.
    #[snafu(display("failed to store desktop auth token: {source}"))]
    SecretStore {
        /// Underlying secret-store error.
        source: SecretStoreError,
    },
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
    /// Legacy plaintext token input. Never serialized.
    #[serde(default, skip_serializing)]
    auth_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auth_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    csrf_header_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    csrf_header_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_connected: Option<String>,
}

impl From<&ServerEntry> for SerializedServer {
    fn from(e: &ServerEntry) -> Self {
        let auth_token_ref = e
            .auth_token_ref
            .clone()
            .or_else(|| e.auth_token.as_ref().map(|_| server_token_ref(&e.id)));
        Self {
            id: e.id.clone(),
            name: e.name.clone(),
            url: e.url.clone(),
            auth_token: e.auth_token.clone(),
            auth_token_ref,
            csrf_header_name: e.csrf_header_name.clone(),
            csrf_header_value: e.csrf_header_value.clone(),
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
            auth_token_ref: s.auth_token_ref,
            auth_token: s.auth_token,
            csrf_header_name: s.csrf_header_name,
            csrf_header_value: s.csrf_header_value,
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
    key: String, // kanon:ignore RUST/plain-string-secret -- keyboard key name, not credential material (#3988)
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
#[serde(deny_unknown_fields)]
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

// --- Path helpers ---

fn settings_dir_from(base: &Path) -> PathBuf {
    base.join("aletheia-desktop")
}

fn settings_path_from(base: &Path) -> PathBuf {
    settings_dir_from(base).join("settings.toml")
}

fn legacy_config_path_from(base: &Path) -> PathBuf {
    base.join("aletheia").join("desktop.toml")
}

// --- First-run detection ---

/// Whether this is the first run (no settings file or legacy config exists yet).
///
/// A clean profile has neither `~/.config/aletheia-desktop/settings.toml` nor
/// `~/.config/aletheia/desktop.toml`. Profiles with a legacy desktop.toml are
/// treated as returning users and are migrated on first load.
pub(crate) fn is_first_run() -> bool {
    let Some(base) = dirs::config_dir() else {
        return true;
    };
    is_first_run_in(&base)
}

pub(crate) fn is_first_run_in(base: &Path) -> bool {
    !settings_path_from(base).exists() && !legacy_config_path_from(base).exists()
}

// --- I/O functions ---

/// Load settings from disk.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub(crate) fn load() -> Result<SettingsConfig, SettingsConfigError> {
    let base = dirs::config_dir().ok_or(SettingsConfigError::NoConfigDir)?;
    load_in(&base)
}

pub(crate) fn load_in(base: &Path) -> Result<SettingsConfig, SettingsConfigError> {
    let path = settings_path_from(base);
    let contents = std::fs::read_to_string(&path).context(ReadFileSnafu)?;
    let mut config: SettingsConfig = toml::from_str(&contents).context(TomlParseSnafu)?;
    let migrated = resolve_server_tokens(base, &mut config)?;
    if migrated {
        save_in(&config, base)?;
    }
    Ok(config)
}

/// Save settings to disk. Creates parent directory if needed.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot
/// be written.
pub(crate) fn save(config: &SettingsConfig) -> Result<(), SettingsConfigError> {
    let base = dirs::config_dir().ok_or(SettingsConfigError::NoConfigDir)?;
    save_in(config, &base)
}

pub(crate) fn save_in(config: &SettingsConfig, base: &Path) -> Result<(), SettingsConfigError> {
    let path = settings_path_from(base);
    let parent = path.parent().ok_or(SettingsConfigError::NoConfigDir)?;
    std::fs::create_dir_all(parent).context(CreateDirSnafu)?;
    let mut persisted = config.clone();
    persist_server_tokens(base, &mut persisted)?;
    let contents = toml::to_string_pretty(&persisted).context(TomlSerializeSnafu)?;

    // WHY: Restrict settings.toml to owner-only on Unix. Windows uses the
    // default ACLs inside the user's config directory.
    #[cfg(unix)]
    let mut tmp = {
        use std::os::unix::fs::PermissionsExt as _;
        let perms = std::fs::Permissions::from_mode(0o600);
        tempfile::Builder::new()
            .permissions(perms)
            .tempfile_in(parent)
            .context(WriteFileSnafu)?
    };
    #[cfg(not(unix))]
    let mut tmp = tempfile::Builder::new()
        .tempfile_in(parent)
        .context(WriteFileSnafu)?;
    tmp.write_all(contents.as_bytes()).context(WriteFileSnafu)?;
    tmp.as_file().sync_all().context(WriteFileSnafu)?;
    tmp.persist(&path)
        .map_err(|e| e.error)
        .context(WriteFileSnafu)?;
    Ok(())
}

fn resolve_server_tokens(
    base: &Path,
    config: &mut SettingsConfig,
) -> Result<bool, SettingsConfigError> {
    let mut migrated_plaintext = false;
    for server in &mut config.servers {
        if let Some(token) = server.auth_token.clone() {
            let token_ref = server
                .auth_token_ref
                .clone()
                .unwrap_or_else(|| server_token_ref(&server.id));
            secret_store::store_token(base, &token_ref, &token).context(SecretStoreSnafu)?;
            server.auth_token_ref = Some(token_ref);
            migrated_plaintext = true;
            continue;
        }

        if let Some(token_ref) = server.auth_token_ref.as_deref() {
            server.auth_token =
                secret_store::load_token(base, token_ref).context(SecretStoreSnafu)?;
        }
    }
    Ok(migrated_plaintext)
}

fn persist_server_tokens(
    base: &Path,
    config: &mut SettingsConfig,
) -> Result<(), SettingsConfigError> {
    for server in &mut config.servers {
        if let Some(token) = server.auth_token.as_deref() {
            let token_ref = server
                .auth_token_ref
                .clone()
                .unwrap_or_else(|| server_token_ref(&server.id));
            secret_store::store_token(base, &token_ref, token).context(SecretStoreSnafu)?;
            server.auth_token_ref = Some(token_ref);
        } else if server.auth_token_ref.is_none() {
            let token_ref = server_token_ref(&server.id);
            secret_store::delete_token(base, &token_ref).context(SecretStoreSnafu)?;
        }
        server.auth_token = None;
    }
    Ok(())
}

/// Load settings from disk, falling back to defaults on any error.
///
/// On first launch (no settings file), returns defaults *without* writing a
/// file. Writing a default settings file before first-run detection would
/// cause clean profiles to skip the setup wizard.
pub(crate) fn load_or_default() -> SettingsConfig {
    if is_first_run() {
        return SettingsConfig::default();
    }

    match load() {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("failed to load settings config, using defaults: {e}");
            SettingsConfig::default()
        }
    }
}

#[cfg(test)]
pub(crate) fn load_or_default_in(base: &Path) -> SettingsConfig {
    if is_first_run_in(base) {
        return SettingsConfig::default();
    }

    match load_in(base) {
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

pub(crate) fn upsert_active_server_in(
    base: &Path,
    server_url: String,
    auth_token: Option<String>,
) -> Result<(), SettingsConfigError> {
    let config = crate::state::connection::ConnectionConfig {
        server_url,
        auth_token,
        ..crate::state::connection::ConnectionConfig::default()
    };
    upsert_active_connection_in(base, &config)
}

pub(crate) fn upsert_active_connection_in(
    base: &Path,
    connection: &crate::state::connection::ConnectionConfig,
) -> Result<(), SettingsConfigError> {
    let settings_path = settings_path_from(base);
    let mut config = if settings_path.exists() {
        load_in(base)?
    } else {
        SettingsConfig::default()
    };
    let mut store = config.server_store();

    let existing_id = store
        .servers
        .iter()
        .find(|s| s.url == connection.server_url)
        .map(|s| s.id.clone());

    if let Some(id) = existing_id {
        if let Some(entry) = store.servers.iter_mut().find(|s| s.id == id) {
            entry.auth_token_ref = connection
                .auth_token
                .as_ref()
                .map(|_| server_token_ref(&id));
            entry.auth_token = connection.auth_token.clone();
            entry.csrf_header_name = connection.csrf_header_name.clone();
            entry.csrf_header_value = connection.csrf_header_value.clone();
        }
        store.set_active(&id);
    } else {
        let id = unique_server_id(&store);
        let auth_token_ref = connection
            .auth_token
            .as_ref()
            .map(|_| server_token_ref(&id));
        store.servers.push(ServerEntry {
            id: id.clone(),
            name: "My Aletheia".to_string(),
            url: connection.server_url.clone(),
            auth_token_ref,
            auth_token: connection.auth_token.clone(),
            csrf_header_name: connection.csrf_header_name.clone(),
            csrf_header_value: connection.csrf_header_value.clone(),
            last_connected: None,
        });
        store.set_active(&id);
    }

    config.apply_server_store(&store);
    save_in(&config, base)
}

fn unique_server_id(store: &ServerConfigStore) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let base_id = format!("srv_{ms:013x}");
    if !store.servers.iter().any(|entry| entry.id == base_id) {
        return base_id;
    }

    let mut suffix = 1;
    loop {
        let id = format!("{base_id}_{suffix}");
        if !store.servers.iter().any(|entry| entry.id == id) {
            return id;
        }
        suffix += 1;
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::state::settings::{AppearanceSettings, KeybindingStore, ServerConfigStore};

    fn temp_base() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        (dir, base)
    }

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
        let expected_ref = server_token_ref(&id);
        assert_eq!(
            restored.servers[0].auth_token_ref.as_deref(),
            Some(expected_ref.as_str())
        );
    }

    #[test]
    fn server_store_round_trips_csrf_contract_via_config() {
        let mut store = ServerConfigStore::default();
        let config = crate::state::connection::ConnectionConfig {
            server_url: "http://csrf:3000".to_string(),
            csrf_header_name: Some("x-custom-csrf".to_string()),
            csrf_header_value: Some("custom-csrf-value".to_string()),
            ..crate::state::connection::ConnectionConfig::default()
        };
        let id = store.add_connection("CSRF".to_string(), &config);
        store.set_active(&id);

        let config = SettingsConfig::from_state(
            &store,
            &AppearanceSettings::default(),
            &KeybindingStore::default(),
        );
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: SettingsConfig = toml::from_str(&toml_str).unwrap();
        let restored_store = restored.server_store();
        let active = restored_store.active().expect("active server");

        assert_eq!(active.csrf_header_name.as_deref(), Some("x-custom-csrf"));
        assert_eq!(
            active.csrf_header_value.as_deref(),
            Some("custom-csrf-value")
        );
    }

    #[test]
    fn settings_config_serialization_omits_raw_bearer_token() {
        let raw_token = "raw-bearer-token-4491";
        let mut store = ServerConfigStore::default();
        store.add(
            "Secure".to_string(),
            "http://secure:3000".to_string(),
            Some(raw_token.to_string()),
        );
        let config = SettingsConfig::from_state(
            &store,
            &AppearanceSettings::default(),
            &KeybindingStore::default(),
        );

        let toml_str = toml::to_string_pretty(&config).unwrap();

        assert!(!toml_str.contains(raw_token));
        assert!(!toml_str.contains("auth_token ="));
        assert!(toml_str.contains("auth_token_ref"));
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
        let value = is_first_run();
        assert!(matches!(value, true | false));
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

    #[test]
    fn default_config_serializes_to_valid_toml() {
        // WHY: first-launch path writes defaults to disk; verify the
        // serialized form round-trips without data loss.
        let config = SettingsConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: SettingsConfig = toml::from_str(&toml_str).unwrap();

        assert!(restored.servers.is_empty());
        assert!(restored.active_server.is_none());
        assert_eq!(restored.appearance.theme, "system");
        assert_eq!(restored.appearance.font_size, 14);
        assert_eq!(restored.appearance.density, "comfortable");
        assert_eq!(restored.appearance.accent_color, "#5b6af0");
        assert!(restored.keybinding_overrides.is_empty());
    }

    // --- Clean profile / migration / persistence tests ---

    #[test]
    fn clean_profile_is_first_run() {
        let (_dir, base) = temp_base();
        assert!(is_first_run_in(&base));
    }

    #[test]
    fn clean_profile_load_or_default_does_not_write_settings() {
        let (_dir, base) = temp_base();
        let config = load_or_default_in(&base);

        assert!(config.servers.is_empty());
        assert!(config.active_server.is_none());
        assert!(!settings_path_from(&base).exists());
    }

    #[test]
    fn legacy_desktop_toml_makes_first_run_false() {
        let (_dir, base) = temp_base();
        let legacy = legacy_config_path_from(&base);
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "[connection]\nserver_url = \"http://old:3000\"\n").unwrap();

        assert!(!is_first_run_in(&base));
    }

    #[test]
    fn upsert_active_server_creates_settings() {
        let (_dir, base) = temp_base();
        let raw_token = "tok-4491";
        upsert_active_server_in(
            &base,
            "http://remote.example.com:18789".to_string(),
            Some(raw_token.to_string()),
        )
        .unwrap();

        let raw_settings = std::fs::read_to_string(settings_path_from(&base)).unwrap();
        assert!(!raw_settings.contains(raw_token));
        assert!(!raw_settings.contains("auth_token ="));
        assert!(raw_settings.contains("auth_token_ref"));

        let config = load_in(&base).unwrap();
        let store = config.server_store();
        let active = store.active().expect("active server after upsert");
        assert_eq!(active.url, "http://remote.example.com:18789");
        assert_eq!(active.auth_token.as_deref(), Some(raw_token));
    }

    #[test]
    fn upsert_active_connection_persists_csrf_contract() {
        let (_dir, base) = temp_base();
        let config = crate::state::connection::ConnectionConfig {
            server_url: "http://remote.example.com:18789".to_string(),
            csrf_header_name: Some("x-custom-csrf".to_string()),
            csrf_header_value: Some("custom-csrf-value".to_string()),
            ..crate::state::connection::ConnectionConfig::default()
        };

        upsert_active_connection_in(&base, &config).unwrap();

        let restored = load_in(&base).unwrap();
        let store = restored.server_store();
        let active = store.active().expect("active server after upsert");
        assert_eq!(active.csrf_header_name.as_deref(), Some("x-custom-csrf"));
        assert_eq!(
            active.csrf_header_value.as_deref(),
            Some("custom-csrf-value")
        );
    }

    #[test]
    fn upsert_existing_url_updates_token_and_makes_active() {
        let (_dir, base) = temp_base();
        let first_token = "first-4491";
        let second_token = "second-4491";
        upsert_active_server_in(
            &base,
            "http://same:3000".to_string(),
            Some(first_token.to_string()),
        )
        .unwrap();
        upsert_active_server_in(
            &base,
            "http://same:3000".to_string(),
            Some(second_token.to_string()),
        )
        .unwrap();

        let raw_settings = std::fs::read_to_string(settings_path_from(&base)).unwrap();
        assert!(!raw_settings.contains(first_token));
        assert!(!raw_settings.contains(second_token));

        let config = load_in(&base).unwrap();
        let store = config.server_store();
        assert_eq!(store.servers.len(), 1);
        assert_eq!(
            store.active().unwrap().auth_token.as_deref(),
            Some(second_token)
        );
    }

    #[test]
    fn legacy_plaintext_settings_token_migrates_out_of_toml_on_load() {
        let (_dir, base) = temp_base();
        let settings = settings_path_from(&base);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let raw_token = "legacy-settings-token-4491";
        std::fs::write(
            &settings,
            format!(
                r#"
active_server = "srv_legacy"

[[servers]]
id = "srv_legacy"
name = "Legacy"
url = "http://legacy:3000"
auth_token = "{raw_token}"
"#
            ),
        )
        .unwrap();

        let config = load_in(&base).unwrap();
        let store = config.server_store();
        let active = store.active().unwrap();

        assert_eq!(active.auth_token.as_deref(), Some(raw_token));
        let migrated = std::fs::read_to_string(&settings).unwrap();
        assert!(!migrated.contains(raw_token));
        assert!(!migrated.contains("auth_token ="));
        assert!(migrated.contains("auth_token_ref"));
    }

    #[test]
    fn upsert_existing_malformed_settings_returns_error() {
        let (_dir, base) = temp_base();
        let settings = settings_path_from(&base);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "not valid toml = [").unwrap();

        let err = upsert_active_server_in(&base, "http://server:3000".to_string(), None)
            .expect_err("malformed settings should not be overwritten");
        assert!(
            matches!(err, SettingsConfigError::TomlParse { .. }),
            "expected parse error, got {err:?}"
        );
    }

    #[test]
    fn server_switch_reload_restores_active_server() {
        let (_dir, base) = temp_base();
        upsert_active_server_in(&base, "http://server-a:3000".to_string(), None).unwrap();
        upsert_active_server_in(&base, "http://server-b:3000".to_string(), None).unwrap();

        let config = load_in(&base).unwrap();
        let store = config.server_store();
        assert_eq!(store.active().unwrap().url, "http://server-b:3000");
        assert_eq!(store.servers.len(), 2);
    }
}
