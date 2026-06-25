use koina::secret::SecretString;
use koina::system::TestSystem;

use super::*;
use crate::test_support::EnvJail;

#[test]
fn load_with_no_yaml_uses_defaults() {
    let jail = EnvJail::new();
    let oikos = Oikos::from_root(jail.directory());
    let config = load_config(&oikos).unwrap_or_else(|e| panic!("load: {e}"));

    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 200_000,
        "no-config default context tokens should be 200k"
    );
    assert_eq!(
        config.gateway.port, 18789,
        "no-config default port should be 18789"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.model.primary, "claude-sonnet-4-6",
        "no-config default model should be sonnet"
    );
}

#[test]
fn load_from_toml_file() {
    let jail = EnvJail::new();
    jail.create_file(
        "config/aletheia.toml",
        "[gateway]\nport = 9999\n\n[agents.defaults]\ncontextTokens = 100000\n",
    );

    let oikos = Oikos::from_root(jail.directory());
    let config = load_config(&oikos).unwrap_or_else(|e| panic!("load: {e}"));

    assert_eq!(
        config.gateway.port, 9999,
        "toml port override should take effect"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 100_000,
        "toml context tokens override should take effect"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.model.primary, "claude-sonnet-4-6",
        "unset model should use default"
    );
}

#[test]
fn env_overrides_toml() {
    let mut jail = EnvJail::new();
    jail.create_file("config/aletheia.toml", "[gateway]\nport = 9999\n");
    jail.set_env("ALETHEIA_GATEWAY__PORT", "7777");

    let oikos = Oikos::from_root(jail.directory());
    let config = load_config(&oikos).unwrap_or_else(|e| panic!("load: {e}"));

    assert_eq!(
        config.gateway.port, 7777,
        "env var should override toml port"
    );
}

#[test]
fn env_overlay_preserves_unknown_path_autotyping() {
    let mut jail = EnvJail::new();
    jail.set_env("_TAXIS_TEST_UNKNOWN__PORT", "5555");
    let mut root = serde_json::json!({});

    let applied = apply_env_overlay(&mut root, "_TAXIS_TEST_", "__");

    assert_eq!(applied, vec!["_TAXIS_TEST_UNKNOWN__PORT"]);
    assert_eq!(
        root.get("unknown").and_then(|value| value.get("port")),
        Some(&serde_json::json!(5555))
    );
}

#[test]
fn missing_dir_still_loads_defaults() {
    let _jail = EnvJail::new();
    let oikos = Oikos::from_root("/nonexistent/path/that/does/not/exist");
    let config = load_config(&oikos).unwrap_or_else(|e| panic!("load: {e}"));

    assert_eq!(
        config.gateway.port, 18789,
        "missing dir should fall back to default port"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 200_000,
        "missing dir should fall back to default context tokens"
    );
}

#[test]
fn write_then_load_roundtrip() {
    let jail = EnvJail::new();
    // NOTE: EnvJail doesn't auto-create the config dir, so create it first.
    std::fs::create_dir_all(jail.directory().join("config")).expect("create config dir");

    let oikos = Oikos::from_root(jail.directory());
    let mut config = AletheiaConfig::default();
    config.gateway.port = 9876;

    write_config(&oikos, &config).unwrap_or_else(|e| panic!("write: {e}"));
    let loaded = load_config(&oikos).unwrap_or_else(|e| panic!("load: {e}"));

    assert_eq!(
        loaded.gateway.port, 9876,
        "written port should survive roundtrip"
    );
    assert_eq!(
        loaded.agents.defaults.model_defaults.context_tokens, 200_000,
        "default context tokens should survive roundtrip"
    );
}

#[test]
fn write_config_persists_secret_string_values_unredacted() {
    let jail = EnvJail::new();
    std::fs::create_dir_all(jail.directory().join("config")).expect("create config dir");

    let oikos = Oikos::from_root(jail.directory());
    let mut config = AletheiaConfig::default();
    let signing_key = "synthetic-signing-key-survives-write";
    config.gateway.auth.signing_key = Some(SecretString::from(signing_key));

    write_config(&oikos, &config).unwrap_or_else(|e| panic!("write: {e}"));

    let persisted =
        std::fs::read_to_string(oikos.config().join("aletheia.toml")).expect("read config");
    assert!(
        persisted.contains(signing_key),
        "persisted config should contain the raw signing key"
    );
    assert!(
        !persisted.contains("[REDACTED]"),
        "persisted config should not contain the redaction marker"
    );

    let loaded = load_config(&oikos).unwrap_or_else(|e| panic!("load: {e}"));
    assert_eq!(
        loaded
            .gateway
            .auth
            .signing_key
            .as_ref()
            .map(SecretString::expose_secret),
        Some(signing_key),
        "signing key should survive write/load roundtrip"
    );
}

// ── load_config_with (FileSystem trait) ──────────────────────────────

#[test]
fn load_config_with_uses_in_memory_toml() {
    let jail = EnvJail::new();
    let oikos = Oikos::from_root(jail.directory());
    let toml_path = oikos.config().join("aletheia.toml");

    let mut fs = TestSystem::new();
    fs.add_file(toml_path, b"[gateway]\nport = 4242\n");

    let config = load_config_with(&oikos, &fs).unwrap_or_else(|e| panic!("load: {e}"));
    assert_eq!(
        config.gateway.port, 4242,
        "in-memory toml port should be loaded"
    );
}

#[test]
fn load_config_with_uses_defaults_when_no_toml() {
    let _jail = EnvJail::new();
    let oikos = Oikos::from_root("/nonexistent");
    let fs = TestSystem::new(); // empty — no files

    let config = load_config_with(&oikos, &fs).unwrap_or_else(|e| panic!("load: {e}"));
    assert_eq!(
        config.gateway.port, 18789,
        "empty filesystem should use default port"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 200_000,
        "empty filesystem should use default context tokens"
    );
}

#[test]
fn load_config_with_merges_env_over_toml() {
    let mut jail = EnvJail::new();
    jail.set_env("ALETHEIA_GATEWAY__PORT", "5555");

    let oikos = Oikos::from_root(jail.directory());
    let toml_path = oikos.config().join("aletheia.toml");

    let mut fs = TestSystem::new();
    fs.add_file(toml_path, b"[gateway]\nport = 1111\n");

    let config = load_config_with(&oikos, &fs).unwrap_or_else(|e| panic!("load: {e}"));
    assert_eq!(
        config.gateway.port, 5555,
        "env var should override in-memory toml port"
    );
}

#[test]
fn load_config_with_mirrors_data_retention_to_maintenance() {
    let jail = EnvJail::new();
    let oikos = Oikos::from_root(jail.directory());
    let toml_path = oikos.config().join("aletheia.toml");

    let mut fs = TestSystem::new();
    fs.add_file(
        toml_path,
        br"
[data.retention]
enabled = true
sessionMaxAgeDays = 90
orphanMessageMaxAgeDays = 30
maxSessionsPerNous = 200
archiveBeforeDelete = true
",
    );

    let config = load_config_with(&oikos, &fs).unwrap_or_else(|e| panic!("load: {e}"));
    assert!(config.data.retention.enabled);
    assert_eq!(config.data.retention.closed_session_ttl_days, Some(90));
    assert_eq!(config.data.retention.orphan_message_max_age_days, Some(30));
    assert_eq!(config.data.retention.max_sessions_per_nous, 200);
    assert!(config.data.retention.archive_before_delete);
    assert!(config.maintenance.retention.enabled);
    assert_eq!(
        config.maintenance.retention.closed_session_ttl_days,
        Some(90)
    );
    assert_eq!(
        config.maintenance.retention.orphan_message_max_age_days,
        Some(30)
    );
    assert_eq!(config.maintenance.retention.max_sessions_per_nous, 200);
    assert!(config.maintenance.retention.archive_before_delete);
}

#[test]
fn reserved_env_vars_do_not_break_config_load() {
    let mut jail = EnvJail::new();
    // WHY: these variables are documented as reserved for other subsystems;
    // they must not be injected as config keys, or `deny_unknown_fields`
    // rejects them and the server cannot start.
    let root = jail.directory().to_str().expect("utf-8 path").to_owned();
    jail.set_env("ALETHEIA_ROOT", &root);
    jail.set_env(
        "ALETHEIA_JWT_SECRET",
        "synthetic-jwt-secret-must-not-be-injected",
    );
    jail.set_env("ALETHEIA_PRIMARY_KEY", "/nonexistent/primary.key");
    jail.set_env("ALETHEIA_ENV_FILE", "/etc/aletheia/env");
    jail.set_env("ALETHEIA_NOUS", "/srv/aletheia/nous");
    jail.set_env("ALETHEIA_CREDS", "/srv/aletheia/creds.json");
    jail.set_env("ALETHEIA_ALLOW_AUTH_NONE", "1");
    jail.set_env("ALETHEIA_ALLOW_AUTH_NONE_LAN", "1");

    let oikos = Oikos::from_root(jail.directory());
    let config = load_config(&oikos).unwrap_or_else(|e| panic!("load: {e}"));

    assert_eq!(
        config.gateway.port, 18789,
        "reserved env vars must not break default config load"
    );
}

#[test]
#[expect(clippy::unwrap_used, reason = "test assertions")]
fn decrypt_toml_content_returns_original_when_toml_parse_fails() {
    let content = "this is not valid toml ::";
    let result = decrypt_toml_content(content).unwrap();
    assert_eq!(result, content, "unparseable content should pass through");
}

#[test]
#[expect(clippy::unwrap_used, reason = "test assertions")]
fn decrypt_toml_content_roundtrips_valid_toml() {
    let content = "[section]\nkey = 'value'\n";
    let result = decrypt_toml_content(content).unwrap();
    assert!(
        result.contains("section") && result.contains("key"),
        "valid TOML should be preserved, got: {result}"
    );
}

#[test]
fn serialize_toml_propagates_non_serializable_value() {
    let mut map = std::collections::HashMap::new();
    map.insert(1i32, "value");

    let result = serialize_toml(&map);
    assert!(
        result.is_err(),
        "a HashMap<i32, _> must fail to serialize as TOML"
    );
}
