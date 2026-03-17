//! Figment-based configuration loading with TOML cascade.

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use snafu::ResultExt;
use tracing::warn;

use crate::config::AletheiaConfig;
use crate::encrypt;
use crate::error::{FigmentSnafu, Result, SerializeTomlSnafu, WriteConfigSnafu};
use crate::oikos::Oikos;

/// Load configuration with cascade: defaults → TOML → environment.
///
/// Resolution order (later wins):
/// 1. Compiled defaults ([`AletheiaConfig::default()`])
/// 2. `{oikos.config()}/aletheia.toml` (if it exists), with `enc:` values decrypted
/// 3. Environment variables: `ALETHEIA_*` (e.g. `ALETHEIA_GATEWAY__PORT=9000`)
///
/// Encrypted values (`enc:` prefix) are transparently decrypted using the
/// master key from `~/.config/aletheia/master.key`. If the key is missing,
/// encrypted values pass through unchanged with a warning.
///
/// # Errors
///
/// Returns [`crate::error::Error::Figment`] if the configuration cascade produces an invalid or
/// unextractable result.
#[expect(
    clippy::result_large_err,
    reason = "figment::Error is inherently large"
)]
#[must_use = "this returns a Result that may contain a read error"]
pub fn load_config(oikos: &Oikos) -> Result<AletheiaConfig> {
    let toml_path = oikos.config().join("aletheia.toml");
    let yaml_path = oikos.config().join("aletheia.yaml");

    let mut figment = Figment::new().merge(Serialized::defaults(AletheiaConfig::default()));

    if toml_path.exists() {
        let toml_content =
            std::fs::read_to_string(&toml_path).context(crate::error::ReadConfigSnafu {
                path: toml_path.clone(),
            })?;

        let decrypted_content = decrypt_toml_content(&toml_content);
        figment = figment.merge(Toml::string(&decrypted_content));
    } else if yaml_path.exists() {
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

    figment = figment.merge(Env::prefixed("ALETHEIA_").split("__"));

    figment.extract().context(FigmentSnafu)
}

/// Parse TOML content, decrypt any `enc:` values, and serialize back.
///
/// If the master key is unavailable or the TOML is unparseable, returns the
/// original content unchanged.
fn decrypt_toml_content(content: &str) -> String {
    let mut value: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return content.to_owned(),
    };

    let master_key = match encrypt::master_key_path() {
        Some(path) => match encrypt::load_master_key(&path) {
            Ok(key) => key,
            Err(e) => {
                warn!(error = %e, "failed to load master key, encrypted values will not be decrypted");
                None
            }
        },
        None => None,
    };

    encrypt::decrypt_toml_values(&mut value, master_key.as_ref());

    toml::to_string(&value).unwrap_or_else(|_| content.to_owned())
}

/// Write configuration to the instance TOML file.
///
/// Uses atomic write: writes to a `.tmp` file, then renames. This prevents
/// corruption if the process is killed during write.
///
/// # Errors
///
/// Returns [`crate::error::Error::SerializeToml`] if the config cannot be serialized to TOML.
/// Returns [`crate::error::Error::WriteConfig`] if the config directory cannot be created or the
/// file cannot be written.
#[expect(
    clippy::result_large_err,
    reason = "figment::Error is inherently large"
)]
#[must_use = "this returns a Result that may contain a write error"]
pub fn write_config(oikos: &Oikos, config: &AletheiaConfig) -> Result<()> {
    let toml = toml::to_string(config).map_err(|e| {
        SerializeTomlSnafu {
            reason: e.to_string(),
        }
        .build()
    })?;

    let config_dir = oikos.config();
    std::fs::create_dir_all(&config_dir).context(WriteConfigSnafu {
        path: config_dir.clone(),
    })?;

    let target = config_dir.join("aletheia.toml");
    let tmp = config_dir.join("aletheia.toml.tmp");

    std::fs::write(&tmp, toml).context(WriteConfigSnafu { path: tmp.clone() })?;
    std::fs::rename(&tmp, &target).context(WriteConfigSnafu { path: target })?;

    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::result_large_err,
    reason = "figment::Jail closures return Box<dyn Error>; test error size doesn't matter"
)]
mod tests {
    use super::*;

    // NOTE: All loader tests run inside figment::Jail to isolate env vars.

    #[test]
    fn load_with_no_yaml_uses_defaults() {
        figment::Jail::expect_with(|jail| {
            let oikos = Oikos::from_root(jail.directory());
            let config = load_config(&oikos).map_err(|e| e.to_string())?;

            assert_eq!(config.agents.defaults.context_tokens, 200_000);
            assert_eq!(config.gateway.port, 18789);
            assert_eq!(config.agents.defaults.model.primary, "claude-sonnet-4-6");
            Ok(())
        });
    }

    #[test]
    fn load_from_toml_file() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file(
                "config/aletheia.toml",
                "[gateway]\nport = 9999\n\n[agents.defaults]\ncontextTokens = 100000\n",
            )?;

            let oikos = Oikos::from_root(jail.directory());
            let config = load_config(&oikos).map_err(|e| e.to_string())?;

            assert_eq!(config.gateway.port, 9999);
            assert_eq!(config.agents.defaults.context_tokens, 100_000);
            assert_eq!(config.agents.defaults.model.primary, "claude-sonnet-4-6");
            Ok(())
        });
    }

    #[test]
    fn env_overrides_toml() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file("config/aletheia.toml", "[gateway]\nport = 9999\n")?;
            jail.set_env("ALETHEIA_GATEWAY__PORT", "7777");

            let oikos = Oikos::from_root(jail.directory());
            let config = load_config(&oikos).map_err(|e| e.to_string())?;

            assert_eq!(config.gateway.port, 7777);
            Ok(())
        });
    }

    #[test]
    fn missing_dir_still_loads_defaults() {
        figment::Jail::expect_with(|_jail| {
            let oikos = Oikos::from_root("/nonexistent/path/that/does/not/exist");
            let config = load_config(&oikos).map_err(|e| e.to_string())?;

            assert_eq!(config.gateway.port, 18789);
            assert_eq!(config.agents.defaults.context_tokens, 200_000);
            Ok(())
        });
    }

    #[test]
    fn write_then_load_roundtrip() {
        figment::Jail::expect_with(|jail| {
            // NOTE: figment::Jail doesn't auto-create the config dir, so create it first.
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;

            let oikos = Oikos::from_root(jail.directory());
            let mut config = AletheiaConfig::default();
            config.gateway.port = 9876;

            write_config(&oikos, &config).map_err(|e| e.to_string())?;
            let loaded = load_config(&oikos).map_err(|e| e.to_string())?;

            assert_eq!(loaded.gateway.port, 9876);
            assert_eq!(loaded.agents.defaults.context_tokens, 200_000);
            Ok(())
        });
    }
}
