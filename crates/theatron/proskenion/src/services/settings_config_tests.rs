//! Tests for `settings_config` — split out to keep the parent module under
//! the RUST/file-too-long line budget.

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

/// `settings.toml` is owner-only, and stays so when it replaces an existing
/// file — the mode has to be applied to each replacement, not just to the
/// first create.
#[cfg(unix)]
#[test]
fn saved_settings_are_owner_only_on_every_write() {
    use std::os::unix::fs::PermissionsExt as _;

    let (_dir, base) = temp_base();
    let path = settings_path_from(&base);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    save_in(&SettingsConfig::default(), &base).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(
        mode & 0o777,
        0o600,
        "expected 0o600, got {:o}",
        mode & 0o777
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

// --- Budget persistence (#5797) ---

#[test]
fn budget_survives_reload() {
    let (_dir, base) = temp_base();
    save_budget_in(
        &BudgetConfig {
            monthly_limit_usd: 500.0,
        },
        &base,
    )
    .unwrap();

    let config = load_in(&base).unwrap();
    assert_eq!(config.budget_config().monthly_limit_usd, 500.0);
}

#[test]
fn budget_defaults_to_zero_on_clean_profile() {
    let (_dir, base) = temp_base();
    let config = load_or_default_in(&base);
    assert_eq!(config.budget_config().monthly_limit_usd, 0.0);
}

#[test]
fn missing_budget_key_in_existing_settings_deserializes_to_zero() {
    // WHY: settings.toml files written before the budget field existed
    // must still load — deny_unknown_fields only rejects unrecognized
    // keys, not missing ones, but this pins that guarantee for `budget`
    // specifically.
    let (_dir, base) = temp_base();
    let settings = settings_path_from(&base);
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, "active_server = \"srv_x\"\n").unwrap();

    let config = load_in(&base).unwrap();
    assert_eq!(config.budget_config().monthly_limit_usd, 0.0);
}

#[test]
fn saving_appearance_state_preserves_previously_set_budget() {
    // WHY: this is the exact defect #5797 reports one step later — a
    // save from an unrelated panel (appearance/server/keybindings) must
    // not silently zero out a budget the operator already set.
    let (_dir, base) = temp_base();
    save_budget_in(
        &BudgetConfig {
            monthly_limit_usd: 250.0,
        },
        &base,
    )
    .unwrap();

    let store = ServerConfigStore::default();
    let appearance = AppearanceSettings {
        theme: "dark".to_string(),
        ..AppearanceSettings::default()
    };
    let keybindings = KeybindingStore::default();
    save_state_in(&store, &appearance, &keybindings, &base);

    let config = load_in(&base).unwrap();
    assert_eq!(config.budget_config().monthly_limit_usd, 250.0);
    assert_eq!(config.appearance_settings().theme, "dark");
}

#[test]
fn setting_budget_preserves_previously_saved_appearance() {
    let (_dir, base) = temp_base();
    let store = ServerConfigStore::default();
    let appearance = AppearanceSettings {
        theme: "light".to_string(),
        ..AppearanceSettings::default()
    };
    let keybindings = KeybindingStore::default();
    save_state_in(&store, &appearance, &keybindings, &base);

    save_budget_in(
        &BudgetConfig {
            monthly_limit_usd: 75.0,
        },
        &base,
    )
    .unwrap();

    let config = load_in(&base).unwrap();
    assert_eq!(config.appearance_settings().theme, "light");
    assert_eq!(config.budget_config().monthly_limit_usd, 75.0);
}

#[test]
fn budget_round_trips_through_toml() {
    let mut config = SettingsConfig::default();
    config.apply_budget(&BudgetConfig {
        monthly_limit_usd: 1234.5,
    });
    let toml_str = toml::to_string_pretty(&config).unwrap();
    let restored: SettingsConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(restored.budget_config().monthly_limit_usd, 1234.5);
}
