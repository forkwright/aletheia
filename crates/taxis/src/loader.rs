//! Figment-based configuration loading with YAML cascade.

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Yaml};
use snafu::ResultExt;

use crate::config::AletheiaConfig;
use crate::error::{FigmentSnafu, Result};
use crate::oikos::Oikos;

/// Load [`AletheiaConfig`] with a three-source cascade: defaults → YAML file → environment.
///
/// Resolution order (later entries override earlier ones):
/// 1. Compiled defaults via [`AletheiaConfig::default()`]
/// 2. `{oikos.config()}/aletheia.yaml` — merged on top of defaults if the file exists
/// 3. Environment variables with the `ALETHEIA_` prefix, using `__` as a path separator
///    (e.g. `ALETHEIA_GATEWAY__PORT=9000` overrides `gateway.port`)
///
/// Missing YAML file is not an error — the loader silently falls through to env vars.
///
/// # Errors
///
/// Returns [`crate::error::Error::Figment`] if:
/// - The YAML file exists but cannot be parsed
/// - An environment variable cannot be coerced to the target field type
/// - Figment deserialization into [`AletheiaConfig`] fails for any reason
#[expect(
    clippy::result_large_err,
    reason = "figment::Error is inherently large"
)]
pub fn load_config(oikos: &Oikos) -> Result<AletheiaConfig> {
    let yaml_path = oikos.config().join("aletheia.yaml");

    let mut figment = Figment::new().merge(Serialized::defaults(AletheiaConfig::default()));

    if yaml_path.exists() {
        figment = figment.merge(Yaml::file(&yaml_path));
    }

    figment = figment.merge(Env::prefixed("ALETHEIA_").split("__"));

    figment.extract().context(FigmentSnafu)
}

#[cfg(test)]
mod tests {
    use super::*;

    // All loader tests run inside figment::Jail to isolate env vars.

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
    fn load_from_yaml_file() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file(
                "config/aletheia.yaml",
                "gateway:\n  port: 9999\nagents:\n  defaults:\n    contextTokens: 100000\n",
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
    fn env_overrides_yaml() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file("config/aletheia.yaml", "gateway:\n  port: 9999\n")?;
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
}
