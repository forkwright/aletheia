//! Deployment-tunable timeout thresholds and capacity limits.

use serde::{Deserialize, Serialize};

/// Default seconds a tool approval waits for an operator decision.
pub const DEFAULT_APPROVAL_TIMEOUT_SECS: u32 = 120;

/// Minimum seconds a tool approval may wait for an operator decision.
pub const MIN_APPROVAL_TIMEOUT_SECS: u32 = 1;

/// Maximum seconds a tool approval may wait for an operator decision.
pub const MAX_APPROVAL_TIMEOUT_SECS: u32 = 3600;

/// Deployment-tunable timeout thresholds.
///
/// Controls wall-clock timeout budgets for LLM, provider, and approval calls.
/// Defaults match the hardcoded constants in `koina::defaults` so that
/// omitting this section from `aletheia.toml` produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct TimeoutsConfig {
    /// Maximum wall-clock seconds for a single LLM API call (Anthropic or CC provider).
    ///
    /// Requests exceeding this limit are cancelled and may trigger a retry.
    /// Valid range: 30–3600. Default: 300.
    pub llm_call_secs: u32,
    /// Maximum wall-clock seconds to wait for an operator tool-approval decision.
    ///
    /// Required/Mandatory tool calls default-deny when this lifetime elapses
    /// or the approval channel disconnects. Valid range: 1–3600. Default: 120.
    pub approval_secs: u32,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            llm_call_secs: koina::defaults::TIMEOUT_SECONDS,
            approval_secs: DEFAULT_APPROVAL_TIMEOUT_SECS,
        }
    }
}

/// Deployment-tunable capacity limits for tool output.
///
/// Controls memory budgets that depend on the host's hardware. Defaults match
/// `koina::defaults`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct CapacityConfig {
    /// Maximum bytes returned by a single tool call before the output is
    /// truncated with an indicator showing the original size.
    ///
    /// Applies to all built-in tools (filesystem, workspace, shell). Set to
    /// `0` to disable truncation. Valid range: 0–10 MiB. Default: 51200 (50 KiB).
    pub max_tool_output_bytes: usize,
}

impl Default for CapacityConfig {
    fn default() -> Self {
        Self {
            max_tool_output_bytes: koina::defaults::MAX_OUTPUT_BYTES,
        }
    }
}

/// Deployment-tunable LLM retry and backoff parameters.
///
/// Controls how the Anthropic provider retries transient failures. Defaults
/// match the constants in `hermeneus::models` so that omitting this section
/// produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct RetrySettings {
    /// Maximum number of retry attempts after an initial transient failure.
    ///
    /// The total number of LLM calls is `max_attempts + 1`. Set to `0` to
    /// disable retries. Valid range: 0–10. Default: 3.
    pub max_attempts: u32,
    /// Initial exponential backoff delay in milliseconds.
    ///
    /// Each successive retry doubles this delay until `backoff_max_ms` is
    /// reached. Valid range: 100–30000. Default: 1000.
    pub backoff_base_ms: u64,
    /// Maximum backoff delay cap in milliseconds.
    ///
    /// No retry will wait longer than this value regardless of how many
    /// attempts have failed. Valid range: `backoff_base_ms`–300000. Default: 30000.
    pub backoff_max_ms: u64,
}

impl Default for RetrySettings {
    fn default() -> Self {
        // WHY: values mirror hermeneus::models::{DEFAULT_MAX_RETRIES, BACKOFF_BASE_MS,
        // BACKOFF_MAX_MS} so that omitting [retry] from aletheia.toml produces
        // identical behaviour to the pre-parameterization defaults.
        Self {
            max_attempts: 3,
            backoff_base_ms: 1_000,
            backoff_max_ms: 30_000,
        }
    }
}
