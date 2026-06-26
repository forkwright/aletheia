//! Daemon behavior configuration.

use serde::{Deserialize, Serialize};
/// Default value used for `DaemonBehaviorConfig::watchdog_backoff_base_secs`.
pub(crate) const DEFAULT_WATCHDOG_BACKOFF_BASE_SECS: u64 = 2;
/// Default value used for `DaemonBehaviorConfig::watchdog_backoff_cap_secs`.
pub(crate) const DEFAULT_WATCHDOG_BACKOFF_CAP_SECS: u64 = 300;

/// Daemon watchdog, prosoche anomaly detection, and runner output settings.
///
/// Defaults for the fields that mirror `daemon` constants are enforced at
/// test-build time by `const _: () = assert!` guards below.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct DaemonBehaviorConfig {
    /// Base duration in seconds for watchdog restart backoff.
    pub watchdog_backoff_base_secs: u64,
    /// Maximum watchdog restart backoff duration in seconds.
    pub watchdog_backoff_cap_secs: u64,
    /// Samples used for anomaly detection in prosoche attention check. Default: 15.
    pub prosoche_anomaly_sample_size: usize,
    /// Lines from task output head to include in brief summary. Default: 5.
    pub runner_output_brief_head_lines: usize,
    /// Lines from task output tail to include in brief summary. Default: 3.
    pub runner_output_brief_tail_lines: usize,
}

impl Default for DaemonBehaviorConfig {
    fn default() -> Self {
        Self {
            watchdog_backoff_base_secs: DEFAULT_WATCHDOG_BACKOFF_BASE_SECS,
            watchdog_backoff_cap_secs: DEFAULT_WATCHDOG_BACKOFF_CAP_SECS,
            prosoche_anomaly_sample_size: 15,
            runner_output_brief_head_lines: 5,
            runner_output_brief_tail_lines: 3,
        }
    }
}

#[cfg(test)]
const _: () =
    assert!(DEFAULT_WATCHDOG_BACKOFF_BASE_SECS == oikonomos::watchdog::BACKOFF_BASE.as_secs());
#[cfg(test)]
const _: () =
    assert!(DEFAULT_WATCHDOG_BACKOFF_CAP_SECS == oikonomos::watchdog::BACKOFF_CAP.as_secs());
