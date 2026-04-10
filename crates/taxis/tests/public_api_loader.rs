//! Integration tests for taxis's loader: TOML cascade, env interpolation,
//! roundtrip, and hot-reload preparation.
//!
//! Part of aletheia#2814. Sibling binary to `tests/public_api.rs`.

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::result_large_err,
    reason = "figment::Jail::expect_with closures return a boxed dynamic error; test error size is irrelevant"
)]

mod common;

use taxis::config::AletheiaConfig;
use taxis::loader::{load_config, write_config};
use taxis::oikos::Oikos;
use taxis::reload::{prepare_reload, restart_prefixes};

use common::{make_valid_instance, write_toml};

// ─── TOML loading: cascade (defaults → file → env) ──────────────────────

#[test]
fn load_config_returns_defaults_when_no_toml_present() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        let oikos = Oikos::from_root(dir.path());
        let config = load_config(&oikos).map_err(|e| e.to_string())?;

        let default = AletheiaConfig::default();
        assert_eq!(
            config.gateway.port, default.gateway.port,
            "no toml should preserve default port"
        );
        assert_eq!(
            config.embedding.dimension, default.embedding.dimension,
            "no toml should preserve default dimension"
        );
        Ok(())
    });
}

#[test]
fn load_config_applies_gateway_port_override_from_toml() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        write_toml(dir.path(), "[gateway]\nport = 4242\n");

        let oikos = Oikos::from_root(dir.path());
        let config = load_config(&oikos).map_err(|e| e.to_string())?;
        assert_eq!(config.gateway.port, 4242);
        Ok(())
    });
}

#[test]
fn load_config_nested_override_preserves_unrelated_defaults() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        write_toml(dir.path(), "[gateway]\nport = 9999\n");

        let oikos = Oikos::from_root(dir.path());
        let config = load_config(&oikos).map_err(|e| e.to_string())?;
        assert_eq!(config.gateway.port, 9999);
        // WHY: contract -- overriding one field must not wipe out defaults
        // for other fields in the same section or sibling sections.
        assert_eq!(config.gateway.bind, "localhost");
        assert_eq!(config.embedding.provider, "candle");
        Ok(())
    });
}

#[test]
fn load_config_env_var_overrides_toml_value() {
    figment::Jail::expect_with(|jail| {
        let dir = make_valid_instance();
        write_toml(dir.path(), "[gateway]\nport = 1111\n");
        jail.set_env("ALETHEIA_GATEWAY__PORT", "2222");

        let oikos = Oikos::from_root(dir.path());
        let config = load_config(&oikos).map_err(|e| e.to_string())?;
        assert_eq!(config.gateway.port, 2222);
        Ok(())
    });
}

#[test]
fn load_config_parses_camel_case_keys() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        // WHY: taxis uses #[serde(rename_all = "camelCase")] everywhere;
        // TOML keys must be camelCase even though Rust fields are snake_case.
        write_toml(
            dir.path(),
            "[embedding]\nprovider = \"mock\"\ndimension = 512\n",
        );

        let oikos = Oikos::from_root(dir.path());
        let config = load_config(&oikos).map_err(|e| e.to_string())?;
        assert_eq!(config.embedding.provider, "mock");
        assert_eq!(config.embedding.dimension, 512);
        Ok(())
    });
}

#[test]
fn load_config_rejects_encrypted_value_when_primary_key_is_missing() {
    figment::Jail::expect_with(|jail| {
        let dir = make_valid_instance();
        // WHY: the loader must never silently pass an encrypted value
        // through as plaintext. Closes a class of "silent crypto failure"
        // bugs where the server starts with enc: strings in memory.
        write_toml(
            dir.path(),
            "[gateway.auth]\nsigningKey = \"enc:dGVzdA==\"\n",
        );
        jail.set_env("ALETHEIA_PRIMARY_KEY", "/nonexistent/path-missing-key.key");

        let oikos = Oikos::from_root(dir.path());
        let err = load_config(&oikos).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("encrypted") || msg.contains("decrypt"),
            "error should mention encryption/decryption, got: {msg}"
        );
        Ok(())
    });
}

// ─── Env var interpolation via the loader ──────────────────────────────

#[test]
fn interpolate_applies_default_when_env_var_unset() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        // NOTE: _TAXIS_TEST_UNSET_PORT_A is not set in the jail
        write_toml(
            dir.path(),
            "[gateway]\nport = ${_TAXIS_TEST_UNSET_PORT_A:-4242}\n",
        );

        let oikos = Oikos::from_root(dir.path());
        let config = load_config(&oikos).map_err(|e| e.to_string())?;
        assert_eq!(
            config.gateway.port, 4242,
            "default from :-syntax should apply when var is unset"
        );
        Ok(())
    });
}

#[test]
fn interpolate_substitutes_env_var_value_when_set() {
    figment::Jail::expect_with(|jail| {
        let dir = make_valid_instance();
        jail.set_env("_TAXIS_TEST_SET_PORT_B", "7777");
        write_toml(
            dir.path(),
            "[gateway]\nport = ${_TAXIS_TEST_SET_PORT_B:-1111}\n",
        );

        let oikos = Oikos::from_root(dir.path());
        let config = load_config(&oikos).map_err(|e| e.to_string())?;
        assert_eq!(config.gateway.port, 7777);
        Ok(())
    });
}

#[test]
fn interpolate_required_var_missing_aborts_load_with_user_message() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        // NOTE: _TAXIS_TEST_REQ_C is not set
        write_toml(
            dir.path(),
            "[gateway.auth]\nnoneRole = \"${_TAXIS_TEST_REQ_C:?missing required role}\"\n",
        );

        let oikos = Oikos::from_root(dir.path());
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
        Ok(())
    });
}

// ─── write_config + load_config roundtrip ──────────────────────────────

#[test]
fn write_then_load_preserves_gateway_port_override() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        let oikos = Oikos::from_root(dir.path());

        let mut config = AletheiaConfig::default();
        config.gateway.port = 13_131;

        write_config(&oikos, &config).map_err(|e| e.to_string())?;
        let loaded = load_config(&oikos).map_err(|e| e.to_string())?;
        assert_eq!(loaded.gateway.port, 13_131);
        Ok(())
    });
}

#[test]
fn write_then_load_preserves_embedding_dimension_override() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        let oikos = Oikos::from_root(dir.path());

        let mut config = AletheiaConfig::default();
        config.embedding.dimension = 1024;

        write_config(&oikos, &config).map_err(|e| e.to_string())?;
        let loaded = load_config(&oikos).map_err(|e| e.to_string())?;
        assert_eq!(loaded.embedding.dimension, 1024);
        Ok(())
    });
}

#[test]
#[cfg(unix)]
fn write_config_creates_file_with_mode_0600() {
    use std::os::unix::fs::PermissionsExt;

    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        let oikos = Oikos::from_root(dir.path());
        let config = AletheiaConfig::default();

        write_config(&oikos, &config).map_err(|e| e.to_string())?;

        let toml_path = dir.path().join("config").join("aletheia.toml");
        let meta = std::fs::metadata(&toml_path).expect("stat config file");
        let mode = meta.permissions().mode() & 0o777;
        // WHY: closes #1710 -- config file may contain signing keys and must
        // be 0600 so only the owning user can read it.
        assert_eq!(mode, 0o600, "aletheia.toml mode should be 0600, got {mode:o}");
        Ok(())
    });
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
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        write_toml(dir.path(), "[gateway]\nport = 7890\n");

        let oikos = Oikos::from_root(dir.path());
        let current = AletheiaConfig::default();
        let outcome = prepare_reload(&oikos, &current).map_err(|e| e.to_string())?;

        assert!(
            outcome
                .diff
                .cold_changes()
                .iter()
                .any(|c| c.path.contains("gateway.port")),
            "gateway.port should appear as a cold change"
        );
        Ok(())
    });
}

#[test]
fn prepare_reload_rejects_invalid_config_and_preserves_current() {
    figment::Jail::expect_with(|_jail| {
        let dir = make_valid_instance();
        // WHY: maxToolIterations = 0 is rejected by the validator.
        write_toml(dir.path(), "[agents.defaults]\nmaxToolIterations = 0\n");

        let oikos = Oikos::from_root(dir.path());
        let current = AletheiaConfig::default();
        let original = current.agents.defaults.max_tool_iterations;

        let result = prepare_reload(&oikos, &current);
        assert!(result.is_err(), "invalid config should fail prepare_reload");
        assert_eq!(
            current.agents.defaults.max_tool_iterations, original,
            "current config must be preserved on rejection"
        );
        Ok(())
    });
}
