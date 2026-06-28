//! Hot-reload classification, config diff, and reload orchestration.

use std::sync::LazyLock;

use serde_json::Value;
use snafu::Snafu;
use tracing::{info, warn};

use crate::config::AletheiaConfig;
use crate::error::SerializeJsonSnafu;
use crate::oikos::Oikos;
use crate::registry::all_specs;

/// Field path prefixes that require a process restart to take effect.
///
/// All other config paths are hot-reloadable: they take effect immediately
/// when the in-memory config is swapped.
///
/// Derived from the static parameter registry so that a single declaration
/// (the registry's `hot_reloadable` flag) drives both metadata and reload
/// behavior.
static RESTART_PREFIXES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    all_specs()
        .iter()
        .filter(|spec| !spec.hot_reloadable)
        .map(|spec| spec.key)
        .collect()
});

/// Returns true if changing the given dotted field path requires a restart.
#[must_use]
pub(crate) fn requires_restart(field_path: &str) -> bool {
    RESTART_PREFIXES
        .iter()
        .any(|prefix| field_path.starts_with(prefix))
}

/// Returns the list of field path prefixes that require restart.
#[must_use]
pub fn restart_prefixes() -> &'static [&'static str] {
    RESTART_PREFIXES.as_slice()
}

/// Return `staged` with every restart-required changed path restored from `current`.
///
/// The returned config is the live/effective view. Callers may still persist the
/// staged config to disk, but must not broadcast cold values as live runtime state.
///
/// # Errors
///
/// Returns [`serde_json::Error`] if the restored JSON cannot deserialize back
/// into [`AletheiaConfig`].
pub fn preserve_restart_required_values(
    current: &AletheiaConfig,
    staged: &AletheiaConfig,
    diff: &ConfigDiff,
) -> Result<AletheiaConfig, serde_json::Error> {
    let current_value = serde_json::to_value(current)?;
    let mut live_value = serde_json::to_value(staged)?;

    for change in diff.cold_changes() {
        if let Some(prefix) = restart_prefix_for_path(&change.path)
            && let Some(old_value) = value_at_path(&current_value, prefix)
        {
            set_value_at_path(&mut live_value, prefix, old_value.clone());
        }
    }

    serde_json::from_value(live_value)
}

fn restart_prefix_for_path(path: &str) -> Option<&'static str> {
    RESTART_PREFIXES
        .iter()
        .copied()
        .find(|prefix| path.starts_with(prefix))
}

fn value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn set_value_at_path(value: &mut Value, path: &str, replacement: Value) {
    let mut current = value;
    let mut parts = path.split('.').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            if let Value::Object(map) = current {
                map.insert(part.to_owned(), replacement);
            }
            return;
        }
        let Value::Object(map) = current else {
            return;
        };
        let Some(next) = map.get_mut(part) else {
            return;
        };
        current = next;
    }
}

/// A single changed field between two config versions.
#[derive(Debug, Clone)]
// kanon:ignore TOPOLOGY/shallow-struct — plain data carrier for diff output; callers pattern-match fields directly
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
///
/// # Errors
///
/// Returns [`crate::error::Error::SerializeJson`] if either config cannot be
/// serialized to JSON. Previously such failures were silently replaced with
/// [`Value::Null`], producing an empty diff and bypassing reload logic.
pub fn diff_configs(
    old: &AletheiaConfig,
    new: &AletheiaConfig,
) -> Result<ConfigDiff, crate::error::Error> {
    use snafu::ResultExt;

    let old_value = serde_json::to_value(old).context(SerializeJsonSnafu)?;
    let new_value = serde_json::to_value(new).context(SerializeJsonSnafu)?;

    let mut changes = Vec::new();
    diff_values(&old_value, &new_value, String::new(), &mut changes);

    Ok(ConfigDiff { changes })
}

/// Log all changes from a config diff at appropriate levels.
///
/// Cold changes (those requiring restart) are logged at `warn` level with
/// an explicit message that the new value is staged but not yet effective.
/// This satisfies the observability contract: the system's reported state
/// must reflect its actual state.
pub fn log_diff(diff: &ConfigDiff) {
    if diff.is_empty() {
        info!("config reload: no changes detected");
        return;
    }

    for change in &diff.changes {
        if change.restart_required {
            warn!(
                path = %change.path,
                "config reload: cold value changed — new value is staged but NOT effective \
                 until the process is restarted"
            );
        } else {
            info!(path = %change.path, "config reload: hot value applied immediately");
        }
    }

    let hot = diff.hot_changes().len();
    let cold = diff.cold_changes().len();
    if cold > 0 {
        warn!(
            hot_applied = hot,
            cold_staged = cold,
            "config reload: {cold} change(s) require restart to take effect"
        );
    } else {
        info!(
            hot_applied = hot,
            "config reload complete: all changes applied"
        );
    }
}

/// Errors from config reload attempts.
#[derive(Debug, Snafu)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location) are self-documenting via display format"
)]
// kanon:ignore RUST/non-exhaustive-enum — already #[non_exhaustive]; false positive from attribute ordering
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

    /// Failed to diff the current and new configs.
    #[snafu(display("failed to diff configs: {source}"))]
    Diff {
        source: crate::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Outcome of a successful reload preparation.
// kanon:ignore TOPOLOGY/shallow-struct — plain output carrier; callers destructure fields directly after prepare_reload
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
/// Returns [`ReloadError::Diff`] if the current and new configs cannot be
/// compared due to a JSON serialization failure.
pub fn prepare_reload(
    oikos: &Oikos,
    current: &AletheiaConfig,
) -> Result<ReloadOutcome, ReloadError> {
    use snafu::ResultExt;

    let new_config = crate::loader::load_config(oikos).context(LoadSnafu)?;
    crate::validate::validate_config(&new_config).context(ValidationSnafu)?;

    let diff = diff_configs(current, &new_config).context(DiffSnafu)?;

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
mod tests {
    use super::*;
    use crate::test_support::EnvJail;

    #[test]
    fn gateway_port_requires_restart() {
        assert!(
            requires_restart("gateway.port"),
            "gateway.port should require restart"
        );
    }

    #[test]
    fn tls_settings_require_restart() {
        assert!(
            requires_restart("gateway.tls.enabled"),
            "tls enabled should require restart"
        );
        assert!(
            requires_restart("gateway.tls.certPath"),
            "tls cert path should require restart"
        );
    }

    #[test]
    fn sandbox_settings_require_restart() {
        assert!(
            requires_restart("sandbox.enabled"),
            "sandbox.enabled should require restart"
        );
        assert!(
            requires_restart("sandbox.enforcement"),
            "sandbox.enforcement should require restart"
        );
    }

    #[test]
    fn external_tools_require_restart() {
        assert!(
            requires_restart("tools.required.search"),
            "tools change should require restart"
        );
        assert!(
            requires_restart("tools.optional.reader"),
            "tools change should require restart"
        );
    }

    #[test]
    fn restart_prefixes_are_derived_from_registry() {
        let prefixes = restart_prefixes();
        for expected in [
            "gateway.port",
            "gateway.bind",
            "gateway.tls",
            "gateway.auth.mode",
            "gateway.csrf",
            "gateway.bodyLimit",
            "channels",
            "providerBehavior.nonStreamingTimeoutSecs",
            "messaging.pollIntervalMs",
            "messaging.bufferCapacity",
            "apiLimits.idempotencyCapacity",
            "workspace.root",
            "sandbox",
            "tools",
        ] {
            assert!(
                prefixes.contains(&expected),
                "expected restart prefix {expected} to be derived from registry"
            );
        }
    }

    #[test]
    fn channel_settings_require_restart() {
        assert!(
            requires_restart("channels.signal.enabled"),
            "channel enabled should require restart"
        );
    }

    #[test]
    fn agent_defaults_hot_reloadable() {
        assert!(
            !requires_restart("agents.defaults.maxToolIterations"),
            "tool iterations should be hot-reloadable"
        );
        assert!(
            !requires_restart("agents.defaults.thinkingBudget"),
            "thinking budget should be hot-reloadable"
        );
        assert!(
            !requires_restart("agents.defaults.modelDefaults.contextTokens"),
            "context tokens should be hot-reloadable"
        );
    }

    #[test]
    fn maintenance_hot_reloadable() {
        assert!(
            !requires_restart("maintenance.traceRotation.maxAgeDays"),
            "trace rotation should be hot-reloadable"
        );
        assert!(
            !requires_restart("maintenance.dbMonitoring.warnThresholdMb"),
            "db monitoring should be hot-reloadable"
        );
    }

    #[test]
    fn embedding_hot_reloadable() {
        assert!(
            !requires_restart("embedding.provider"),
            "embedding provider should be hot-reloadable"
        );
    }

    #[test]
    fn diff_identical_configs_is_empty() {
        let config = AletheiaConfig::default();
        let diff = diff_configs(&config, &config).unwrap_or_else(|e| panic!("diff configs: {e}"));
        assert!(diff.is_empty(), "identical configs should have no diff");
    }

    #[test]
    fn diff_detects_hot_reloadable_change() {
        let old = AletheiaConfig::default();
        let mut new = old.clone();
        new.agents.defaults.model_defaults.thinking_budget = 20_000;

        let diff = diff_configs(&old, &new).unwrap_or_else(|e| panic!("diff configs: {e}"));
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

        let diff = diff_configs(&old, &new).unwrap_or_else(|e| panic!("diff configs: {e}"));
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
        new.agents.defaults.model_defaults.thinking_budget = 20_000;
        new.gateway.port = 9999;
        new.maintenance.trace_rotation.max_age_days = 7;

        let diff = diff_configs(&old, &new).unwrap_or_else(|e| panic!("diff configs: {e}"));
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

        let diff = diff_configs(&old, &new).unwrap_or_else(|e| panic!("diff configs: {e}"));
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
        let jail = EnvJail::new();
        jail.create_file(
            "config/aletheia.toml",
            "[agents.defaults]\nthinkingBudget = 20000\n",
        );

        let oikos = Oikos::from_root(jail.directory());
        let current = AletheiaConfig::default();

        let outcome = prepare_reload(&oikos, &current).unwrap_or_else(|e| panic!("reload: {e}"));
        assert!(
            !outcome.diff.is_empty(),
            "thinkingBudget change should produce a diff"
        );
        assert_eq!(
            outcome
                .new_config
                .agents
                .defaults
                .model_defaults
                .thinking_budget,
            20_000,
            "new config should have updated value"
        );
        assert!(
            outcome.diff.cold_changes().is_empty(),
            "agent defaults are hot-reloadable"
        );
    }

    #[test]
    fn prepare_reload_rejects_invalid_config() {
        let jail = EnvJail::new();
        // WHY: maxToolIterations=0 is rejected by the validator (must be 1..=10000).
        jail.create_file(
            "config/aletheia.toml",
            "[agents.defaults]\nmaxToolIterations = 0\n",
        );

        let oikos = Oikos::from_root(jail.directory());
        let current = AletheiaConfig::default();

        let result = prepare_reload(&oikos, &current);
        assert!(result.is_err(), "invalid config should be rejected");
    }

    #[test]
    fn prepare_reload_preserves_current_on_rejection() {
        let jail = EnvJail::new();
        jail.create_file(
            "config/aletheia.toml",
            "[agents.defaults]\nmaxToolIterations = 0\n",
        );

        let oikos = Oikos::from_root(jail.directory());
        let current = AletheiaConfig::default();
        let original_budget = current.agents.defaults.model_defaults.thinking_budget;

        let _ = prepare_reload(&oikos, &current);

        assert_eq!(
            current.agents.defaults.model_defaults.thinking_budget, original_budget,
            "current config must not be modified on rejection"
        );
    }

    #[test]
    fn prepare_reload_cold_values_appear_in_diff() {
        let jail = EnvJail::new();
        jail.create_file(
            "config/aletheia.toml",
            "[gateway]\nport = 9999\n\n[agents.defaults]\nthinkingBudget = 20000\n",
        );

        let oikos = Oikos::from_root(jail.directory());
        let current = AletheiaConfig::default();

        let outcome = prepare_reload(&oikos, &current).unwrap_or_else(|e| panic!("reload: {e}"));

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
    }

    #[test]
    fn preserve_restart_required_values_restores_all_cold_prefixes() {
        let current = AletheiaConfig::default();
        let mut staged = current.clone();
        staged.gateway.port = 9999;
        staged.provider_behavior.non_streaming_timeout_secs = 99;
        staged.sandbox.enabled = !current.sandbox.enabled;
        staged.tools.optional.insert(
            "search".to_owned(),
            crate::config::ExternalToolEntry {
                kind: crate::config::ExternalToolKind::Http,
                endpoint: Some("https://example.com/search".to_owned()),
                command: None,
                args: Vec::new(),
                cwd: None,
                env: std::collections::HashMap::new(),
                description: None,
                method: crate::config::ExternalToolMethod::Post,
                auth: None,
                trust_annotations: false,
            },
        );
        staged.agents.defaults.model_defaults.thinking_budget = 20_000;

        let diff = diff_configs(&current, &staged).unwrap_or_else(|e| panic!("diff configs: {e}"));
        let live = preserve_restart_required_values(&current, &staged, &diff)
            .unwrap_or_else(|e| panic!("preserve cold values: {e}"));

        assert_eq!(live.gateway.port, current.gateway.port);
        assert_eq!(
            live.provider_behavior.non_streaming_timeout_secs,
            current.provider_behavior.non_streaming_timeout_secs
        );
        assert_eq!(live.sandbox.enabled, current.sandbox.enabled);
        assert!(
            live.tools.optional.is_empty(),
            "tools changes should stay staged until restart"
        );
        assert_eq!(
            live.agents.defaults.model_defaults.thinking_budget,
            staged.agents.defaults.model_defaults.thinking_budget
        );
    }

    #[test]
    fn prepare_reload_no_changes_when_config_identical() {
        let jail = EnvJail::new();
        let default_toml = toml::to_string(&AletheiaConfig::default())
            .unwrap_or_else(|e| panic!("serialize default: {e}"));
        jail.create_file("config/aletheia.toml", &default_toml);

        let oikos = Oikos::from_root(jail.directory());
        let current = AletheiaConfig::default();

        let outcome = prepare_reload(&oikos, &current).unwrap_or_else(|e| panic!("reload: {e}"));
        assert!(
            outcome.diff.is_empty(),
            "identical config should produce empty diff"
        );
    }
}
