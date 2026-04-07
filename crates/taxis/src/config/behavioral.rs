//! Behavioral tuning parameters extracted from hardcoded constants.
//!
//! These parameters control runtime behavior: timeouts, thresholds,
//! capacities, and model routing. All have sensible defaults matching
//! the original hardcoded values. Exposed in `aletheia.toml` under
//! `[behavioral]` sections.
//!
//! WHY: Issue #2306 (W-24). Behavioral tuning should not require code
//! changes. Three tiers: `const` (invariants, not here), deployment-tunable
//! (this file), per-agent-tunable (future, via intent system).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Distillation parameters
// ---------------------------------------------------------------------------

/// Controls when and how context distillation triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DistillationConfig {
    /// Token count that triggers distillation.
    pub context_token_trigger: usize,
    /// Message count that triggers distillation.
    pub message_count_trigger: usize,
    /// Days of inactivity before a session is considered stale.
    pub stale_session_days: u32,
    /// Minimum messages in a stale session before distillation.
    pub stale_session_min_messages: u32,
    /// Message count for sessions that have never been distilled.
    pub never_distilled_message_trigger: u32,
    /// Minimum messages before legacy threshold applies.
    pub legacy_threshold_min_messages: u32,
    /// Maximum share of context window used for history (0.0–1.0).
    pub max_history_share: f64,
    /// Number of recent messages to keep verbatim (not distilled).
    pub verbatim_tail: u32,
}

impl Default for DistillationConfig {
    fn default() -> Self {
        Self {
            context_token_trigger: 120_000,
            message_count_trigger: 150,
            stale_session_days: 7,
            stale_session_min_messages: 20,
            never_distilled_message_trigger: 30,
            legacy_threshold_min_messages: 10,
            max_history_share: 0.7,
            verbatim_tail: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Agent health parameters
// ---------------------------------------------------------------------------

/// Controls agent health monitoring and recovery.
///
/// All durations are in seconds for TOML compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AgentHealthConfig {
    /// Interval between health checks (seconds).
    pub health_interval_secs: u64,
    /// Timeout for ping/heartbeat checks (seconds).
    pub ping_timeout_secs: u64,
    /// Maximum restart backoff duration (seconds).
    pub max_restart_backoff_secs: u64,
    /// Timeout for draining active work before restart (seconds).
    pub restart_drain_timeout_secs: u64,
    /// Window for counting restarts before declaring unhealthy (seconds).
    pub restart_decay_window_secs: u64,
    /// Consecutive timeouts before marking unhealthy.
    pub dead_threshold: u32,
    /// Window for tracking degraded state (seconds).
    pub degraded_window_secs: u64,
    /// Inbox receive timeout before declaring idle (seconds).
    pub inbox_recv_timeout_secs: u64,
    /// Default timeout for sending messages to an agent (seconds).
    pub send_timeout_secs: u64,
}

impl Default for AgentHealthConfig {
    fn default() -> Self {
        Self {
            health_interval_secs: 30,
            ping_timeout_secs: 5,
            max_restart_backoff_secs: 300,
            restart_drain_timeout_secs: 30,
            restart_decay_window_secs: 3600,
            dead_threshold: 3,
            degraded_window_secs: 600,
            inbox_recv_timeout_secs: 30,
            send_timeout_secs: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Capacity limits
// ---------------------------------------------------------------------------

/// Controls various capacity and size limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct CapacityConfig {
    /// Maximum number of concurrent agent sessions.
    pub max_sessions: u32,
    /// Maximum spawned background tasks per agent.
    pub max_spawned_tasks: u32,
    /// Agent inbox message capacity.
    pub inbox_capacity: u32,
    /// Maximum task stack depth.
    pub max_task_stack: u32,
    /// Maximum auto-extracted skills per agent.
    pub max_skills: u32,
    /// Maximum corrections tracked per agent.
    pub max_corrections: u32,
    /// Default loop detection window size.
    pub loop_detection_window: u32,
    /// Maximum cycle length for loop detection.
    pub cycle_detection_max_len: u32,
}

impl Default for CapacityConfig {
    fn default() -> Self {
        Self {
            max_sessions: 1_000,
            max_spawned_tasks: 8,
            inbox_capacity: 32,
            max_task_stack: 10,
            max_skills: 5,
            max_corrections: 50,
            loop_detection_window: 50,
            cycle_detection_max_len: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// Retry and backoff
// ---------------------------------------------------------------------------

/// Controls retry and backoff behavior for LLM calls and operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct RetryConfig {
    /// Maximum retry attempts.
    pub max_retries: u32,
    /// Base backoff duration in milliseconds.
    pub backoff_base_ms: u64,
    /// Backoff multiplier (exponential factor).
    pub backoff_factor: u32,
    /// Maximum backoff duration in milliseconds.
    pub backoff_max_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            backoff_base_ms: 1_000,
            backoff_factor: 2,
            backoff_max_ms: 30_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Model routing
// ---------------------------------------------------------------------------

/// Default model identifiers for different task types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ModelRoutingConfig {
    /// Default model for standard operations.
    pub default_model: String,
    /// Model for high-complexity/judgment tasks.
    pub opus_model: String,
    /// Model for lightweight/fast tasks.
    pub haiku_model: String,
    /// Model for distillation operations.
    pub distillation_model: String,
    /// Model for attention/prosoche checks.
    pub prosoche_model: String,
}

impl Default for ModelRoutingConfig {
    fn default() -> Self {
        Self {
            default_model: "claude-sonnet-4-20250514".to_owned(),
            opus_model: "claude-opus-4-20250514".to_owned(),
            haiku_model: "claude-haiku-4-5-20251001".to_owned(),
            distillation_model: "claude-sonnet-4-20250514".to_owned(),
            prosoche_model: "claude-haiku-4-5-20251001".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level behavioral config
// ---------------------------------------------------------------------------

/// All behavioral tuning parameters.
///
/// Exposed in `aletheia.toml` under `[behavioral]`:
/// ```toml
/// [behavioral.distillation]
/// context_token_trigger = 120000
///
/// [behavioral.agent_health]
/// health_interval_secs = 30
///
/// [behavioral.capacity]
/// max_sessions = 1000
///
/// [behavioral.retry]
/// max_retries = 3
///
/// [behavioral.models]
/// default_model = "claude-sonnet-4-20250514"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BehavioralConfig {
    /// Distillation triggers and thresholds.
    #[serde(default)]
    pub distillation: DistillationConfig,
    /// Agent health monitoring parameters.
    #[serde(default)]
    pub agent_health: AgentHealthConfig,
    /// Capacity and size limits.
    #[serde(default)]
    pub capacity: CapacityConfig,
    /// Retry and backoff behavior.
    #[serde(default)]
    pub retry: RetryConfig,
    /// Model routing defaults.
    #[serde(default)]
    pub models: ModelRoutingConfig,
}

impl BehavioralConfig {
    /// Validate all parameters are within sensible bounds.
    ///
    /// Returns a list of validation error messages. Empty = valid.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // Distillation
        if self.distillation.context_token_trigger == 0 {
            errors.push("distillation.context_token_trigger must be > 0".to_owned());
        }
        if self.distillation.message_count_trigger <= 0 {
            errors.push("distillation.message_count_trigger must be > 0".to_owned());
        }
        if !(0.0..=1.0).contains(&self.distillation.max_history_share) {
            errors.push("distillation.max_history_share must be 0.0–1.0".to_owned());
        }
        if self.distillation.verbatim_tail == 0 {
            errors.push("distillation.verbatim_tail must be > 0".to_owned());
        }

        // Agent health
        if self.agent_health.health_interval_secs == 0 {
            errors.push("agent_health.health_interval_secs must be > 0".to_owned());
        }
        if self.agent_health.dead_threshold == 0 {
            errors.push("agent_health.dead_threshold must be > 0".to_owned());
        }

        // Capacity
        if self.capacity.max_sessions == 0 {
            errors.push("capacity.max_sessions must be > 0".to_owned());
        }
        if self.capacity.max_spawned_tasks == 0 {
            errors.push("capacity.max_spawned_tasks must be > 0".to_owned());
        }
        if self.capacity.inbox_capacity == 0 {
            errors.push("capacity.inbox_capacity must be > 0".to_owned());
        }

        // Retry
        if self.retry.max_retries == 0 {
            errors.push("retry.max_retries must be > 0".to_owned());
        }
        if self.retry.backoff_base_ms == 0 {
            errors.push("retry.backoff_base_ms must be > 0".to_owned());
        }
        if self.retry.backoff_max_ms < self.retry.backoff_base_ms {
            errors.push("retry.backoff_max_ms must be >= backoff_base_ms".to_owned());
        }

        // Models
        if self.models.default_model.is_empty() {
            errors.push("models.default_model must not be empty".to_owned());
        }

        errors
    }
}

impl Default for BehavioralConfig {
    fn default() -> Self {
        Self {
            distillation: DistillationConfig::default(),
            agent_health: AgentHealthConfig::default(),
            capacity: CapacityConfig::default(),
            retry: RetryConfig::default(),
            models: ModelRoutingConfig::default(),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_behavioral_config() {
        let config = BehavioralConfig::default();
        assert_eq!(config.distillation.context_token_trigger, 120_000);
        assert_eq!(config.agent_health.dead_threshold, 3);
        assert_eq!(config.capacity.max_sessions, 1_000);
        assert_eq!(config.retry.max_retries, 3);
        assert_eq!(config.models.default_model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn roundtrip_toml() {
        let config = BehavioralConfig::default();
        let toml = toml::to_string_pretty(&config).unwrap();
        let deserialized: BehavioralConfig = toml::from_str(&toml).unwrap();
        assert_eq!(
            deserialized.distillation.context_token_trigger,
            config.distillation.context_token_trigger
        );
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let toml_str = r#"
[distillation]
context_token_trigger = 200000
"#;
        let config: BehavioralConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.distillation.context_token_trigger, 200_000);
        // Other fields use defaults.
        assert_eq!(config.distillation.message_count_trigger, 150);
        assert_eq!(config.capacity.max_sessions, 1_000);
    }
}
