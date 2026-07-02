//! Configuration loading with TOML cascade.
//!
//! Resolution order (later wins):
//! 1. Compiled defaults ([`AletheiaConfig::default()`])
//! 2. `{oikos.config()}/aletheia.toml` (if it exists), with env-var
//!    interpolation (`${VAR:-default}`, `${VAR:?error}`) applied first,
//!    then `enc:` values decrypted.
//! 3. Environment variables: `ALETHEIA_*` (e.g. `ALETHEIA_GATEWAY__PORT=9000`),
//!    with `__` splitting nested keys and lowercasing.
//!
//! Encrypted values (`enc:` prefix) are decrypted after interpolation. If the
//! primary key is missing but `enc:` values are present, startup fails with an
//! actionable error listing the affected fields.

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;

use serde_json::Value as JsonValue;
use snafu::ResultExt;
use tracing::{error, warn};

use koina::disk_space::{DiskSpaceMonitor, DiskStatus};
use koina::system::{FileSystem, RealSystem};

use crate::config::AletheiaConfig;
use crate::config_decrypt;
use crate::error::{ConfigLoadSnafu, LoadSnafu, Result, SerializeTomlSnafu, WriteConfigSnafu};
use crate::interpolate;
use crate::oikos::Oikos;

/// Load configuration with cascade: defaults → TOML → environment.
///
/// Resolution order (later wins):
/// 1. Compiled defaults ([`AletheiaConfig::default()`])
/// 2. `{oikos.config()}/aletheia.toml` (if it exists), with env-var interpolation
///    (`${VAR:-default}`, `${VAR:?error}`) applied first, then `enc:` values decrypted
/// 3. Environment variables: `ALETHEIA_*` (e.g. `ALETHEIA_GATEWAY__PORT=9000`)
///
/// Encrypted values (`enc:` prefix) are transparently decrypted using the
/// primary key from `~/.config/aletheia/primary.key`. If the key is missing
/// but `enc:` values are present, startup fails with an actionable error
/// listing the affected fields.
///
/// Call [`load_config_with`] to supply a custom [`FileSystem`] implementation
/// (e.g. `koina::system::TestSystem` in tests).
///
/// # Errors
///
/// Returns an error if the configuration cascade produces an invalid or
/// unextractable result.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn load_config(oikos: &Oikos) -> Result<AletheiaConfig> {
    load_config_with(oikos, &RealSystem)
}

/// Load configuration using the provided [`FileSystem`] for file access.
///
/// This is the primary implementation; [`load_config`] is a convenience
/// wrapper that passes [`RealSystem`]. Prefer this variant in tests so that
/// TOML files can be supplied in-memory without touching the real disk.
///
/// # Errors
///
/// Returns an error if the TOML file cannot be read.
/// Returns an error if the configuration cascade fails.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn load_config_with(oikos: &Oikos, fs: &impl FileSystem) -> Result<AletheiaConfig> {
    let toml_path = oikos.config().join("aletheia.toml");
    let yaml_path = oikos.config().join("aletheia.yaml");

    // Tier 1: compiled defaults serialized to a JSON value tree.
    // WHY JSON: serde_json::Value is the canonical merge target — TOML deserialises
    // cleanly into it, and the final AletheiaConfig deserialises back out of it.
    let mut root = serde_json::to_value(AletheiaConfig::default()).map_err(|e| {
        LoadSnafu {
            reason: format!("serialize defaults: {e}"),
        }
        .build()
    })?;

    // Tier 2: TOML file (if present), interpolated + decrypted, then deep-merged.
    if fs.exists(&toml_path) {
        let bytes = fs
            .read_file(&toml_path)
            .context(crate::error::ReadConfigSnafu {
                path: toml_path.clone(),
            })?;
        let toml_content = String::from_utf8_lossy(&bytes);
        let interpolated = interpolate::interpolate_env_vars(toml_content.as_ref())?;
        let decrypted_content = config_decrypt::decrypt_toml_content(&interpolated)?;

        let toml_json: JsonValue =
            toml::from_str(&decrypted_content).context(crate::error::ParseTomlSnafu {
                path: toml_path.clone(),
            })?;
        deep_merge(&mut root, toml_json);
    } else if fs.exists(&yaml_path) {
        warn!(
            "Found aletheia.yaml but not aletheia.toml -- run migration or rename. \
             See docs/CONFIGURATION.md."
        );
    } else {
        warn!(
            "No config file found, using defaults. \
             Create aletheia.toml to configure. See docs/CONFIGURATION.md."
        );
    }

    // Tier 3: environment variables, ALETHEIA_ prefix, `__` splitting nested keys.
    let applied_env_vars = apply_env_overlay(&mut root, "ALETHEIA_", "__");
    mirror_data_retention(&mut root);

    serde_json::from_value::<AletheiaConfig>(root).context(ConfigLoadSnafu {
        reason: deserialize_reason(&applied_env_vars),
    })
}

fn mirror_data_retention(root: &mut JsonValue) {
    let Some(retention) = root
        .get("data")
        .and_then(|data| data.get("retention"))
        .cloned()
    else {
        return;
    };

    let Some(root_map) = root.as_object_mut() else {
        return;
    };
    let maintenance = root_map
        .entry("maintenance".to_owned())
        .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
    let Some(maintenance_map) = maintenance.as_object_mut() else {
        return;
    };
    maintenance_map.insert("retention".to_owned(), retention);
}

/// Deep-merge `src` into `dst`. Objects merge by key; everything else replaces.
fn deep_merge(dst: &mut JsonValue, src: JsonValue) {
    match (dst, src) {
        (JsonValue::Object(dst_map), JsonValue::Object(src_map)) => {
            for (key, src_val) in src_map {
                match dst_map.get_mut(&key) {
                    Some(dst_val) => deep_merge(dst_val, src_val),
                    None => {
                        dst_map.insert(key, src_val);
                    }
                }
            }
        }
        (dst_slot, src_val) => *dst_slot = src_val,
    }
}

/// Environment-variable names under the `ALETHEIA_` prefix that are reserved
/// for other subsystems and must never be injected as config-tree keys.
///
/// WHY: `AletheiaConfig` uses `#[serde(deny_unknown_fields)]`; if these
/// process variables were written into the merged JSON they would be rejected
/// as unknown fields and the server would refuse to start.
const RESERVED_ENV_NAMES: [&str; 8] = [
    "ROOT",
    "PRIMARY_KEY",
    "JWT_SECRET",
    "ENV_FILE",
    "NOUS",
    "CREDS",
    "ALLOW_AUTH_NONE",
    "ALLOW_AUTH_NONE_LAN",
];

/// Walk `ALETHEIA_*` env vars and write each into `root`, splitting the name
/// by `separator` to build the nested path. Keys are lowercased to match
/// serde `rename_all = "camelCase"` output (which lowercases single words).
///
/// Known leaves are coerced to the existing JSON leaf type in the
/// defaults-plus-TOML tree. Unknown paths keep the pre-#3447 figment autotyping
/// contract so operators can keep writing `ALETHEIA_GATEWAY__PORT=9000` without quoting.
///
/// WARNING: Reserved process variables (e.g. `ALETHEIA_ROOT`) are skipped so
/// they cannot collide with `#[serde(deny_unknown_fields)]`.
fn apply_env_overlay(root: &mut JsonValue, prefix: &str, separator: &str) -> Vec<String> {
    let mut applied = Vec::new();
    let mut vars: Vec<_> = std::env::vars().collect();
    vars.sort_by(|(left, _), (right, _)| left.cmp(right));

    for (key, value) in vars {
        let Some(rest) = key.strip_prefix(prefix) else {
            continue;
        };
        let path: Vec<String> = rest.split(separator).map(str::to_ascii_lowercase).collect();
        if path.iter().any(String::is_empty) {
            continue;
        }
        if path.first().is_some_and(|head| {
            RESERVED_ENV_NAMES
                .iter()
                .any(|name| name.eq_ignore_ascii_case(head))
        }) {
            continue;
        }
        let parsed = get_path(root, &path).map_or_else(
            || parse_env_value(&value),
            |existing| coerce_env_value(&value, existing),
        );
        set_path(root, &path, parsed);
        applied.push(key);
    }
    applied
}

fn deserialize_reason(applied_env_vars: &[String]) -> String {
    if applied_env_vars.is_empty() {
        "deserialize merged config".to_owned()
    } else {
        format!(
            "deserialize merged config after applying {}",
            applied_env_vars.join(", ")
        )
    }
}

fn get_path<'a>(root: &'a JsonValue, path: &[String]) -> Option<&'a JsonValue> {
    let mut cursor = root;
    for segment in path {
        let map = cursor.as_object()?;
        let key = matching_key(map, segment)?;
        cursor = map.get(key)?;
    }
    Some(cursor)
}

fn matching_key<'a>(map: &'a serde_json::Map<String, JsonValue>, segment: &str) -> Option<&'a str> {
    if let Some((key, _)) = map.get_key_value(segment) {
        return Some(key.as_str());
    }
    map.keys()
        .find(|key| key.eq_ignore_ascii_case(segment))
        .map(String::as_str)
}

fn coerce_env_value(raw: &str, existing: &JsonValue) -> JsonValue {
    match existing {
        JsonValue::Bool(_) => match raw.trim() {
            "true" => JsonValue::Bool(true),
            "false" => JsonValue::Bool(false),
            _ => JsonValue::String(raw.to_owned()),
        },
        JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => parse_env_value(raw),
        JsonValue::String(_) | JsonValue::Null => JsonValue::String(raw.to_owned()),
    }
}

/// Parse an environment-variable string into a JSON scalar. Booleans, integers,
/// floats (if containing `.`), else a string. Preserves the pre-#3447 figment
/// autotyping contract.
fn parse_env_value(raw: &str) -> JsonValue {
    let trimmed = raw.trim();
    if trimmed == "true" {
        return JsonValue::Bool(true);
    }
    if trimmed == "false" {
        return JsonValue::Bool(false);
    }
    if trimmed.contains('.')
        && let Ok(f) = trimmed.parse::<f64>()
        && let Some(n) = serde_json::Number::from_f64(f)
    {
        return JsonValue::Number(n);
    }
    if let Ok(u) = trimmed.parse::<u64>() {
        return JsonValue::Number(serde_json::Number::from(u));
    }
    if let Ok(i) = trimmed.parse::<i64>() {
        return JsonValue::Number(serde_json::Number::from(i));
    }
    JsonValue::String(raw.to_owned())
}

/// Drill into `root` following `path`, creating intermediate objects as needed,
/// and set the leaf to `value`. If an intermediate slot is not an object
/// (e.g. a previously set scalar), it is replaced with a fresh object so the
/// env overlay always wins at the leaf.
fn set_path(root: &mut JsonValue, path: &[String], value: JsonValue) {
    if path.is_empty() {
        return;
    }
    let mut cursor = root;
    let last_idx = path.len() - 1;
    for (i, segment) in path.iter().enumerate() {
        if i == last_idx {
            if !cursor.is_object() {
                *cursor = JsonValue::Object(serde_json::Map::new());
            }
            if let Some(map) = cursor.as_object_mut() {
                let key = matching_key(map, segment).map_or_else(|| segment.clone(), str::to_owned);
                map.insert(key, value);
            }
            return;
        }
        if !cursor.is_object() {
            *cursor = JsonValue::Object(serde_json::Map::new());
        }
        let Some(map) = cursor.as_object_mut() else {
            return;
        };
        let key = matching_key(map, segment).map_or_else(|| segment.clone(), str::to_owned);
        cursor = map
            .entry(key)
            .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
    }
}

/// Read a standalone TOML file, apply env-var interpolation and decrypt
/// `enc:` values, and return the parsed value tree.
///
/// This uses the same interpolation and decryption pipeline as the cascade
/// loader, but does **not** apply compiled defaults or environment-variable
/// overlays.
///
/// Call [`parse_toml_file_with`] to supply a custom [`FileSystem`] implementation
/// (e.g. `koina::system::TestSystem` in tests).
///
/// # Errors
///
/// Returns an error if the file cannot be read, if TOML parsing fails,
/// or if encrypted values are found but the decryption key is missing.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn parse_toml_file(path: &std::path::Path) -> Result<toml::Value> {
    parse_toml_file_with(path, &RealSystem)
}

/// Read a standalone TOML file using the provided [`FileSystem`].
///
/// This is the primary implementation; [`parse_toml_file`] is a convenience
/// wrapper that passes [`RealSystem`]. Prefer this variant in tests so that
/// TOML files can be supplied in-memory without touching the real disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read, if TOML parsing fails,
/// or if encrypted values are found but the decryption key is missing.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn parse_toml_file_with(path: &std::path::Path, fs: &impl FileSystem) -> Result<toml::Value> {
    let bytes = fs.read_file(path).context(crate::error::ReadConfigSnafu {
        path: path.to_path_buf(),
    })?;
    let toml_content = String::from_utf8_lossy(&bytes);
    let interpolated = interpolate::interpolate_env_vars(toml_content.as_ref())?;
    let mut value: toml::Value =
        toml::from_str(&interpolated).context(crate::error::ParseTomlSnafu {
            path: path.to_path_buf(),
        })?;
    config_decrypt::decrypt_toml_value(&mut value)?;
    Ok(value)
}

/// Write configuration to the instance TOML file.
///
/// Uses atomic write: writes to a `.tmp` file, then renames. This prevents
/// corruption if the process is killed during write.
///
/// # Errors
///
/// Returns an error if the config cannot be serialized to TOML.
/// Returns an error if the config directory cannot be created or the
/// file cannot be written.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn write_config(oikos: &Oikos, config: &AletheiaConfig) -> Result<()> {
    write_config_checked(oikos, config, None)
}

/// Write configuration with optional disk space monitoring.
///
/// Config writes are essential (state preservation), so they always proceed.
/// Warning and critical disk states emit tracing diagnostics.
pub(crate) fn write_config_checked(
    oikos: &Oikos,
    config: &AletheiaConfig,
    disk_monitor: Option<&DiskSpaceMonitor>,
) -> Result<()> {
    if let Some(monitor) = disk_monitor {
        match monitor.status() {
            DiskStatus::Warning { available_bytes } => {
                let mb = available_bytes / (1024 * 1024);
                warn!(
                    available_mb = mb,
                    "disk space low, config write proceeding (essential)"
                );
            }
            DiskStatus::Critical { available_bytes } => {
                let mb = available_bytes / (1024 * 1024);
                error!(
                    available_mb = mb,
                    "disk space critical, config write proceeding (essential)"
                );
            }
            _ => {
                // NOTE: DiskStatus::Ok requires no warning; write proceeds silently
            }
        }
    }

    let config_dir = oikos.config();
    let target = config_dir.join("aletheia.toml");
    let tmp = config_dir.join("aletheia.toml.tmp");

    let mut toml_value = toml::Value::try_from(config).map_err(|e| {
        SerializeTomlSnafu {
            reason: e.to_string(),
        }
        .build()
    })?;
    crate::redact::expose_secret_leaves_for_toml(&mut toml_value, config);

    let toml = toml::to_string(&toml_value).map_err(|e| {
        SerializeTomlSnafu {
            reason: e.to_string(),
        }
        .build()
    })?;

    std::fs::create_dir_all(&config_dir).context(WriteConfigSnafu {
        path: config_dir.clone(),
    })?;

    // WHY: mode 0600 ensures config file (which may contain secrets) is
    // readable only by the owning user.
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .context(WriteConfigSnafu { path: tmp.clone() })?;
        f.write_all(toml.as_bytes())
            .context(WriteConfigSnafu { path: tmp.clone() })?;
    }
    std::fs::rename(&tmp, &target).context(WriteConfigSnafu { path: target })?;

    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test harness: seeding fixtures must panic loudly on setup failure"
)]
mod tests {
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
}
