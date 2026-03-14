//! Nous agent configuration.

use serde::{Deserialize, Serialize};

use crate::recall::RecallConfig;

/// Configuration for a single nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NousConfig {
    /// Agent identifier (e.g. "syn", "demiurge").
    pub id: String,
    /// Human-readable display name (e.g. "Syn"). Falls back to `id` if absent.
    #[serde(default)]
    pub name: Option<String>,
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
    /// Maximum tool execution iterations per turn.
    pub max_tool_iterations: u32,
    /// Loop detection threshold (identical tool calls).
    pub loop_detection_threshold: u32,
    /// Domain tags for this agent (static config + pack overlays).
    #[serde(default)]
    pub domains: Vec<String>,
    /// Server-side tools to include in API requests (e.g., web search).
    #[serde(default)]
    pub server_tools: Vec<aletheia_hermeneus::types::ServerToolDefinition>,
    /// Whether prompt caching is enabled for this agent.
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,
    /// Maximum cumulative tokens (input + output) allowed per session.
    ///
    /// Once a session exceeds this budget the guard stage rejects further
    /// turns with a `GuardResult::Rejected` response. Set to `0` to disable
    /// the cap (default: 500,000).
    #[serde(default = "default_session_token_cap")]
    pub session_token_cap: u64,
    /// Per-agent recall pipeline configuration.
    ///
    /// Overrides the global defaults for this agent's recall stage.
    /// Wired from the taxis per-agent config block at startup.
    #[serde(default)]
    pub recall: RecallConfig,
    /// Characters per token for conservative token-budget estimation.
    ///
    /// Used by `CharEstimator` when sizing bootstrap sections and recall
    /// output.  The default of 4 follows the common "1 token ≈ 4 chars"
    /// heuristic. Wired from `agents.defaults.chars_per_token` at startup.
    #[serde(default = "default_chars_per_token")]
    pub chars_per_token: u32,
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

impl Default for NousConfig {
    fn default() -> Self {
        Self {
            id: "default".to_owned(),
            name: None,
            model: "claude-opus-4-20250514".to_owned(),
            context_window: 200_000,
            max_output_tokens: 16_384,
            bootstrap_max_tokens: 40_000,
            thinking_enabled: false,
            thinking_budget: 10_000,
            max_tool_iterations: 50,
            loop_detection_threshold: 3,
            domains: Vec::new(),
            server_tools: Vec::new(),
            cache_enabled: true,
            session_token_cap: default_session_token_cap(),
            recall: RecallConfig::default(),
            chars_per_token: default_chars_per_token(),
        }
    }
}

/// Pipeline configuration — controls stage behavior.
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
/// `total_secs` is a hard cap — if elapsed time exceeds it, remaining
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
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn nous_config_defaults() {
        let config = NousConfig::default();
        assert_eq!(config.context_window, 200_000);
        assert_eq!(config.max_tool_iterations, 50);
        assert!(!config.thinking_enabled);
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
        assert_eq!(config.model, back.model);
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
            model: "claude-haiku-4-5-20251001".to_owned(),
            context_window: 100_000,
            max_output_tokens: 8_192,
            bootstrap_max_tokens: 20_000,
            thinking_enabled: true,
            thinking_budget: 5_000,
            max_tool_iterations: 10,
            loop_detection_threshold: 5,
            domains: vec!["medical".to_owned()],
            server_tools: Vec::new(),
            cache_enabled: false,
            session_token_cap: 250_000,
            recall: RecallConfig::default(),
            chars_per_token: 4,
        };
        assert_eq!(config.name.as_deref(), Some("Chiron"));
        assert!(config.thinking_enabled);
        assert_eq!(config.domains.len(), 1);
        assert!(!config.cache_enabled);
    }
}
