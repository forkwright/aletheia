//! Integration tests for taxis's loader: TOML cascade, env interpolation,
//! roundtrip, and hot-reload preparation.
//!
//! Part of aletheia#2814. Sibling binary to `tests/public_api.rs`.

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

mod common;

use taxis::config::{AletheiaConfig, DeploymentTarget, ProviderKind};
use taxis::loader::{load_config, write_config};
use taxis::oikos::Oikos;
use taxis::reload::{prepare_reload, restart_prefixes};
use taxis::test_support::EnvJail;

use common::write_toml;

// WHY: EnvJail owns a tempdir and a process-wide lock. Integration tests use
// the jail's own directory (seeded with config/, data/, nous/) rather than a
// separate TempDir so env-var scopes are always serialised.
fn seed_instance(jail: &EnvJail) -> &std::path::Path {
    let root = jail.directory();
    std::fs::create_dir_all(root.join("config")).expect("create config dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::create_dir_all(root.join("nous")).expect("create nous dir");
    root
}

fn is_gguf_model_id(model: &str) -> bool {
    std::path::Path::new(model)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"))
}

// ─── TOML loading: cascade (defaults → file → env) ──────────────────────

#[test]
fn load_config_returns_defaults_when_no_toml_present() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    let oikos = Oikos::from_root(root);
    let config = load_config(&oikos).unwrap();

    let default = AletheiaConfig::default();
    assert_eq!(
        config.gateway.port, default.gateway.port,
        "no toml should preserve default port"
    );
    assert_eq!(
        config.embedding.dimension, default.embedding.dimension,
        "no toml should preserve default dimension"
    );
}

#[test]
fn load_config_applies_gateway_port_override_from_toml() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    write_toml(root, "[gateway]\nport = 4242\n");

    let oikos = Oikos::from_root(root);
    let config = load_config(&oikos).unwrap();
    assert_eq!(config.gateway.port, 4242);
}

#[test]
fn load_config_nested_override_preserves_unrelated_defaults() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    write_toml(root, "[gateway]\nport = 9999\n");

    let oikos = Oikos::from_root(root);
    let config = load_config(&oikos).unwrap();
    assert_eq!(config.gateway.port, 9999);
    // WHY: contract -- overriding one field must not wipe out defaults
    // for other fields in the same section or sibling sections.
    assert_eq!(config.gateway.bind, "localhost");
    assert_eq!(config.embedding.provider, "candle");
}

#[test]
fn load_config_env_var_overrides_toml_value() {
    let mut jail = EnvJail::new();
    let root = seed_instance(&jail).to_path_buf();
    write_toml(&root, "[gateway]\nport = 1111\n");
    jail.set_env("ALETHEIA_GATEWAY__PORT", "2222");

    let oikos = Oikos::from_root(&root);
    let config = load_config(&oikos).unwrap();
    assert_eq!(config.gateway.port, 2222);
}

#[test]
fn load_config_env_var_overrides_embedding_cloud_fields() {
    let mut jail = EnvJail::new();
    let root = seed_instance(&jail).to_path_buf();
    jail.set_env("ALETHEIA_EMBEDDING__PROVIDER", "openai-compat");
    jail.set_env("ALETHEIA_EMBEDDING__BASEURL", "http://127.0.0.1:5005/v1");
    jail.set_env("ALETHEIA_EMBEDDING__APIKEYENV", "ALETHEIA_EMBEDDING_KEY");

    let oikos = Oikos::from_root(&root);
    let config = load_config(&oikos).unwrap();
    assert_eq!(config.embedding.provider, "openai-compat");
    assert_eq!(
        config.embedding.base_url,
        Some("http://127.0.0.1:5005/v1".to_owned())
    );
    assert_eq!(
        config.embedding.api_key_env,
        Some("ALETHEIA_EMBEDDING_KEY".to_owned())
    );
}

#[test]
fn load_config_env_var_preserves_numeric_looking_string_leaf() {
    let mut jail = EnvJail::new();
    let root = seed_instance(&jail).to_path_buf();
    jail.set_env("ALETHEIA_AGENTS__DEFAULTS__MODEL__PRIMARY", "+05550100123");

    let oikos = Oikos::from_root(&root);
    let config = load_config(&oikos).unwrap();
    assert_eq!(
        config.agents.defaults.model_defaults.model.primary,
        "+05550100123"
    );
}

#[test]
fn load_config_env_var_type_error_names_applied_var() {
    let mut jail = EnvJail::new();
    let root = seed_instance(&jail).to_path_buf();
    jail.set_env("ALETHEIA_GATEWAY__TLS__ENABLED", "definitely-not-bool");

    let oikos = Oikos::from_root(&root);
    let err = load_config(&oikos).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ALETHEIA_GATEWAY__TLS__ENABLED"),
        "error should name the env var applied before deserialization failed: {msg}"
    );
}

#[test]
fn load_config_parses_camel_case_keys() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    // WHY: taxis uses #[serde(rename_all = "camelCase")] everywhere;
    // TOML keys must be camelCase even though Rust fields are snake_case.
    write_toml(root, "[embedding]\nprovider = \"mock\"\ndimension = 512\n");

    let oikos = Oikos::from_root(root);
    let config = load_config(&oikos).unwrap();
    assert_eq!(config.embedding.provider, "mock");
    assert_eq!(config.embedding.dimension, 512);
}

#[test]
fn instance_example_provider_entries_load_strictly() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    write_toml(
        root,
        include_str!("../../../instance.example/config/aletheia.toml"),
    );

    let oikos = Oikos::from_root(root);
    let config = load_config(&oikos).expect("example config should load");

    let anthropic = config
        .providers
        .iter()
        .find(|provider| provider.name == "anthropic-cloud")
        .expect("anthropic provider should be declared");
    assert_eq!(anthropic.kind, ProviderKind::Anthropic);
    assert_eq!(anthropic.deployment_target, DeploymentTarget::Cloud);

    for name in ["local-chat", "local-code", "local-inference", "local-vlm"] {
        let provider = config
            .providers
            .iter()
            .find(|provider| provider.name == name)
            .unwrap_or_else(|| panic!("{name} provider should be declared"));

        assert_eq!(provider.kind, ProviderKind::OpenAiCompatible);
        assert_eq!(provider.deployment_target, DeploymentTarget::LocalHosted);
        assert!(
            provider.models.iter().any(|model| is_gguf_model_id(model)),
            "{name} should include the llama-server model id"
        );
        assert!(
            provider.models.iter().any(|model| !is_gguf_model_id(model)),
            "{name} should keep a short routing alias"
        );
    }
}

#[test]
fn load_config_rejects_encrypted_value_when_primary_key_is_missing() {
    let mut jail = EnvJail::new();
    let root = seed_instance(&jail).to_path_buf();
    // WHY: the loader must never silently pass an encrypted value
    // through as plaintext. Closes a class of "silent crypto failure"
    // bugs where the server starts with enc: strings in memory.
    write_toml(&root, "[gateway.auth]\nsigningKey = \"enc:dGVzdA==\"\n");
    jail.set_env("ALETHEIA_PRIMARY_KEY", "/nonexistent/path-missing-key.key");

    let oikos = Oikos::from_root(&root);
    let err = load_config(&oikos).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("encrypted") || msg.contains("decrypt"),
        "error should mention encryption/decryption, got: {msg}"
    );
}

// ─── Env var interpolation via the loader ──────────────────────────────

#[test]
fn interpolate_applies_default_when_env_var_unset() {
    let mut jail = EnvJail::new();
    let root = seed_instance(&jail).to_path_buf();
    // NOTE: _TAXIS_TEST_UNSET_PORT_A is not set in the jail
    jail.remove_env("_TAXIS_TEST_UNSET_PORT_A");
    write_toml(
        &root,
        "[gateway]\nport = ${_TAXIS_TEST_UNSET_PORT_A:-4242}\n",
    );

    let oikos = Oikos::from_root(&root);
    let config = load_config(&oikos).unwrap();
    assert_eq!(
        config.gateway.port, 4242,
        "default from :-syntax should apply when var is unset"
    );
}

#[test]
fn interpolate_substitutes_env_var_value_when_set() {
    let mut jail = EnvJail::new();
    let root = seed_instance(&jail).to_path_buf();
    jail.set_env("_TAXIS_TEST_SET_PORT_B", "7777");
    write_toml(&root, "[gateway]\nport = ${_TAXIS_TEST_SET_PORT_B:-1111}\n");

    let oikos = Oikos::from_root(&root);
    let config = load_config(&oikos).unwrap();
    assert_eq!(config.gateway.port, 7777);
}

#[test]
fn interpolate_required_var_missing_aborts_load_with_user_message() {
    let mut jail = EnvJail::new();
    let root = seed_instance(&jail).to_path_buf();
    // NOTE: _TAXIS_TEST_REQ_C is not set
    jail.remove_env("_TAXIS_TEST_REQ_C");
    write_toml(
        &root,
        "[gateway.auth]\nnoneRole = \"${_TAXIS_TEST_REQ_C:?missing required role}\"\n",
    );

    let oikos = Oikos::from_root(&root);
    let err = load_config(&oikos).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("_TAXIS_TEST_REQ_C"),
        "error should name the unset var: {msg}"
    );
    assert!(
        msg.contains("missing required role"),
        "error should include user-supplied message: {msg}"
    );
}

// ─── write_config + load_config roundtrip ──────────────────────────────

#[test]
fn write_then_load_preserves_gateway_port_override() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    let oikos = Oikos::from_root(root);

    let mut config = AletheiaConfig::default();
    config.gateway.port = 13_131;

    write_config(&oikos, &config).unwrap();
    let loaded = load_config(&oikos).unwrap();
    assert_eq!(loaded.gateway.port, 13_131);
}

#[test]
fn write_then_load_preserves_embedding_dimension_override() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    let oikos = Oikos::from_root(root);

    let mut config = AletheiaConfig::default();
    config.embedding.dimension = 1024;

    write_config(&oikos, &config).unwrap();
    let loaded = load_config(&oikos).unwrap();
    assert_eq!(loaded.embedding.dimension, 1024);
}

#[test]
#[cfg(unix)]
fn write_config_creates_file_with_mode_0600() {
    use std::os::unix::fs::PermissionsExt;

    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    let oikos = Oikos::from_root(root);
    let config = AletheiaConfig::default();

    write_config(&oikos, &config).unwrap();

    let toml_path = root.join("config").join("aletheia.toml");
    let meta = std::fs::metadata(&toml_path).expect("stat config file");
    let mode = meta.permissions().mode() & 0o777;
    // WHY(#1710): config file may contain signing keys and must be 0600 so
    // only the owning user can read it.
    assert_eq!(
        mode, 0o600,
        "aletheia.toml mode should be 0600, got {mode:o}"
    );
}

// ─── Hot-reload classification ──────────────────────────────────────────

#[test]
fn restart_prefixes_lists_gateway_port() {
    let prefixes = restart_prefixes();
    assert!(
        prefixes.contains(&"gateway.port"),
        "gateway.port must be in restart_prefixes, got {prefixes:?}"
    );
}

#[test]
fn restart_prefixes_lists_channels() {
    let prefixes = restart_prefixes();
    assert!(prefixes.contains(&"channels"));
}

#[test]
fn prepare_reload_classifies_gateway_port_change_as_cold() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    write_toml(root, "[gateway]\nport = 7890\n");

    let oikos = Oikos::from_root(root);
    let current = AletheiaConfig::default();
    let outcome = prepare_reload(&oikos, &current).unwrap();

    assert!(
        outcome
            .diff
            .cold_changes()
            .iter()
            .any(|c| c.path.contains("gateway.port")),
        "gateway.port should appear as a cold change"
    );
}

#[test]
fn prepare_reload_rejects_invalid_config_and_preserves_current() {
    let jail = EnvJail::new();
    let root = seed_instance(&jail);
    // WHY: maxToolIterations = 0 is rejected by the validator.
    write_toml(root, "[agents.defaults]\nmaxToolIterations = 0\n");

    let oikos = Oikos::from_root(root);
    let current = AletheiaConfig::default();
    let original = current.agents.defaults.max_tool_iterations;

    let result = prepare_reload(&oikos, &current);
    assert!(result.is_err(), "invalid config should fail prepare_reload");
    assert_eq!(
        current.agents.defaults.max_tool_iterations, original,
        "current config must be preserved on rejection"
    );
}
