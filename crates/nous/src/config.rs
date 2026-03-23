//! Nous agent configuration.

use serde::{Deserialize, Serialize};

use crate::recall::RecallConfig;

/// LLM generation settings for a nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NousGenerationConfig {
    /// Default model for this agent.
    pub model: String,
    /// Maximum context window tokens.
    pub context_window: u32,
    /// Maximum output tokens per turn.
    pub max_output_tokens: u32,
    /// Maximum tokens allocated to bootstrap context.
    pub bootstrap_max_tokens: u32,
    /// Whether extended thinking is enabled.
    pub thinking_enabled: bool,
    /// Token budget for extended thinking.
    pub thinking_budget: u32,
    /// Characters per token for conservative token-budget estimation.
    pub chars_per_token: u32,
    /// Model to use for prosoche heartbeat sessions instead of the primary model.
    pub prosoche_model: String,
}

impl Default for NousGenerationConfig {
    fn default() -> Self {
        use aletheia_koina::defaults as d;
        Self {
            model: "claude-opus-4-20250514".to_owned(),
            context_window: d::CONTEXT_TOKENS,
            max_output_tokens: d::MAX_OUTPUT_TOKENS,
            bootstrap_max_tokens: d::BOOTSTRAP_MAX_TOKENS,
            thinking_enabled: false,
            thinking_budget: 10_000,
            chars_per_token: default_chars_per_token(),
            prosoche_model: default_prosoche_model(),
        }
    }
}

/// Resource and safety limits for a nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NousLimits {
    /// Maximum tool execution iterations per turn.
    pub max_tool_iterations: u32,
    /// Loop detection threshold (identical tool calls before detection).
    pub loop_detection_threshold: u32,
    /// Consecutive error threshold (same-tool errors before detection).
    pub consecutive_error_threshold: u32,
    /// Maximum loop warnings before escalating to halt.
    pub loop_max_warnings: u32,
    /// Maximum cumulative tokens (input + output) allowed per session.
    ///
    /// Once a session exceeds this budget the guard stage rejects further
    /// turns with a `GuardResult::Rejected` response. Set to `0` to disable
    /// the cap (default: 500,000).
    pub session_token_cap: u64,
    /// Maximum size in bytes for a single tool result before truncation.
    ///
    /// Results exceeding this limit are truncated with an indicator showing
    /// the original and truncated sizes. Set to `0` to disable. Default:
    /// 32 768 bytes (32 KB).
    pub max_tool_result_bytes: u32,
    /// Maximum consecutive LLM iterations that produce only tool calls
    /// without any reasoning text before a think-first prompt is injected.
    ///
    /// WHY: Without this limit, agents can fire long bursts of tool calls
    /// before producing any reasoning, wasting tokens and obscuring intent.
    /// When the limit is hit, a system message is injected asking the agent
    /// to explain its reasoning before making more tool calls. Set to `0` to
    /// disable. Default: 3. Closes #1980.
    pub max_consecutive_tool_only_iterations: u32,
}

impl Default for NousLimits {
    fn default() -> Self {
        use aletheia_koina::defaults as d;
        Self {
            max_tool_iterations: d::MAX_TOOL_ITERATIONS,
            loop_detection_threshold: 3,
            consecutive_error_threshold: 4,
            loop_max_warnings: 2,
            session_token_cap: default_session_token_cap(),
            max_tool_result_bytes: default_max_tool_result_bytes(),
            max_consecutive_tool_only_iterations: 3,
        }
    }
}

/// Configuration for a single nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NousConfig {
    /// Agent identifier (e.g. "syn", "demiurge").
    pub id: String,
    /// Human-readable display name (e.g. "Syn"). Falls back to `id` if absent.
    #[serde(default)]
    pub name: Option<String>,
    /// LLM generation settings (model, token limits, thinking).
    #[serde(flatten)]
    pub generation: NousGenerationConfig,
    /// Resource and safety limits.
    #[serde(flatten)]
    pub limits: NousLimits,
    /// Domain tags for this agent (static config + pack overlays).
    #[serde(default)]
    pub domains: Vec<String>,
    /// Server-side tools to include in API requests (e.g., web search).
    #[serde(default)]
    pub server_tools: Vec<aletheia_hermeneus::types::ServerToolDefinition>,
    /// Whether prompt caching is enabled for this agent.
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,
    /// Per-agent recall pipeline configuration.
    #[serde(default)]
    pub recall: RecallConfig,
    /// Tool allowlist for sub-agent role enforcement.
    ///
    /// When `Some`, only the listed tool names are available during execution.
    /// When `None`, all registered tools are available. Set by role templates
    /// during ephemeral sub-agent spawning.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

fn default_cache_enabled() -> bool {
    true
}

fn default_session_token_cap() -> u64 {
    500_000
}

fn default_chars_per_token() -> u32 {
    // WHY: must match AgentDefaults::chars_per_token default in taxis so that
    //      the serde default (used when deserialising NousConfig directly)
    //      is identical to the value wired at startup via ResolvedNousConfig.
    4
}

/// Default prosoche model: Haiku-tier for cheap heartbeat checks.
fn default_prosoche_model() -> String {
    "claude-haiku-4-5-20251001".to_owned()
}

fn default_max_tool_result_bytes() -> u32 {
    aletheia_koina::defaults::MAX_TOOL_RESULT_BYTES
}

impl Default for NousConfig {
    fn default() -> Self {
        Self {
            id: "default".to_owned(),
            name: None,
            generation: NousGenerationConfig::default(),
            limits: NousLimits::default(),
            domains: Vec::new(),
            server_tools: Vec::new(),
            cache_enabled: true,
            recall: RecallConfig::default(),
            tool_allowlist: None,
        }
    }
}

/// Pipeline configuration: controls stage behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Token budget for history (remaining after bootstrap).
    pub history_budget_ratio: f64,
    /// Knowledge extraction configuration (None = disabled).
    #[serde(default)]
    pub extraction: Option<aletheia_mneme::extract::ExtractionConfig>,
    /// Per-stage time budgets.
    #[serde(default)]
    pub stage_budget: StageBudget,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            history_budget_ratio: 0.6,
            extraction: None,
            stage_budget: StageBudget::default(),
        }
    }
}

/// Per-stage time budget configuration (seconds).
///
/// Each field is a maximum wall-clock duration for that pipeline stage.
/// `total_secs` is a hard cap: if elapsed time exceeds it, remaining
/// stages are skipped and a partial result is returned.
/// A value of 0 means no limit for that stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageBudget {
    /// Context assembly stage limit.
    pub context_secs: u32,
    /// Semantic recall stage limit.
    pub recall_secs: u32,
    /// History retrieval stage limit.
    pub history_secs: u32,
    /// Guard evaluation stage limit.
    pub guard_secs: u32,
    /// LLM execution stage limit (0 = unlimited, provider controls timeout).
    pub execute_secs: u32,
    /// Finalization stage limit.
    pub finalize_secs: u32,
    /// Hard cap on total pipeline wall-clock time.
    pub total_secs: u32,
}

impl Default for StageBudget {
    fn default() -> Self {
        Self {
            context_secs: 10,
            recall_secs: 15,
            history_secs: 5,
            guard_secs: 2,
            execute_secs: 0,
            finalize_secs: 10,
            total_secs: 300,
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn nous_config_defaults() {
        let config = NousConfig::default();
        assert_eq!(config.generation.context_window, 200_000);
        assert_eq!(
            config.limits.max_tool_iterations,
            aletheia_koina::defaults::MAX_TOOL_ITERATIONS,
            "default should match koina::defaults"
        );
        assert!(!config.generation.thinking_enabled);
        assert_eq!(
            config.limits.max_consecutive_tool_only_iterations, 3,
            "default tool-only iteration limit should be 3"
        );
    }

    #[test]
    fn pipeline_config_defaults() {
        let config = PipelineConfig::default();
        assert!((config.history_budget_ratio - 0.6).abs() < f64::EPSILON);
        assert!(config.extraction.is_none());
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = NousConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: NousConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.id, back.id);
        assert_eq!(config.generation.model, back.generation.model);
    }

    #[test]
    fn pipeline_config_serde_roundtrip() {
        let config = PipelineConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: PipelineConfig = serde_json::from_str(&json).unwrap();
        assert!((back.history_budget_ratio - 0.6).abs() < f64::EPSILON);
        assert!(back.extraction.is_none());
    }

    #[test]
    fn stage_budget_defaults() {
        let budget = StageBudget::default();
        assert_eq!(budget.context_secs, 10);
        assert_eq!(budget.recall_secs, 15);
        assert_eq!(budget.execute_secs, 0);
        assert_eq!(budget.total_secs, 300);
    }

    #[test]
    fn nous_config_custom_values() {
        let config = NousConfig {
            id: "chiron".to_owned(),
            name: Some("Chiron".to_owned()),
            generation: NousGenerationConfig {
                model: "claude-haiku-4-5-20251001".to_owned(),
                context_window: 100_000,
                max_output_tokens: 8_192,
                bootstrap_max_tokens: 20_000,
                thinking_enabled: true,
                thinking_budget: 5_000,
                chars_per_token: 4,
                prosoche_model: "claude-haiku-4-5-20251001".to_owned(),
            },
            limits: NousLimits {
                max_tool_iterations: 10,
                loop_detection_threshold: 5,
                consecutive_error_threshold: 4,
                loop_max_warnings: 2,
                session_token_cap: 250_000,
                max_tool_result_bytes: 32_768,
                max_consecutive_tool_only_iterations: 3,
            },
            domains: vec!["medical".to_owned()],
            server_tools: Vec::new(),
            cache_enabled: false,
            recall: RecallConfig::default(),
            tool_allowlist: None,
        };
        assert_eq!(config.name.as_deref(), Some("Chiron"));
        assert!(config.generation.thinking_enabled);
        assert_eq!(config.domains.len(), 1);
        assert!(!config.cache_enabled);
    }

    #[test]
    fn prosoche_model_defaults_to_haiku() {
        let config = NousConfig::default();
        assert_eq!(
            config.generation.prosoche_model,
            "claude-haiku-4-5-20251001"
        );
    }
}
