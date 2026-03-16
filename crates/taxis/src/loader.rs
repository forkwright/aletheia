//! Figment-based configuration loading with TOML/YAML cascade.

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml, Yaml};
use snafu::ResultExt;
use tracing::warn;

use crate::config::AletheiaConfig;
use crate::error::{FigmentSnafu, Result, SerializeTomlSnafu, WriteConfigSnafu};
use crate::oikos::Oikos;

/// Load configuration with cascade: defaults → config file → environment.
///
/// Resolution order (later wins):
/// 1. Compiled defaults ([`AletheiaConfig::default()`])
/// 2. `{oikos.config()}/aletheia.toml` or `aletheia.yaml` (TOML takes precedence)
/// 3. Environment variables: `ALETHEIA_*` (e.g. `ALETHEIA_GATEWAY__PORT=9000`)
#[expect(
    clippy::result_large_err,
    reason = "figment::Error is inherently large"
)]
pub fn load_config(oikos: &Oikos) -> Result<AletheiaConfig> {
    let toml_path = oikos.config().join("aletheia.toml");
    let yaml_path = oikos.config().join("aletheia.yaml");

    let mut figment = Figment::new().merge(Serialized::defaults(AletheiaConfig::default()));

    let has_toml = toml_path.exists();
    let has_yaml = yaml_path.exists();

    if has_toml && has_yaml {
        warn!(
            "Both aletheia.toml and aletheia.yaml found — using TOML, ignoring YAML. \
             Remove aletheia.yaml to silence this warning."
        );
        figment = figment.merge(Toml::file(&toml_path));
    } else if has_toml {
        figment = figment.merge(Toml::file(&toml_path));
    } else if has_yaml {
        figment = figment.merge(Yaml::file(&yaml_path));
    } else {
        warn!(
            "No config file found, using defaults. \
             Create aletheia.toml to configure. See docs/CONFIGURATION.md."
        );
    }

    figment = figment.merge(Env::prefixed("ALETHEIA_").split("__"));

    figment.extract().context(FigmentSnafu)
}

/// Write configuration to the instance TOML file.
///
/// Uses atomic write: writes to a `.tmp` file, then renames. This prevents
/// corruption if the process is killed during write.
#[expect(
    clippy::result_large_err,
    reason = "figment::Error is inherently large"
)]
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
    fn load_from_yaml_file() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file(
                "config/aletheia.yaml",
                "gateway:\n  port: 8888\nagents:\n  defaults:\n    contextTokens: 50000\n",
            )?;

            let oikos = Oikos::from_root(jail.directory());
            let config = load_config(&oikos).map_err(|e| e.to_string())?;

            assert_eq!(config.gateway.port, 8888);
            assert_eq!(config.agents.defaults.context_tokens, 50_000);
            assert_eq!(config.agents.defaults.model.primary, "claude-sonnet-4-6");
            Ok(())
        });
    }

    #[test]
    fn toml_takes_precedence_over_yaml() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file("config/aletheia.toml", "[gateway]\nport = 1111\n")?;
            jail.create_file("config/aletheia.yaml", "gateway:\n  port: 2222\n")?;

            let oikos = Oikos::from_root(jail.directory());
            let config = load_config(&oikos).map_err(|e| e.to_string())?;

            assert_eq!(config.gateway.port, 1111);
            Ok(())
        });
    }

    #[test]
    fn env_overrides_yaml() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file("config/aletheia.yaml", "gateway:\n  port: 8888\n")?;
            jail.set_env("ALETHEIA_GATEWAY__PORT", "5555");

            let oikos = Oikos::from_root(jail.directory());
            let config = load_config(&oikos).map_err(|e| e.to_string())?;

            assert_eq!(config.gateway.port, 5555);
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
