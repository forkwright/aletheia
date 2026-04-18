//! Dispatch configuration for recurring cron tasks.

use serde::{Deserialize, Serialize};

/// Dispatch configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
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
    /// What to dispatch.
    pub dispatch_spec: DispatchSpecConfig,
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
}
