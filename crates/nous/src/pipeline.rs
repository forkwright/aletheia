//! Message processing pipeline.
//!
//! Each inbound message flows through stages:
//! 1. **Context** — assemble bootstrap (SOUL.md, USER.md, etc.)
//! 2. **History** — load conversation history within token budget
//! 3. **Guard** — check rate limits, loop detection, safety
//! 4. **Resolve** — resolve model, tools, and routing
//! 5. **Execute** — call LLM, process tool use, iterate
//! 6. **Finalize** — persist messages, update counts, extract facts

use serde::{Deserialize, Serialize};
use tracing::instrument;

use aletheia_taxis::oikos::Oikos;

use crate::bootstrap::BootstrapAssembler;
use crate::budget::TokenBudget;
use crate::config::{NousConfig, PipelineConfig};
use crate::session::SessionState;

/// Input to the pipeline — an inbound message.
#[derive(Debug, Clone)]
pub struct PipelineInput {
    /// The user's message content.
    pub content: String,
    /// Session state.
    pub session: SessionState,
    /// Pipeline configuration.
    pub config: PipelineConfig,
}

/// Output from a pipeline stage.
#[derive(Debug, Clone)]
pub struct PipelineContext {
    /// The assembled system prompt.
    pub system_prompt: Option<String>,
    /// Conversation history (messages to send to the LLM).
    pub messages: Vec<PipelineMessage>,
    /// Available tools for this turn.
    pub tools: Vec<String>,
    /// Token budget remaining after bootstrap + history.
    pub remaining_tokens: i64,
    /// Whether distillation is needed before this turn.
    pub needs_distillation: bool,
    /// Guard decision.
    pub guard_result: GuardResult,
}

impl Default for PipelineContext {
    fn default() -> Self {
        Self {
            system_prompt: None,
            messages: Vec::new(),
            tools: Vec::new(),
            remaining_tokens: 0,
            needs_distillation: false,
            guard_result: GuardResult::Allow,
        }
    }
}

/// A message in the pipeline (simplified from full Anthropic types).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMessage {
    /// Message role.
    pub role: String,
    /// Message content.
    pub content: String,
    /// Estimated tokens.
    pub token_estimate: i64,
}

/// Guard stage result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardResult {
    /// Request is allowed.
    Allow,
    /// Request is rate-limited (retry after ms).
    RateLimited { retry_after_ms: u64 },
    /// Loop detected — abort.
    LoopDetected { pattern: String },
    /// Request rejected for safety.
    Rejected { reason: String },
}

/// Loop detector — tracks repeated tool call patterns.
#[derive(Debug, Clone)]
pub struct LoopDetector {
    /// Recent tool call signatures.
    history: Vec<String>,
    /// Threshold for identical consecutive calls.
    threshold: u32,
}

impl LoopDetector {
    /// Create a new loop detector.
    #[must_use]
    pub fn new(threshold: u32) -> Self {
        Self {
            history: Vec::new(),
            threshold,
        }
    }

    /// Record a tool call and check for loops.
    ///
    /// Returns `Some(pattern)` if a loop is detected.
    pub fn record(&mut self, tool_name: &str, input_hash: &str) -> Option<String> {
        let signature = format!("{tool_name}:{input_hash}");
        self.history.push(signature.clone());

        // Check for N consecutive identical calls
        let recent = self.history.iter().rev().take(self.threshold as usize);
        let all_same = recent.clone().count() >= self.threshold as usize
            && recent.clone().all(|s| *s == signature);

        if all_same {
            Some(signature)
        } else {
            None
        }
    }

    /// Reset the detector (e.g. on new turn).
    pub fn reset(&mut self) {
        self.history.clear();
    }

    /// Number of calls recorded.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.history.len()
    }
}

/// Interaction signal — classifies what kind of work a turn involved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InteractionSignal {
    /// Direct conversation (no tools).
    Conversation,
    /// Tool execution occurred.
    ToolExecution,
    /// Code was written or modified.
    CodeGeneration,
    /// Research or web search.
    Research,
    /// Planning or architectural discussion.
    Planning,
    /// Error recovery.
    ErrorRecovery,
}

/// Turn result — the output of processing one turn.
#[derive(Debug, Clone)]
pub struct TurnResult {
    /// Assistant's response content.
    pub content: String,
    /// Tool calls made during this turn.
    pub tool_calls: Vec<ToolCall>,
    /// Token usage.
    pub usage: TurnUsage,
    /// Interaction signals detected.
    pub signals: Vec<InteractionSignal>,
    /// Stop reason.
    pub stop_reason: String,
}

/// A tool call made during a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool call ID.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Input parameters (JSON).
    pub input: serde_json::Value,
    /// Result content.
    pub result: Option<String>,
    /// Whether the tool call errored.
    pub is_error: bool,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
}

/// Token usage for a single turn.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TurnUsage {
    /// Input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Cache read tokens.
    pub cache_read_tokens: u64,
    /// Cache write tokens.
    pub cache_write_tokens: u64,
    /// Number of LLM calls in this turn (1 + tool iterations).
    pub llm_calls: u32,
}

impl TurnUsage {
    /// Total tokens consumed.
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Assemble bootstrap context and populate the pipeline context.
///
/// This is the "context" stage of the pipeline. It:
/// 1. Creates a token budget from the nous config
/// 2. Runs the bootstrap assembler against oikos workspace files
/// 3. Sets [`PipelineContext::system_prompt`] and [`PipelineContext::remaining_tokens`]
///
/// # Errors
///
/// Returns [`crate::error::Error::ContextAssembly`] if required workspace files
/// (e.g. SOUL.md) are missing.
#[instrument(skip_all, fields(nous_id = %nous_config.id))]
pub fn assemble_context(
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
) -> crate::error::Result<()> {
    let mut budget = TokenBudget::new(
        u64::from(nous_config.context_window),
        pipeline_config.history_budget_ratio,
        u64::from(nous_config.max_output_tokens),
        u64::from(nous_config.bootstrap_max_tokens),
    );

    let assembler = BootstrapAssembler::new(oikos);
    let result = assembler.assemble(&nous_config.id, &mut budget)?;

    ctx.system_prompt = Some(result.system_prompt);
    #[expect(clippy::cast_possible_wrap, reason = "budget fits in i64 for practical context windows")]
    {
        ctx.remaining_tokens = budget.remaining() as i64;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Loop Detector ---

    #[test]
    fn loop_detector_no_loop() {
        let mut det = LoopDetector::new(3);
        assert!(det.record("exec", "hash1").is_none());
        assert!(det.record("read", "hash2").is_none());
        assert!(det.record("exec", "hash3").is_none());
    }

    #[test]
    fn loop_detector_detects_repeat() {
        let mut det = LoopDetector::new(3);
        assert!(det.record("exec", "same").is_none());
        assert!(det.record("exec", "same").is_none());
        let result = det.record("exec", "same");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "exec:same");
    }

    #[test]
    fn loop_detector_different_inputs_ok() {
        let mut det = LoopDetector::new(3);
        assert!(det.record("exec", "hash1").is_none());
        assert!(det.record("exec", "hash2").is_none());
        assert!(det.record("exec", "hash3").is_none());
        // Different hashes, no loop
    }

    #[test]
    fn loop_detector_reset() {
        let mut det = LoopDetector::new(3);
        det.record("exec", "same");
        det.record("exec", "same");
        det.reset();
        assert_eq!(det.call_count(), 0);
        assert!(det.record("exec", "same").is_none()); // Reset cleared history
    }

    #[test]
    fn loop_detector_threshold_4() {
        let mut det = LoopDetector::new(4);
        assert!(det.record("exec", "same").is_none());
        assert!(det.record("exec", "same").is_none());
        assert!(det.record("exec", "same").is_none());
        // Not yet — threshold is 4
        let result = det.record("exec", "same");
        assert!(result.is_some());
    }

    // --- Guard Result ---

    #[test]
    fn guard_result_equality() {
        assert_eq!(GuardResult::Allow, GuardResult::Allow);
        assert_ne!(
            GuardResult::Allow,
            GuardResult::Rejected {
                reason: "test".to_owned()
            }
        );
    }

    // --- Turn Usage ---

    #[test]
    fn turn_usage_total() {
        let usage = TurnUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 800,
            cache_write_tokens: 200,
            llm_calls: 3,
        };
        assert_eq!(usage.total_tokens(), 1500);
    }

    // --- Interaction Signal ---

    #[test]
    fn interaction_signal_serde() {
        let signal = InteractionSignal::CodeGeneration;
        let json = serde_json::to_string(&signal).unwrap();
        assert_eq!(json, "\"code_generation\"");
        let back: InteractionSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(back, signal);
    }

    // --- Pipeline Context ---

    #[test]
    fn pipeline_context_default() {
        let ctx = PipelineContext::default();
        assert!(ctx.system_prompt.is_none());
        assert!(ctx.messages.is_empty());
        assert!(!ctx.needs_distillation);
        assert_eq!(ctx.guard_result, GuardResult::Allow);
    }

    // --- Guard Result variants ---

    #[test]
    fn guard_result_rate_limited() {
        let g = GuardResult::RateLimited { retry_after_ms: 5000 };
        assert_ne!(g, GuardResult::Allow);
        match g {
            GuardResult::RateLimited { retry_after_ms } => assert_eq!(retry_after_ms, 5000),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn guard_result_loop_detected() {
        let g = GuardResult::LoopDetected { pattern: "exec:abc".to_owned() };
        match g {
            GuardResult::LoopDetected { pattern } => assert_eq!(pattern, "exec:abc"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn guard_result_rejected() {
        let g = GuardResult::Rejected { reason: "unsafe content".to_owned() };
        match g {
            GuardResult::Rejected { reason } => assert!(reason.contains("unsafe")),
            _ => panic!("wrong variant"),
        }
    }

    // --- Loop Detector edge cases ---

    #[test]
    fn loop_detector_threshold_1() {
        let mut det = LoopDetector::new(1);
        let result = det.record("exec", "hash");
        assert!(result.is_some());
    }

    #[test]
    fn loop_detector_call_count_tracks() {
        let mut det = LoopDetector::new(10);
        det.record("a", "1");
        det.record("b", "2");
        det.record("c", "3");
        assert_eq!(det.call_count(), 3);
    }

    #[test]
    fn loop_detector_many_unique_then_repeat() {
        let mut det = LoopDetector::new(3);
        for i in 0..20 {
            det.record("tool", &format!("hash{i}"));
        }
        assert!(det.record("exec", "same").is_none());
        assert!(det.record("exec", "same").is_none());
        assert!(det.record("exec", "same").is_some());
    }

    // --- Interaction Signal ---

    #[test]
    fn all_interaction_signals_serde_roundtrip() {
        let signals = [
            InteractionSignal::Conversation,
            InteractionSignal::ToolExecution,
            InteractionSignal::CodeGeneration,
            InteractionSignal::Research,
            InteractionSignal::Planning,
            InteractionSignal::ErrorRecovery,
        ];
        for signal in signals {
            let json = serde_json::to_string(&signal).unwrap();
            let back: InteractionSignal = serde_json::from_str(&json).unwrap();
            assert_eq!(signal, back);
        }
    }

    // --- Turn Usage ---

    #[test]
    fn turn_usage_default_is_zero() {
        let usage = TurnUsage::default();
        assert_eq!(usage.total_tokens(), 0);
        assert_eq!(usage.llm_calls, 0);
    }

    #[test]
    fn turn_usage_serde_roundtrip() {
        let usage = TurnUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 80,
            cache_write_tokens: 20,
            llm_calls: 2,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: TurnUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage.total_tokens(), back.total_tokens());
    }

    // --- Context assembly ---

    #[test]
    fn assemble_context_populates_pipeline() {
        use crate::config::{NousConfig, PipelineConfig};
        use aletheia_taxis::oikos::Oikos;
        use std::fs;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("nous/test-agent")).unwrap();
        fs::create_dir_all(root.join("shared")).unwrap();
        fs::create_dir_all(root.join("theke")).unwrap();
        fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").unwrap();
        fs::write(root.join("theke/USER.md"), "Test user.").unwrap();

        let oikos = Oikos::from_root(root);
        let nous_config = NousConfig {
            id: "test-agent".to_owned(),
            ..NousConfig::default()
        };
        let pipeline_config = PipelineConfig::default();
        let mut ctx = PipelineContext::default();

        assemble_context(&oikos, &nous_config, &pipeline_config, &mut ctx).unwrap();

        assert!(ctx.system_prompt.is_some());
        let prompt = ctx.system_prompt.unwrap();
        assert!(prompt.contains("I am a test agent."));
        assert!(prompt.contains("Test user."));
        assert!(ctx.remaining_tokens > 0);
    }
}
