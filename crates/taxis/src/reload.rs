//! Hot-reload classification, config diff, and reload orchestration.

use serde_json::Value;
use snafu::Snafu;
use tracing::{info, warn};

use crate::config::AletheiaConfig;
use crate::oikos::Oikos;

/// Field path prefixes that require a process restart to take effect.
///
/// All other config paths are hot-reloadable: they take effect immediately
/// when the in-memory config is swapped.
const RESTART_PREFIXES: &[&str] = &[
    "gateway.port",
    "gateway.bind",
    "gateway.tls",
    "gateway.auth.mode",
    "gateway.csrf",
    "gateway.bodyLimit",
    "channels",
];

/// Returns true if changing the given dotted field path requires a restart.
#[must_use]
pub fn requires_restart(field_path: &str) -> bool {
    RESTART_PREFIXES
        .iter()
        .any(|prefix| field_path.starts_with(prefix))
}

/// Returns the list of field path prefixes that require restart.
#[must_use]
pub fn restart_prefixes() -> &'static [&'static str] {
    RESTART_PREFIXES
}

/// A single changed field between two config versions.
#[derive(Debug, Clone)]
pub struct ConfigChange {
    /// Dotted path to the changed field (e.g. `agents.defaults.thinkingBudget`).
    pub path: String,
    /// Whether this change requires a restart to take effect.
    pub restart_required: bool,
}

/// Result of comparing two configs.
#[derive(Debug, Clone)]
pub struct ConfigDiff {
    /// Fields that changed between old and new config.
    pub changes: Vec<ConfigChange>,
}

impl ConfigDiff {
    /// Returns true if no fields changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Returns only the changes that are hot-reloadable (no restart needed).
    #[must_use]
    pub fn hot_changes(&self) -> Vec<&ConfigChange> {
        self.changes
            .iter()
            .filter(|c| !c.restart_required)
            .collect()
    }

    /// Returns only the changes that require a restart.
    #[must_use]
    pub fn cold_changes(&self) -> Vec<&ConfigChange> {
        self.changes.iter().filter(|c| c.restart_required).collect()
    }
}

/// Compare two configs and return the list of changed field paths.
///
/// Serializes both configs to JSON and walks the tree to find leaf differences.
/// Each changed path is classified as hot-reloadable or cold (restart required).
#[must_use]
pub fn diff_configs(old: &AletheiaConfig, new: &AletheiaConfig) -> ConfigDiff {
    let old_value = serde_json::to_value(old).unwrap_or(Value::Null);
    let new_value = serde_json::to_value(new).unwrap_or(Value::Null);

    let mut changes = Vec::new();
    diff_values(&old_value, &new_value, String::new(), &mut changes);

    ConfigDiff { changes }
}

/// Log all changes from a config diff at appropriate levels.
pub fn log_diff(diff: &ConfigDiff) {
    if diff.is_empty() {
        info!("config reload: no changes detected");
        return;
    }

    for change in &diff.changes {
        if change.restart_required {
            warn!(
                path = %change.path,
                "config reload: cold value changed (restart required to take effect)"
            );
        } else {
            info!(path = %change.path, "config reload: value updated");
        }
    }

    let hot = diff.hot_changes().len();
    let cold = diff.cold_changes().len();
    info!(
        hot_reloaded = hot,
        restart_required = cold,
        "config reload complete"
    );
}

/// Errors from config reload attempts.
#[derive(Debug, Snafu)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location) are self-documenting via display format"
)]
pub enum ReloadError {
    /// Failed to load config from disk.
    #[snafu(display("failed to load config: {source}"))]
    Load {
        source: crate::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// New config failed validation; old config is preserved.
    #[snafu(display("config validation failed: {source}"))]
    Validation {
        source: crate::validate::ValidationError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Outcome of a successful reload preparation.
pub struct ReloadOutcome {
    /// The validated new config ready to be swapped in.
    pub new_config: AletheiaConfig,
    /// Diff between the old (current) and new config.
    pub diff: ConfigDiff,
}

/// Load config from disk, validate, and diff against current.
///
/// On success, returns the new config and a diff. The caller is responsible
/// for atomically swapping the config into the live store and notifying
/// subscribers.
///
/// # Errors
///
/// Returns [`ReloadError::Load`] if reading from disk fails.
/// Returns [`ReloadError::Validation`] if the new config is invalid
/// (the current config is unchanged).
#[expect(
    clippy::result_large_err,
    reason = "figment::Error is inherently large"
)]
pub fn prepare_reload(
    oikos: &Oikos,
    current: &AletheiaConfig,
) -> Result<ReloadOutcome, ReloadError> {
    use snafu::ResultExt;

    let new_config = crate::loader::load_config(oikos).context(LoadSnafu)?;
    crate::validate::validate_config(&new_config).context(ValidationSnafu)?;

    let diff = diff_configs(current, &new_config);

    Ok(ReloadOutcome { new_config, diff })
}

fn diff_values(old: &Value, new: &Value, prefix: String, changes: &mut Vec<ConfigChange>) {
    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) => {
            for (key, old_val) in old_map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match new_map.get(key) {
                    Some(new_val) => diff_values(old_val, new_val, path, changes),
                    None => changes.push(ConfigChange {
                        restart_required: requires_restart(&path),
                        path,
                    }),
                }
            }
            for key in new_map.keys() {
                if !old_map.contains_key(key) {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    changes.push(ConfigChange {
                        restart_required: requires_restart(&path),
                        path,
                    });
                }
            }
        }
        (Value::Array(old_arr), Value::Array(new_arr)) => {
            if old_arr != new_arr {
                changes.push(ConfigChange {
                    restart_required: requires_restart(&prefix),
                    path: prefix,
                });
            }
        }
        (old_val, new_val) => {
            if old_val != new_val {
                changes.push(ConfigChange {
                    restart_required: requires_restart(&prefix),
                    path: prefix,
                });
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::result_large_err,
    reason = "figment::Jail closures return Box<dyn Error>; test error size doesn't matter"
)]
mod tests {
    use super::*;

    #[test]
    fn gateway_port_requires_restart() {
        assert!(requires_restart("gateway.port"));
    }

    #[test]
    fn tls_settings_require_restart() {
        assert!(requires_restart("gateway.tls.enabled"));
        assert!(requires_restart("gateway.tls.certPath"));
    }

    #[test]
    fn channel_settings_require_restart() {
        assert!(requires_restart("channels.signal.enabled"));
    }

    #[test]
    fn agent_defaults_hot_reloadable() {
        assert!(!requires_restart("agents.defaults.timeoutSeconds"));
        assert!(!requires_restart("agents.defaults.maxToolIterations"));
        assert!(!requires_restart("agents.defaults.thinkingBudget"));
    }

    #[test]
    fn maintenance_hot_reloadable() {
        assert!(!requires_restart("maintenance.traceRotation.maxAgeDays"));
        assert!(!requires_restart(
            "maintenance.dbMonitoring.warnThresholdMb"
        ));
    }

    #[test]
    fn embedding_hot_reloadable() {
        assert!(!requires_restart("embedding.provider"));
    }

    #[test]
    fn diff_identical_configs_is_empty() {
        let config = AletheiaConfig::default();
        let diff = diff_configs(&config, &config);
        assert!(diff.is_empty(), "identical configs should have no diff");
    }

    #[test]
    fn diff_detects_hot_reloadable_change() {
        let old = AletheiaConfig::default();
        let mut new = old.clone();
        new.agents.defaults.thinking_budget = 20_000;

        let diff = diff_configs(&old, &new);
        assert!(!diff.is_empty(), "changed config should have diff");
        assert!(
            diff.hot_changes()
                .iter()
                .any(|c| c.path.contains("thinkingBudget")),
            "thinkingBudget should appear in hot changes"
        );
        assert!(
            diff.cold_changes().is_empty(),
            "no cold changes expected for agent defaults"
        );
    }

    #[test]
    fn diff_detects_cold_change() {
        let old = AletheiaConfig::default();
        let mut new = old.clone();
        new.gateway.port = 9999;

        let diff = diff_configs(&old, &new);
        assert!(!diff.is_empty(), "changed config should have diff");
        assert!(
            diff.cold_changes()
                .iter()
                .any(|c| c.path.contains("gateway.port")),
            "gateway.port should appear in cold changes"
        );
    }

    #[test]
    fn diff_detects_multiple_changes() {
        let old = AletheiaConfig::default();
        let mut new = old.clone();
        new.agents.defaults.thinking_budget = 20_000;
        new.gateway.port = 9999;
        new.maintenance.trace_rotation.max_age_days = 7;

        let diff = diff_configs(&old, &new);
        assert!(
            diff.changes.len() >= 3,
            "expected at least 3 changes, got {}",
            diff.changes.len()
        );
    }

    #[test]
    fn diff_hot_and_cold_partition_correctly() {
        let old = AletheiaConfig::default();
        let mut new = old.clone();
        new.agents.defaults.max_tool_iterations = 500;
        new.gateway.port = 9999;

        let diff = diff_configs(&old, &new);
        let hot = diff.hot_changes();
        let cold = diff.cold_changes();

        assert!(!hot.is_empty(), "should have hot changes");
        assert!(!cold.is_empty(), "should have cold changes");

        for c in &hot {
            assert!(
                !c.restart_required,
                "hot change should not require restart: {}",
                c.path
            );
        }
        for c in &cold {
            assert!(
                c.restart_required,
                "cold change should require restart: {}",
                c.path
            );
        }
    }

    #[test]
    fn prepare_reload_succeeds_with_valid_config() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file(
                "config/aletheia.toml",
                "[agents.defaults]\nthinkingBudget = 20000\n",
            )?;

            let oikos = Oikos::from_root(jail.directory());
            let current = AletheiaConfig::default();

            let outcome = prepare_reload(&oikos, &current).map_err(|e| e.to_string())?;
            assert!(
                !outcome.diff.is_empty(),
                "thinkingBudget change should produce a diff"
            );
            assert_eq!(
                outcome.new_config.agents.defaults.thinking_budget, 20_000,
                "new config should have updated value"
            );
            assert!(
                outcome.diff.cold_changes().is_empty(),
                "agent defaults are hot-reloadable"
            );
            Ok(())
        });
    }

    #[test]
    fn prepare_reload_rejects_invalid_config() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            // WHY: maxToolIterations=0 is rejected by the validator (must be 1..=10000).
            jail.create_file(
                "config/aletheia.toml",
                "[agents.defaults]\nmaxToolIterations = 0\n",
            )?;

            let oikos = Oikos::from_root(jail.directory());
            let current = AletheiaConfig::default();

            let result = prepare_reload(&oikos, &current);
            assert!(result.is_err(), "invalid config should be rejected");
            Ok(())
        });
    }

    #[test]
    fn prepare_reload_preserves_current_on_rejection() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file(
                "config/aletheia.toml",
                "[agents.defaults]\nmaxToolIterations = 0\n",
            )?;

            let oikos = Oikos::from_root(jail.directory());
            let current = AletheiaConfig::default();
            let original_budget = current.agents.defaults.thinking_budget;

            let _ = prepare_reload(&oikos, &current);

            assert_eq!(
                current.agents.defaults.thinking_budget, original_budget,
                "current config must not be modified on rejection"
            );
            Ok(())
        });
    }

    #[test]
    fn prepare_reload_cold_values_appear_in_diff() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            jail.create_file(
                "config/aletheia.toml",
                "[gateway]\nport = 9999\n\n[agents.defaults]\nthinkingBudget = 20000\n",
            )?;

            let oikos = Oikos::from_root(jail.directory());
            let current = AletheiaConfig::default();

            let outcome = prepare_reload(&oikos, &current).map_err(|e| e.to_string())?;

            let cold = outcome.diff.cold_changes();
            assert!(
                cold.iter().any(|c| c.path.contains("gateway.port")),
                "gateway.port should appear as a cold change"
            );

            let hot = outcome.diff.hot_changes();
            assert!(
                hot.iter().any(|c| c.path.contains("thinkingBudget")),
                "thinkingBudget should appear as a hot change"
            );
            Ok(())
        });
    }

    #[test]
    fn prepare_reload_no_changes_when_config_identical() {
        figment::Jail::expect_with(|jail| {
            std::fs::create_dir_all(jail.directory().join("config")).map_err(|e| e.to_string())?;
            let default_toml =
                toml::to_string(&AletheiaConfig::default()).map_err(|e| e.to_string())?;
            std::fs::write(jail.directory().join("config/aletheia.toml"), default_toml)
                .map_err(|e| e.to_string())?;

            let oikos = Oikos::from_root(jail.directory());
            let current = AletheiaConfig::default();

            let outcome = prepare_reload(&oikos, &current).map_err(|e| e.to_string())?;
            assert!(
                outcome.diff.is_empty(),
                "identical config should produce empty diff"
            );
            Ok(())
        });
    }
}
