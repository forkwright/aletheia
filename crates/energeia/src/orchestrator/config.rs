// WHY: Centralizes orchestrator-level defaults (concurrency, budget, timeouts)
// separate from per-session config (EngineConfig) so callers can tune the
// dispatch pipeline without touching session internals.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Configuration for the dispatch orchestrator.
///
/// Controls concurrency limits, budget defaults, and timeouts that apply to
/// the entire dispatch run rather than individual sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "OrchestratorConfigRaw")]
#[non_exhaustive]
pub struct OrchestratorConfig {
    /// Maximum number of sessions executing concurrently within a group.
    /// Defaults to 4.
    pub max_concurrent: u32,
    /// Default cost budget in USD for the entire dispatch.
    /// `None` means no cost limit.
    pub default_budget_usd: Option<f64>,
    /// Default turn budget across all sessions in a dispatch.
    /// `None` means no turn limit.
    pub default_budget_turns: Option<u32>,
    /// Maximum wall-clock duration for the entire dispatch.
    /// `None` means no time limit.
    #[serde(with = "duration_ms_option")]
    pub max_duration: Option<Duration>,
    /// Idle timeout per session (no events within this window triggers timeout).
    /// `None` disables idle timeout detection.
    #[serde(with = "duration_ms_option")]
    pub session_idle_timeout: Option<Duration>,
    /// Maximum number of corrective prompt retries per failed prompt.
    /// Defaults to 1.
    pub max_corrective_retries: u32,
}

/// Raw deserialization type for [`OrchestratorConfig`].
#[derive(Debug, Clone, Deserialize)]
struct OrchestratorConfigRaw {
    max_concurrent: u32,
    default_budget_usd: Option<f64>,
    default_budget_turns: Option<u32>,
    #[serde(with = "duration_ms_option")]
    max_duration: Option<Duration>,
    #[serde(with = "duration_ms_option")]
    session_idle_timeout: Option<Duration>,
    max_corrective_retries: u32,
}

impl From<OrchestratorConfigRaw> for OrchestratorConfig {
    fn from(raw: OrchestratorConfigRaw) -> Self {
        Self {
            max_concurrent: raw.max_concurrent,
            default_budget_usd: raw.default_budget_usd,
            default_budget_turns: raw.default_budget_turns,
            max_duration: raw.max_duration,
            session_idle_timeout: raw.session_idle_timeout,
            max_corrective_retries: raw.max_corrective_retries,
        }
    }
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            default_budget_usd: None,
            default_budget_turns: None,
            max_duration: None,
            session_idle_timeout: Some(Duration::from_mins(10)),
            max_corrective_retries: 1,
        }
    }
}

impl OrchestratorConfig {
    /// Create a config with all defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum concurrent sessions per group.
    #[must_use]
    pub fn max_concurrent(mut self, n: u32) -> Self {
        self.max_concurrent = n;
        self
    }

    /// Set the default cost budget for the dispatch.
    #[must_use]
    pub fn default_budget_usd(mut self, usd: f64) -> Self {
        self.default_budget_usd = Some(usd);
        self
    }

    /// Set the default turn budget for the dispatch.
    #[must_use]
    pub fn default_budget_turns(mut self, turns: u32) -> Self {
        self.default_budget_turns = Some(turns);
        self
    }

    /// Set the maximum wall-clock duration for the dispatch.
    #[must_use]
    pub fn max_duration(mut self, duration: Duration) -> Self {
        self.max_duration = Some(duration);
        self
    }

    /// Set the idle timeout per session.
    #[must_use]
    pub fn session_idle_timeout(mut self, timeout: Duration) -> Self {
        self.session_idle_timeout = Some(timeout);
        self
    }

    /// Set the maximum corrective prompt retries per failed prompt.
    #[must_use]
    pub fn max_corrective_retries(mut self, n: u32) -> Self {
        self.max_corrective_retries = n;
        self
    }
}

/// Serde helper for `Option<Duration>` as milliseconds.
mod duration_ms_option {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    #[expect(
        clippy::ref_option,
        reason = "serde(with) requires &T signature for the field type"
    )]
    pub(crate) fn serialize<S: Serializer>(
        val: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match val {
            Some(d) => serializer.serialize_some(&d.as_millis()),
            None => serializer.serialize_none(),
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        let ms: Option<u64> = Option::deserialize(deserializer)?;
        Ok(ms.map(Duration::from_millis))
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.max_concurrent, 4);
        assert!(config.default_budget_usd.is_none());
        assert!(config.default_budget_turns.is_none());
        assert!(config.max_duration.is_none());
        assert_eq!(config.session_idle_timeout, Some(Duration::from_secs(600)));
        assert_eq!(config.max_corrective_retries, 1);
    }

    #[test]
    fn builder_methods() {
        let config = OrchestratorConfig::new()
            .max_concurrent(8)
            .default_budget_usd(25.0)
            .default_budget_turns(500)
            .max_duration(Duration::from_secs(3600))
            .session_idle_timeout(Duration::from_secs(300))
            .max_corrective_retries(2);

        assert_eq!(config.max_concurrent, 8);
        assert_eq!(config.default_budget_usd, Some(25.0));
        assert_eq!(config.default_budget_turns, Some(500));
        assert_eq!(config.max_duration, Some(Duration::from_secs(3600)));
        assert_eq!(config.session_idle_timeout, Some(Duration::from_secs(300)));
        assert_eq!(config.max_corrective_retries, 2);
    }

    #[test]
    fn roundtrip_serialization() {
        let config = OrchestratorConfig::new()
            .max_concurrent(6)
            .default_budget_usd(10.0)
            .max_duration(Duration::from_secs(1800));

        let json = serde_json::to_string(&config).expect("serialize");
        let back: OrchestratorConfig = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.max_concurrent, 6);
        assert_eq!(back.default_budget_usd, Some(10.0));
        assert_eq!(back.max_duration, Some(Duration::from_secs(1800)));
    }

    #[test]
    fn roundtrip_with_none_durations() {
        let config = OrchestratorConfig {
            session_idle_timeout: None,
            max_duration: None,
            ..Default::default()
        };

        let json = serde_json::to_string(&config).expect("serialize");
        let back: OrchestratorConfig = serde_json::from_str(&json).expect("deserialize");

        assert!(back.session_idle_timeout.is_none());
        assert!(back.max_duration.is_none());
    }
}
