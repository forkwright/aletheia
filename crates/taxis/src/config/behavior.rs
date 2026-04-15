//! Deployment-tunable behavior configuration types.

use serde::{Deserialize, Serialize};

/// Deployment-tunable timeout thresholds.
///
/// Controls wall-clock timeout budgets for LLM and provider calls.
/// Defaults match the hardcoded constants in `koina::defaults` so that
/// omitting this section from `aletheia.toml` produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TimeoutsConfig {
    /// Maximum wall-clock seconds for a single LLM API call (Anthropic or CC provider).
    ///
    /// Requests exceeding this limit are cancelled and may trigger a retry.
    /// Valid range: 30–3600. Default: 300.
    pub llm_call_secs: u32,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            llm_call_secs: koina::defaults::TIMEOUT_SECONDS,
        }
    }
}

/// Deployment-tunable capacity limits for tool output and context windows.
///
/// Controls memory and token budgets that depend on the host's hardware and
/// the LLM provider's context limits. Defaults match `koina::defaults`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CapacityConfig {
    /// Maximum bytes returned by a single tool call before the output is
    /// truncated with an indicator showing the original size.
    ///
    /// Applies to all built-in tools (filesystem, workspace, shell). Set to
    /// `0` to disable truncation. Valid range: 0–10 MiB. Default: 51200 (50 KiB).
    pub max_tool_output_bytes: usize,
    /// Context window token limit applied to Opus-class models when the
    /// global `contextTokens` is still at its default value (200k).
    ///
    /// Opus models support a 1M token context window; this automatic upgrade
    /// preserves that capability without requiring manual per-agent overrides.
    /// Set to the same value as `contextTokens` to disable the auto-upgrade.
    /// Valid range: 200000–2000000. Default: 1000000.
    pub opus_context_tokens: u32,
}

impl Default for CapacityConfig {
    fn default() -> Self {
        Self {
            max_tool_output_bytes: koina::defaults::MAX_OUTPUT_BYTES,
            opus_context_tokens: koina::defaults::OPUS_CONTEXT_TOKENS,
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

/// Nous actor/manager health, restart, GC, and loop-detection thresholds.
///
/// All defaults match the current hardcoded constants in the `nous` crate so
/// that omitting this section from `aletheia.toml` produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct NousBehaviorConfig {
    /// Panics within the window that trigger degraded mode. Default: 5.
    /// Mirrors `nous::actor::DEGRADED_PANIC_THRESHOLD`.
    pub degraded_panic_threshold: u32,
    /// Window in seconds for counting panics toward degraded threshold. Default: 600.
    /// Mirrors `nous::actor::DEGRADED_WINDOW`.
    pub degraded_window_secs: u64,
    /// Actor inbox receive timeout in seconds before a warning is logged. Default: 30.
    /// Mirrors `nous::actor::INBOX_RECV_TIMEOUT`.
    pub inbox_recv_timeout_secs: u64,
    /// Consecutive receive timeouts before a warning log is emitted. Default: 3.
    /// Mirrors `nous::actor::CONSECUTIVE_TIMEOUT_WARN_THRESHOLD`.
    pub consecutive_timeout_warn_threshold: u32,
    /// Actor inbox channel capacity. Default: 32.
    pub inbox_capacity: usize,
    /// Maximum number of concurrently spawned tasks per agent. Default: 8.
    pub max_spawned_tasks: usize,
    /// Maximum number of concurrent sessions across all agents. Default: 1000.
    pub max_sessions: usize,
    /// Completed-task garbage collection interval in seconds. Default: 300.
    /// Mirrors `nous::tasks::gc::DEFAULT_GC_INTERVAL`.
    pub gc_interval_secs: u64,
    /// Consecutive failed pings before marking an agent dead. Default: 3.
    /// Mirrors `nous::manager::DEAD_THRESHOLD`.
    pub manager_dead_threshold: u32,
    /// Cap on exponential restart backoff in seconds. Default: 300.
    /// Mirrors `nous::manager::MAX_RESTART_BACKOFF`.
    pub manager_max_restart_backoff_secs: u64,
    /// Drain timeout in seconds before forcing an agent restart. Default: 30.
    /// Mirrors `nous::manager::RESTART_DRAIN_TIMEOUT`.
    pub manager_restart_drain_timeout_secs: u64,
    /// Window in seconds over which the failure count decays to zero. Default: 3600.
    /// Mirrors `nous::manager::RESTART_DECAY_WINDOW`.
    pub manager_restart_decay_window_secs: u64,
    /// Agent health poll interval in seconds. Default: 30.
    /// Mirrors `nous::manager::DEFAULT_HEALTH_INTERVAL`.
    pub manager_health_interval_secs: u64,
    /// Timeout in seconds for health-ping responses. Default: 5.
    /// Mirrors `nous::manager::DEFAULT_PING_TIMEOUT`.
    pub manager_ping_timeout_secs: u64,
    /// Maximum seconds a turn may be active before the health check considers
    /// the actor stuck. An `active_turn` flag alone cannot distinguish a legitimately
    /// busy actor from one hung on an infinite loop or deadlock. Default: 600 (10 min).
    /// WHY: Without a timeout, a stuck `active_turn` flag prevents the health check
    /// from ever restarting the actor, making a single hung pipeline permanently
    /// block all subsequent messages. (#3254)
    pub stuck_turn_timeout_secs: u64,
    /// Number of recent tool calls scanned for loop detection. Default: 50.
    /// Mirrors `nous::pipeline::DEFAULT_LOOP_WINDOW`.
    pub loop_detection_window: usize,
    /// Maximum sequence length examined for repeating cycles. Default: 10.
    /// Mirrors `nous::pipeline::CYCLE_DETECTION_MAX_LEN`.
    pub cycle_detection_max_len: usize,
    /// Events accumulated before self-audit runs. Default: 50.
    /// Mirrors `nous::self_audit::DEFAULT_EVENT_THRESHOLD`.
    pub self_audit_event_threshold: u32,
}

impl Default for NousBehaviorConfig {
    fn default() -> Self {
        Self {
            degraded_panic_threshold: 5,
            degraded_window_secs: 600,
            inbox_recv_timeout_secs: 30,
            consecutive_timeout_warn_threshold: 3,
            inbox_capacity: 32,
            max_spawned_tasks: 8,
            max_sessions: 1_000,
            gc_interval_secs: 300,
            manager_dead_threshold: 3,
            manager_max_restart_backoff_secs: 300,
            manager_restart_drain_timeout_secs: 30,
            manager_restart_decay_window_secs: 3_600,
            manager_health_interval_secs: 30,
            manager_ping_timeout_secs: 5,
            stuck_turn_timeout_secs: 600,
            loop_detection_window: 50,
            cycle_detection_max_len: 10,
            self_audit_event_threshold: 50,
        }
    }
}

/// Episteme knowledge conflict resolution, decay, and extraction parameters.
///
/// All defaults match the current hardcoded constants in the `episteme` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct KnowledgeConfig {
    /// Maximum LLM calls per fact during conflict resolution. Default: 3.
    /// Mirrors `episteme::conflict::MAX_LLM_CALLS_PER_FACT`.
    pub conflict_max_llm_calls_per_fact: usize,
    /// Similarity threshold above which intra-batch candidates are merged. Default: 0.95.
    /// Mirrors `episteme::conflict::INTRA_BATCH_DEDUP_THRESHOLD`.
    pub conflict_intra_batch_dedup_threshold: f64,
    /// Maximum vector distance for a fact to be a conflict candidate. Default: 0.28.
    /// Mirrors `episteme::conflict::CANDIDATE_DISTANCE_THRESHOLD`.
    pub conflict_candidate_distance_threshold: f64,
    /// Maximum conflict candidates evaluated per fact. Default: 5.
    /// Mirrors `episteme::conflict::MAX_CANDIDATES`.
    pub conflict_max_candidates: usize,
    /// Confidence boost per reinforcement event. Default: 0.02.
    /// Mirrors `episteme::decay::REINFORCEMENT_BOOST`.
    pub decay_reinforcement_boost: f64,
    /// Maximum cumulative reinforcement bonus. Default: 1.0.
    /// Mirrors `episteme::decay::MAX_REINFORCEMENT_BONUS`.
    pub decay_max_reinforcement_bonus: f64,
    /// Confidence bonus per additional corroborating agent. Default: 0.15.
    /// Mirrors `episteme::decay::CROSS_AGENT_BONUS_PER_AGENT`.
    pub decay_cross_agent_bonus_per_agent: f64,
    /// Cap on total cross-agent multiplier. Default: 1.75.
    /// Mirrors `episteme::decay::MAX_CROSS_AGENT_MULTIPLIER`.
    pub decay_max_cross_agent_multiplier: f64,
    /// Minimum confidence for a fact to pass extraction filtering. Default: 0.3.
    pub extraction_confidence_threshold: f64,
    /// Minimum character length for an extracted fact. Default: 10.
    pub extraction_min_fact_length: usize,
    /// Maximum character length for an extracted fact. Default: 500.
    pub extraction_max_fact_length: usize,
    /// Minimum tool calls before operational instinct scoring fires. Default: 5.
    /// Mirrors `episteme::ops_facts::MIN_TOOL_CALLS`.
    pub instinct_min_tool_calls: u64,
    /// Maximum length for parameter values before truncation. Default: 200.
    /// Mirrors `episteme::instinct::MAX_PARAM_VALUE_LEN`.
    pub instinct_max_param_value_len: usize,
    /// Maximum length for context summaries. Default: 100.
    /// Mirrors `episteme::instinct::MAX_CONTEXT_SUMMARY_LEN`.
    pub instinct_max_context_summary_len: usize,
    /// Maximum byte length for fact content strings. Default: 102400 (100 KiB).
    /// Mirrors `eidos::knowledge::fact::MAX_CONTENT_LENGTH`.
    pub max_content_length: usize,
    /// Default maximum entries returned by a single side-query. Default: 5.
    /// Mirrors `episteme::side_query::DEFAULT_MAX_RESULTS`.
    pub side_query_max_results: usize,
    /// Default cache time-to-live in seconds for side-query. Default: 300.
    /// Mirrors `episteme::side_query::DEFAULT_CACHE_TTL_SECS`.
    pub side_query_cache_ttl_secs: u64,
    /// Default maximum cache entries for side-query. Default: 64.
    /// Mirrors `episteme::side_query::DEFAULT_CACHE_CAPACITY`.
    pub side_query_cache_capacity: usize,
    /// Decay score below which a skill is flagged for review. Default: 0.3.
    /// Mirrors `episteme::skill::decay::NEEDS_REVIEW_THRESHOLD`.
    pub skill_decay_needs_review_threshold: f64,
    /// Decay score below which a skill is auto-retired. Default: 0.08.
    /// Mirrors `episteme::skill::decay::RETIRE_THRESHOLD`.
    pub skill_decay_retire_threshold: f64,
    /// Days of inactivity before decay reaches review threshold (low-usage skills). Default: 28.
    /// Mirrors `episteme::skill::decay::DEFAULT_STALE_DAYS`.
    pub skill_decay_stale_days: u32,
    /// Usage count above which a skill decays slower. Default: 10.
    /// Mirrors `episteme::skill::decay::HIGH_USAGE_THRESHOLD`.
    pub skill_decay_high_usage_threshold: u32,
    /// Multiplier applied to decay half-life for high-usage skills. Default: 3.0.
    /// Mirrors `episteme::skill::decay::HIGH_USAGE_DECAY_FACTOR`.
    pub skill_decay_high_usage_factor: f64,
    /// Surprise threshold (nats) for episode boundary detection. Default: 2.0.
    /// Mirrors `episteme::surprise::DEFAULT_THRESHOLD`.
    pub surprise_threshold: f64,
    /// EMA alpha for surprise baseline adaptation. Default: 0.3.
    /// Mirrors `episteme::surprise::DEFAULT_EMA_ALPHA`.
    pub surprise_ema_alpha: f64,
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            conflict_max_llm_calls_per_fact: 3,
            conflict_intra_batch_dedup_threshold: 0.95,
            conflict_candidate_distance_threshold: 0.28,
            conflict_max_candidates: 5,
            decay_reinforcement_boost: 0.02,
            decay_max_reinforcement_bonus: 1.0,
            decay_cross_agent_bonus_per_agent: 0.15,
            decay_max_cross_agent_multiplier: 1.75,
            max_content_length: 102_400,
            extraction_confidence_threshold: 0.3,
            extraction_min_fact_length: 10,
            extraction_max_fact_length: 500,
            instinct_min_tool_calls: 5,
            instinct_max_param_value_len: 200,
            instinct_max_context_summary_len: 100,
            side_query_max_results: 5,
            side_query_cache_ttl_secs: 300,
            side_query_cache_capacity: 64,
            skill_decay_needs_review_threshold: 0.3,
            skill_decay_retire_threshold: 0.08,
            skill_decay_stale_days: 28,
            skill_decay_high_usage_threshold: 10,
            skill_decay_high_usage_factor: 3.0,
            surprise_threshold: 2.0,
            surprise_ema_alpha: 0.3,
        }
    }
}

/// Hermeneus provider timeout, concurrency, and complexity routing thresholds.
///
/// All defaults match the current hardcoded constants in the `hermeneus` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ProviderBehaviorConfig {
    /// Timeout in seconds for non-streaming LLM requests. Default: 120.
    /// Mirrors `hermeneus::anthropic::client::NON_STREAMING_TIMEOUT`.
    pub non_streaming_timeout_secs: u64,
    /// Default retry delay from SSE stream retry field in milliseconds. Default: 1000.
    /// Mirrors `hermeneus::anthropic::error::SSE_DEFAULT_RETRY_MS`.
    pub sse_default_retry_ms: u64,
    /// EWMA smoothing factor for adaptive concurrency limiter. Default: 0.8.
    /// Mirrors `hermeneus::concurrency::DEFAULT_EWMA_ALPHA`.
    pub concurrency_ewma_alpha: f64,
    /// Latency threshold in seconds above which concurrency limit is reduced. Default: 30.0.
    /// Mirrors `hermeneus::concurrency::DEFAULT_LATENCY_THRESHOLD_SECS`.
    pub concurrency_latency_threshold_secs: f64,
    /// Complexity score below which Haiku-class model is selected. Default: 30.
    /// Mirrors `hermeneus::complexity::DEFAULT_LOW_THRESHOLD`.
    pub complexity_low_threshold: u32,
    /// Complexity score above which Opus-class model is selected. Default: 70.
    /// Mirrors `hermeneus::complexity::DEFAULT_HIGH_THRESHOLD`.
    pub complexity_high_threshold: u32,
}

impl Default for ProviderBehaviorConfig {
    fn default() -> Self {
        Self {
            non_streaming_timeout_secs: 120,
            sse_default_retry_ms: 1_000,
            concurrency_ewma_alpha: 0.8,
            concurrency_latency_threshold_secs: 30.0,
            complexity_low_threshold: 30,
            complexity_high_threshold: 70,
        }
    }
}

/// Anthropic-specific sovereignty and privacy settings.
///
/// Mirrors the operator-facing controls at the hermeneus (Anthropic client)
/// boundary. Defaults are sovereignty-first: nothing is cached on Anthropic
/// servers unless the operator explicitly opts in.
///
/// Issues: #3406, #3410, #3409.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AnthropicConfig {
    /// Prompt cache policy (#3410).
    ///
    /// Controls whether outgoing requests carry `cache_control` markers that
    /// let Anthropic store operator system prompts, tool definitions, and
    /// recent conversation turns on their side for reuse. `"disabled"` (the
    /// default) strips every marker so operator content never enters the
    /// Anthropic prompt cache; `"ephemeral"` opts in to the standard 5-minute
    /// cache; `"extended"` reserves the slot for the 1-hour cache wire format
    /// and currently behaves the same as `"ephemeral"`.
    ///
    /// Tradeoff: enabling caching lowers per-turn token spend at the cost of
    /// storing the operator's system prompt on Anthropic infrastructure for
    /// the cache lifetime.
    pub prompt_cache_mode: PromptCacheMode,
}

/// Prompt cache policy for the Anthropic provider.
///
/// Mirrors `hermeneus::provider::PromptCacheMode` so the taxis config layer
/// does not depend on hermeneus; the runtime wiring in `crates/aletheia`
/// converts between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PromptCacheMode {
    /// No `cache_control` markers emitted — operator content never enters
    /// Anthropic's prompt cache. Sovereignty default.
    #[default]
    Disabled,
    /// Standard 5-minute ephemeral cache.
    Ephemeral,
    /// Extended 1-hour cache (reserved; behaves like `Ephemeral` until the
    /// wire format for extended TTL is plumbed through).
    Extended,
}

/// Pylon API request size and idempotency cache limits.
///
/// All defaults match the current hardcoded constants in the `pylon` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ApiLimitsConfig {
    /// Maximum characters in a session name. Default: 255.
    /// Mirrors `pylon::handlers::sessions::MAX_SESSION_NAME_LEN`.
    pub max_session_name_len: usize,
    /// Maximum bytes in a session identifier. Default: 256.
    /// Mirrors `pylon::handlers::sessions::MAX_IDENTIFIER_BYTES`.
    pub max_identifier_bytes: usize,
    /// Maximum messages returned by the history endpoint. Default: 1000.
    /// Mirrors `pylon::handlers::sessions::MAX_HISTORY_LIMIT`.
    pub max_history_limit: u32,
    /// Default messages returned by the history endpoint. Default: 50.
    /// Mirrors `pylon::handlers::sessions::DEFAULT_HISTORY_LIMIT`.
    pub default_history_limit: u32,
    /// Maximum bytes per streaming message body. Default: 262144 (256 KiB).
    /// Mirrors `pylon::handlers::sessions::streaming::MAX_MESSAGE_BYTES`.
    pub max_message_bytes: usize,
    /// Maximum facts returned by a single knowledge list request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::MAX_FACTS_LIMIT`.
    pub max_facts_limit: usize,
    /// Maximum results for a single knowledge search request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::MAX_SEARCH_LIMIT`.
    pub max_search_limit: usize,
    /// Maximum facts in a single bulk-import request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::bulk_import::MAX_IMPORT_BATCH_SIZE`.
    pub max_import_batch_size: usize,
    /// TTL in seconds for idempotency key cache entries. Default: 300.
    /// Mirrors `pylon::idempotency::DEFAULT_TTL`.
    pub idempotency_ttl_secs: u64,
    /// Maximum idempotency cache entries (LRU cap). Default: 10000.
    /// Mirrors `pylon::idempotency::DEFAULT_CAPACITY`.
    pub idempotency_capacity: usize,
    /// Maximum character length of an idempotency key. Default: 64.
    pub idempotency_max_key_length: usize,
    /// Acceptable clock skew in seconds before token expiry check warns. Default: 30.
    /// Mirrors `pylon::handlers::health::CLOCK_SKEW_LEEWAY`.
    pub clock_skew_leeway_secs: u64,
    /// Time in seconds before token expiry that triggers a warning. Default: 3600.
    /// Mirrors `pylon::handlers::health::EXPIRY_WARNING_THRESHOLD`.
    pub expiry_warning_threshold_secs: u64,
}

impl Default for ApiLimitsConfig {
    fn default() -> Self {
        Self {
            max_session_name_len: 255,
            max_identifier_bytes: 256,
            max_history_limit: 1_000,
            default_history_limit: 50,
            max_message_bytes: 262_144,
            max_facts_limit: 1_000,
            max_search_limit: 1_000,
            max_import_batch_size: 1_000,
            idempotency_ttl_secs: 300,
            idempotency_capacity: 10_000,
            idempotency_max_key_length: 64,
            clock_skew_leeway_secs: 30,
            expiry_warning_threshold_secs: 3_600,
        }
    }
}

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

/// Organon tool size, timeout, and length limits.
///
/// All defaults match the current hardcoded constants in the `organon` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ToolLimitsConfig {
    /// Maximum character length for glob patterns. Default: 1000.
    /// Mirrors `organon::builtins::filesystem::MAX_PATTERN_LENGTH`.
    pub max_pattern_length: usize,
    /// Timeout in seconds for filesystem subprocess commands. Default: 60.
    /// Mirrors `organon::builtins::filesystem::SUBPROCESS_TIMEOUT`.
    pub subprocess_timeout_secs: u64,
    /// Maximum bytes per workspace write operation. Default: 10485760 (10 MiB).
    /// Mirrors `organon::builtins::workspace::MAX_WRITE_BYTES`.
    pub max_write_bytes: usize,
    /// Maximum bytes per workspace read operation. Default: 52428800 (50 MiB).
    /// Mirrors `organon::builtins::workspace::MAX_READ_BYTES`.
    pub max_read_bytes: u64,
    /// Maximum character length of a shell command. Default: 10000.
    /// Mirrors `organon::builtins::workspace::MAX_COMMAND_LENGTH`.
    pub max_command_length: usize,
    /// Maximum characters per intra-session message. Default: 4000.
    /// Mirrors `organon::builtins::communication::MESSAGE_MAX_LEN`.
    pub message_max_len: usize,
    /// Maximum characters per inter-session message. Default: 100000.
    /// Mirrors `organon::builtins::communication::INTER_SESSION_MAX_MESSAGE_LEN`.
    pub inter_session_max_message_len: usize,
    /// Maximum wait timeout in seconds for inter-session messages. Default: 300.
    /// Mirrors `organon::builtins::communication::INTER_SESSION_MAX_TIMEOUT_SECS`.
    pub inter_session_max_timeout_secs: u64,
    /// Maximum concurrent agent-dispatch tasks. Default: 10.
    /// Also present in `AgentBehaviorDefaults::tool_agent_dispatch_max_tasks`.
    pub max_dispatch_tasks: usize,
    /// Default timeout in seconds for spawned sub-agents. Default: 300.
    pub agent_dispatch_timeout_secs: u64,
    /// Default row limit for Datalog memory queries. Default: 100.
    /// Also present in `AgentBehaviorDefaults::tool_datalog_default_row_limit`.
    pub datalog_default_row_limit: usize,
    /// Default query timeout in seconds for the Datalog memory tool. Default: 5.0.
    /// Also present in `AgentBehaviorDefaults::tool_datalog_default_timeout_secs`.
    pub datalog_default_timeout_secs: f64,
    /// Maximum image file size in bytes for the view-file tool. Default: 20971520 (20 MiB).
    /// Also present in `AgentBehaviorDefaults::tool_max_image_bytes`.
    pub max_image_bytes: u64,
    /// Maximum PDF file size in bytes for the view-file tool. Default: 33554432 (32 MiB).
    /// Also present in `AgentBehaviorDefaults::tool_max_pdf_bytes`.
    pub max_pdf_bytes: u64,
}

impl Default for ToolLimitsConfig {
    fn default() -> Self {
        Self {
            max_pattern_length: 1_000,
            subprocess_timeout_secs: 60,
            max_write_bytes: 10_485_760,
            max_read_bytes: 52_428_800,
            max_command_length: 10_000,
            message_max_len: 4_000,
            inter_session_max_message_len: 100_000,
            inter_session_max_timeout_secs: 300,
            max_dispatch_tasks: 10,
            agent_dispatch_timeout_secs: 300,
            datalog_default_row_limit: 100,
            datalog_default_timeout_secs: 5.0,
            max_image_bytes: 20_971_520,
            max_pdf_bytes: 33_554_432,
        }
    }
}

/// Agora messaging transport poll, buffer, circuit-breaker, and RPC settings.
///
/// All defaults match the current hardcoded constants in the `agora` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct MessagingConfig {
    /// How often Semeion polls for new channel messages in milliseconds. Default: 2000.
    /// Mirrors `agora::semeion::DEFAULT_POLL_INTERVAL`.
    pub poll_interval_ms: u64,
    /// Inbound message buffer size per channel. Default: 100.
    /// Mirrors `agora::semeion::DEFAULT_BUFFER_CAPACITY`.
    pub buffer_capacity: usize,
    /// Consecutive channel errors before the channel is halted. Default: 5.
    /// Mirrors `agora::semeion::CIRCUIT_BREAKER_THRESHOLD`.
    pub circuit_breaker_threshold: u32,
    /// How often a halted channel is health-checked in seconds. Default: 60.
    /// Mirrors `agora::semeion::HALTED_HEALTH_CHECK_INTERVAL`.
    pub halted_health_check_interval_secs: u64,
    /// Timeout in seconds for Semeion RPC calls. Default: 10.
    /// Mirrors `agora::semeion::client::RPC_TIMEOUT`.
    pub rpc_timeout_secs: u64,
    /// Timeout in seconds for Semeion health-check requests. Default: 2.
    /// Mirrors `agora::semeion::client::HEALTH_TIMEOUT`.
    pub health_timeout_secs: u64,
    /// Timeout in seconds waiting to receive a Semeion response. Default: 15.
    /// Mirrors `agora::semeion::client::RECEIVE_TIMEOUT`.
    pub receive_timeout_secs: u64,
    /// Default timeout in seconds for agent-dispatch tool calls. Default: 300.
    /// Mirrors `organon::builtins::agent::DEFAULT_TIMEOUT_SECS`.
    pub agent_dispatch_timeout_secs: u64,
    /// Maximum concurrent inbound-message handler tasks. Default: 64.
    /// Mirrors `agora::listener::ChannelListener::MAX_CONCURRENT_HANDLERS`.
    pub max_concurrent_handlers: usize,
}

impl Default for MessagingConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 2_000,
            buffer_capacity: 100,
            circuit_breaker_threshold: 5,
            halted_health_check_interval_secs: 60,
            rpc_timeout_secs: 10,
            health_timeout_secs: 2,
            receive_timeout_secs: 15,
            agent_dispatch_timeout_secs: 300,
            max_concurrent_handlers: 64,
        }
    }
}

/// Self-tuning feedback loop configuration.
///
/// Controls whether agents may propose parameter changes and the evidence
/// thresholds required before a proposal is accepted. The global `enabled`
/// flag is a kill switch; individual agents may opt out via
/// `AgentBehaviorDefaults::tuning_eligible`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TuningConfig {
    /// Global kill switch for self-tuning. Default: false.
    ///
    /// When false, no tuning proposals are generated or applied regardless
    /// of per-agent settings.
    pub enabled: bool,
    /// Maximum parameter changes applied per prosoche cycle. Default: 3.
    ///
    /// Limits the blast radius of a single tuning cycle. Additional proposals
    /// beyond this limit are deferred to the next cycle.
    pub max_changes_per_cycle: u32,
    /// Minimum metric observations required before a proposal is generated. Default: 20.
    ///
    /// Below this threshold, evidence is considered insufficient and the
    /// proposal is rejected.
    pub evidence_min_samples: u32,
    /// Significance threshold in standard deviations. Default: 1.5.
    ///
    /// The observed delta must exceed `significance_threshold * stddev` for
    /// the evidence to be considered statistically significant.
    pub significance_threshold: f64,
}

impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_changes_per_cycle: 3,
            evidence_min_samples: 20,
            significance_threshold: 1.5,
        }
    }
}
