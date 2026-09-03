//! Stuck-detection threshold configuration.
//!
//! WHY(#6750): the pattern-matching `StuckDetector` machinery that consumed
//! this configuration was removed with the unwired `PlanningRuntime`; the
//! `StuckConfig` type stays because `taxis::config::AgentBehaviorDefaults`
//! derives its `planning_stuck_*` defaults from `StuckConfig::default()`.

use serde::{Deserialize, Serialize};

/// Configuration for stuck detection thresholds.
///
/// This type owns the default thresholds:
/// `taxis::config::AgentBehaviorDefaults::planning_stuck_*` derives its values
/// from `StuckConfig::default()` rather than restating them. Callers should
/// construct from the resolved taxis config rather than relying on `Default`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StuckConfig {
    /// How many identical consecutive errors trigger detection.
    pub repeated_error_threshold: u32,
    /// How many identical consecutive tool+args calls trigger detection.
    pub same_args_threshold: u32,
    /// How many alternating failure cycles trigger detection.
    pub alternating_threshold: u32,
    /// How many scattered retries with the same error trigger detection.
    pub escalating_retry_threshold: u32,
    /// Maximum number of invocations retained in the sliding window.
    pub history_window: usize,
}

impl Default for StuckConfig {
    fn default() -> Self {
        Self {
            repeated_error_threshold: 3,
            same_args_threshold: 3,
            alternating_threshold: 3,
            escalating_retry_threshold: 3,
            history_window: 20,
        }
    }
}
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = StuckConfig::default();
        assert_eq!(config.repeated_error_threshold, 3);
        assert_eq!(config.same_args_threshold, 3);
        assert_eq!(config.alternating_threshold, 3);
        assert_eq!(config.escalating_retry_threshold, 3);
        assert_eq!(config.history_window, 20);
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = StuckConfig {
            repeated_error_threshold: 5,
            same_args_threshold: 4,
            alternating_threshold: 6,
            escalating_retry_threshold: 7,
            history_window: 50,
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: StuckConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.repeated_error_threshold, 5);
        assert_eq!(back.same_args_threshold, 4);
        assert_eq!(back.alternating_threshold, 6);
        assert_eq!(back.escalating_retry_threshold, 7);
        assert_eq!(back.history_window, 50);
    }
}
