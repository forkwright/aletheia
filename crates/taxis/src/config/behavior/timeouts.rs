//! Deployment-tunable timeout thresholds and capacity limits.

use serde::{Deserialize, Serialize};

/// Deployment-tunable timeout thresholds.
///
/// Controls the wall-clock budget for the operator approval gate. Defaults
/// match the hardcoded constant previously owned by `nous::approval`, so
/// omitting this section from `aletheia.toml` produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct TimeoutsConfig {
    /// Maximum wall-clock seconds a Required/Mandatory tool call waits for an
    /// operator approval decision before defaulting to deny.
    ///
    /// WHY configurable (#5011): approval lifetime is part of the execution
    /// safety contract — it controls how long an irreversible action blocks
    /// the pipeline and what a dropped client connection does. It was
    /// previously an unowned constant in `nous::approval`. Valid range:
    /// 5–3600. Default: 120 (matches the desktop daily-driver UX — long
    /// enough to read the overlay, short enough that a dropped connection
    /// denies rather than hangs).
    pub approval_timeout_secs: u32,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            approval_timeout_secs: 120,
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
/// Controls how Hermeneus HTTP providers retry transient failures. Defaults
/// match `koina::defaults` (also the source hermeneus re-exports its own
/// retry constants from) so that omitting this section produces identical
/// behaviour.
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
        // WHY: values come from koina::defaults (taxis cannot depend on
        // hermeneus under the layering rules; both depend on koina) so that
        // omitting [retry] from aletheia.toml produces identical behaviour
        // to the pre-parameterization defaults.
        Self {
            max_attempts: koina::defaults::DEFAULT_MAX_RETRIES,
            backoff_base_ms: koina::defaults::BACKOFF_BASE_MS,
            backoff_max_ms: koina::defaults::BACKOFF_MAX_MS,
        }
    }
}

/// Deployment-tunable per-stage wall-clock budgets for the nous turn
/// pipeline (aletheia#7296).
///
/// Each field is a maximum seconds a pipeline stage may run before the
/// remaining stages are skipped and a partial result is returned. `0` means
/// no limit for that stage. Defaults match the compile-time constants
/// `nous::config::StageBudget` previously hardcoded, so omitting
/// `[stageBudget]` from `aletheia.toml` produces identical behaviour.
///
/// NOTE: this section does not yet govern ephemeral sub-agent turns; see
/// the `stage_budget` field doc on the config struct that embeds this one
/// (aletheia#7306).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct StageBudgetConfig {
    /// Context-assembly stage limit, in seconds. Default: 10.
    pub context_secs: u32,
    /// Semantic recall stage limit, in seconds. Default: 15.
    ///
    /// Also bounds the query-rewrite and side-query-ranking LLM calls the
    /// recall stage makes: each gets at most half of this budget (minus a
    /// small reserve for the non-LLM search work), floored at 3s. A
    /// query-rewrite call that exceeds its share falls back to the raw,
    /// unrewritten query rather than failing the stage.
    pub recall_secs: u32,
    /// History-retrieval stage limit, in seconds. Default: 5.
    pub history_secs: u32,
    /// Guard-evaluation stage limit, in seconds. Default: 2.
    pub guard_secs: u32,
    /// LLM execution stage limit, in seconds. `0` means unlimited (the
    /// provider controls its own timeout). Default: 0.
    pub execute_secs: u32,
    /// Finalization stage limit, in seconds. Default: 10.
    pub finalize_secs: u32,
    /// Reflection stage limit, in seconds. Default: 30.
    pub reflection_secs: u32,
    /// Hard cap on total pipeline wall-clock time, in seconds. Default: 300.
    pub total_secs: u32,
}

impl Default for StageBudgetConfig {
    fn default() -> Self {
        Self {
            context_secs: 10,
            recall_secs: 15,
            history_secs: 5,
            guard_secs: 2,
            execute_secs: 0,
            finalize_secs: 10,
            reflection_secs: 30,
            total_secs: 300,
        }
    }
}
