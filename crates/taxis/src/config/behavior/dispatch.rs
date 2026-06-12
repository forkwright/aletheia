//! Dispatch configuration for recurring cron tasks.

use serde::{Deserialize, Serialize};

/// Dispatch configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct DispatchConfig {
    /// Recurring cron-dispatched tasks.
    pub cron_tasks: Vec<CronTaskConfig>,
}

/// Configuration for a single cron-dispatched task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct CronTaskConfig {
    /// Unique task name.
    pub name: String,
    /// Cron expression (e.g., "0 2 * * *").
    pub schedule: String,
    /// Jitter in seconds (+/-).
    pub jitter_secs: u64,
    /// Whether this task is registered with the scheduler. Defaults to `true`
    /// so that defining a task in config implies the operator wants it run;
    /// set `enabled = false` to leave the task in the config without firing.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// What to dispatch.
    pub dispatch_spec: DispatchSpecConfig,
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn cron_task_enabled_defaults_to_true_when_omitted() {
        let toml = r#"
name = "nightly"
schedule = "0 2 * * *"
jitterSecs = 0

[dispatchSpec]
promptNumbers = [1, 2]
project = "aletheia"
budgetUsd = 4.25
"#;
        let parsed: CronTaskConfig = toml::from_str(toml).expect("valid cron task config");
        assert!(
            parsed.enabled,
            "omitted `enabled` should default to true so configured tasks actually run"
        );
        assert_eq!(parsed.dispatch_spec.budget_usd, Some(4.25));
    }

    #[test]
    fn cron_task_disabled_when_enabled_false() {
        let toml = r#"
name = "off"
schedule = "0 2 * * *"
jitterSecs = 0
enabled = false

[dispatchSpec]
promptNumbers = []
project = "aletheia"
"#;
        let parsed: CronTaskConfig = toml::from_str(toml).expect("valid cron task config");
        assert!(
            !parsed.enabled,
            "`enabled = false` should round-trip from config"
        );
    }
}

/// Raw dispatch spec for config deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct DispatchSpecConfig {
    /// Prompt numbers to execute.
    pub prompt_numbers: Vec<u32>,
    /// Project identifier.
    pub project: String,
    /// Optional DAG reference.
    #[serde(default)]
    pub dag_ref: Option<String>,
    /// Maximum parallelism.
    #[serde(default)]
    pub max_parallel: Option<u32>,
    /// Maximum turns per initial session.
    #[serde(default)]
    pub max_turns: Option<u32>,
    /// Maximum total dispatch cost in USD.
    #[serde(default)]
    pub budget_usd: Option<f64>,
}
