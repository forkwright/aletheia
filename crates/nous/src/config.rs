//! Nous agent configuration.

use serde::{Deserialize, Serialize};

/// Configuration for a single nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NousConfig {
    /// Agent identifier (e.g. "syn", "demiurge").
    pub id: String,
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
}

impl Default for NousConfig {
    fn default() -> Self {
        Self {
            id: "default".to_owned(),
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
        }
    }
}

/// Pipeline configuration — controls stage behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Percentage of context window that triggers distillation.
    pub distillation_threshold: f64,
    /// Whether to include agent notes in context.
    pub include_notes: bool,
    /// Whether to include working state in context.
    pub include_working_state: bool,
    /// Maximum number of notes to inject.
    pub max_notes: usize,
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
            distillation_threshold: 0.9,
            include_notes: true,
            include_working_state: true,
            max_notes: 50,
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
        assert!((config.distillation_threshold - 0.9).abs() < f64::EPSILON);
        assert!(config.include_notes);
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = NousConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: NousConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.id, back.id);
        assert_eq!(config.model, back.model);
    }
}
