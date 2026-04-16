//! Daemon behavior configuration.

use serde::{Deserialize, Serialize};

/// Daemon watchdog, prosoche anomaly detection, and runner output settings.
///
/// All defaults match the current hardcoded constants in the `daemon` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DaemonBehaviorConfig {
    /// Base duration in seconds for watchdog restart backoff. Default: 2.
    /// Mirrors `daemon::watchdog::BACKOFF_BASE`.
    pub watchdog_backoff_base_secs: u64,
    /// Maximum watchdog restart backoff duration in seconds. Default: 300.
    /// Mirrors `daemon::watchdog::BACKOFF_CAP`.
    pub watchdog_backoff_cap_secs: u64,
    /// Samples used for anomaly detection in prosoche attention check. Default: 15.
    /// Mirrors `daemon::prosoche::ANOMALY_SAMPLE_SIZE`.
    pub prosoche_anomaly_sample_size: usize,
    /// Lines from task output head to include in brief summary. Default: 5.
    /// Mirrors `daemon::runner::output::BRIEF_HEAD_LINES`.
    pub runner_output_brief_head_lines: usize,
    /// Lines from task output tail to include in brief summary. Default: 3.
    /// Mirrors `daemon::runner::output::BRIEF_TAIL_LINES`.
    pub runner_output_brief_tail_lines: usize,
}

impl Default for DaemonBehaviorConfig {
    fn default() -> Self {
        Self {
            watchdog_backoff_base_secs: 2,
            watchdog_backoff_cap_secs: 300,
            prosoche_anomaly_sample_size: 15,
            runner_output_brief_head_lines: 5,
            runner_output_brief_tail_lines: 3,
        }
    }
}
