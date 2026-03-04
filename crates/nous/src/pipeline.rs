//! Message processing pipeline.
//!
//! Each inbound message flows through stages:
//! 1. **Context** — assemble bootstrap (SOUL.md, USER.md, etc.)
//!     - **Recall** — retrieve and inject relevant knowledge
//! 2. **History** — load conversation history within token budget
//! 3. **Guard** — check rate limits, loop detection, safety
//! 4. **Resolve** — resolve model, tools, and routing
//! 5. **Execute** — call LLM, process tool use, iterate
//! 6. **Finalize** — persist messages, update counts, extract facts

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::{debug, error, instrument, warn};

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::store::SessionStore;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolContext;
use aletheia_taxis::oikos::Oikos;

use crate::bootstrap::{BootstrapAssembler, BootstrapSection};
use crate::budget::TokenBudget;
use crate::config::{NousConfig, PipelineConfig};
use crate::error;
use crate::history::{self, HistoryConfig, HistoryResult};
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
    /// Token budget remaining after bootstrap (system prompt space).
    pub remaining_tokens: i64,
    /// Token budget allocated for conversation history.
    pub history_budget: i64,
    /// Whether distillation is needed before this turn.
    pub needs_distillation: bool,
    /// Guard decision.
    pub guard_result: GuardResult,
    /// Recall stage output, if recall was run.
    pub recall_result: Option<crate::recall::RecallStageResult>,
    /// History stage output, if history was loaded.
    pub history_result: Option<HistoryResult>,
}

impl Default for PipelineContext {
    fn default() -> Self {
        Self {
            system_prompt: None,
            messages: Vec::new(),
            tools: Vec::new(),
            remaining_tokens: 0,
            history_budget: 0,
            needs_distillation: false,
            guard_result: GuardResult::Allow,
            recall_result: None,
            history_result: None,
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

/// Loop detector — tracks repeated tool call patterns with a capped ring buffer.
#[derive(Debug, Clone)]
pub struct LoopDetector {
    /// Recent tool call signatures (ring buffer, capped at `window` entries).
    history: VecDeque<String>,
    /// Threshold for identical consecutive calls.
    threshold: u32,
    /// Maximum history entries retained.
    window: usize,
}

const DEFAULT_LOOP_WINDOW: usize = 20;

impl LoopDetector {
    /// Create a new loop detector with the default window size (20).
    #[must_use]
    pub fn new(threshold: u32) -> Self {
        Self {
            history: VecDeque::with_capacity(DEFAULT_LOOP_WINDOW),
            threshold,
            window: DEFAULT_LOOP_WINDOW,
        }
    }

    /// Record a tool call and check for loops.
    ///
    /// Returns `Some(pattern)` if a loop is detected (N consecutive identical calls
    /// where N = threshold).
    pub fn record(&mut self, tool_name: &str, input_hash: &str) -> Option<String> {
        let signature = format!("{tool_name}:{input_hash}");
        self.history.push_back(signature.clone());

        // Evict oldest entry if over window cap
        if self.history.len() > self.window {
            self.history.pop_front();
        }

        // Check for N consecutive identical calls
        let recent = self.history.iter().rev().take(self.threshold as usize);
        let all_same = recent.clone().count() >= self.threshold as usize
            && recent.clone().all(|s| *s == signature);

        if all_same { Some(signature) } else { None }
    }

    /// Reset the detector (e.g. on new turn).
    pub fn reset(&mut self) {
        self.history.clear();
    }

    /// Number of calls currently in the history window.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.history.len()
    }

    /// Count consecutive identical entries at the tail of the history.
    ///
    /// Returns 0 if empty, otherwise the number of trailing entries matching the last one.
    #[must_use]
    pub fn pattern_count(&self) -> usize {
        let Some(last) = self.history.back() else {
            return 0;
        };
        self.history
            .iter()
            .rev()
            .take_while(|s| *s == last)
            .count()
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
/// 3. Includes any extra sections (e.g. from domain packs)
/// 4. Sets [`PipelineContext::system_prompt`] and [`PipelineContext::remaining_tokens`]
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
    assemble_context_with_extra(oikos, nous_config, pipeline_config, ctx, Vec::new())
}

/// Assemble bootstrap context with extra sections from domain packs.
#[instrument(skip_all, fields(nous_id = %nous_config.id))]
pub fn assemble_context_with_extra(
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_sections: Vec<BootstrapSection>,
) -> crate::error::Result<()> {
    let mut budget = TokenBudget::new(
        u64::from(nous_config.context_window),
        pipeline_config.history_budget_ratio,
        u64::from(nous_config.max_output_tokens),
        u64::from(nous_config.bootstrap_max_tokens),
    );

    let assembler = BootstrapAssembler::new(oikos);
    let result = assembler.assemble_with_extra(&nous_config.id, &mut budget, extra_sections)?;

    ctx.system_prompt = Some(result.system_prompt);
    #[expect(
        clippy::cast_possible_wrap,
        reason = "budget fits in i64 for practical context windows"
    )]
    {
        ctx.remaining_tokens = budget.remaining() as i64;
        ctx.history_budget = budget.history_budget() as i64;
    }

    Ok(())
}

/// Guard stage — check rate limits, loop detection, safety.
///
/// Stub implementation. Always allows the request.
#[must_use]
pub fn check_guard(_session: &SessionState, _config: &NousConfig) -> GuardResult {
    GuardResult::Allow
}

/// Run the full pipeline for one turn.
///
/// Stages: context → recall → history → guard → execute → finalize.
/// Resolve (stage 4) is future work.
#[expect(
    clippy::too_many_arguments,
    reason = "pipeline threading requires all dependencies until config struct refactor"
)]
#[expect(
    clippy::too_many_lines,
    reason = "pipeline orchestrator — sequential stages are clearer inline"
)]
#[instrument(skip_all, fields(nous_id = %config.id))]
pub async fn run_pipeline(
    input: PipelineInput,
    oikos: &Oikos,
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    embedding_provider: Option<&dyn EmbeddingProvider>,
    vector_search: Option<&dyn crate::recall::VectorSearch>,
    session_store: Option<&Mutex<SessionStore>>,
    extra_bootstrap: Vec<BootstrapSection>,
) -> error::Result<TurnResult> {
    // Stage 1: Context (with domain pack sections if any)
    let mut ctx = PipelineContext::default();
    assemble_context_with_extra(oikos, config, pipeline_config, &mut ctx, extra_bootstrap)?;

    // Stage 1.5: Recall
    if let (Some(ep), Some(vs)) = (embedding_provider, vector_search) {
        let recall_config = crate::recall::RecallConfig::default();
        let recall_stage = crate::recall::RecallStage::new(recall_config);
        #[expect(
            clippy::cast_sign_loss,
            reason = "remaining_tokens is positive after context assembly"
        )]
        let budget = ctx.remaining_tokens.max(0) as u64;
        match recall_stage.run(&input.content, &config.id, ep, vs, budget) {
            Ok(recall_result) => {
                if let Some(ref section) = recall_result.recall_section {
                    if let Some(ref mut prompt) = ctx.system_prompt {
                        prompt.push_str("\n\n");
                        prompt.push_str(section);
                    }
                    #[expect(clippy::cast_possible_wrap, reason = "recall tokens fit in i64")]
                    {
                        ctx.remaining_tokens -= recall_result.tokens_consumed as i64;
                    }
                }
                ctx.recall_result = Some(recall_result);
            }
            Err(e) => {
                warn!(error = %e, "recall stage failed, continuing without recalled knowledge");
            }
        }
    } else {
        debug!("recall skipped: embedding provider or vector search not configured");
    }

    // Stage 2: History
    let history_config = HistoryConfig::default();
    if let Some(store_mutex) = session_store {
        let store = store_mutex.lock().expect("session store lock");
        let (messages, hist_result) = history::load_history(
            &store,
            &input.session.id,
            ctx.history_budget,
            &history_config,
            &input.content,
        )?;
        ctx.messages = messages;
        ctx.history_budget -= hist_result.tokens_consumed;
        ctx.history_result = Some(hist_result);
    } else {
        #[expect(clippy::cast_possible_wrap, reason = "message length fits in i64")]
        let token_estimate = input.content.len() as i64 / 4;
        ctx.messages.push(PipelineMessage {
            role: "user".to_owned(),
            content: input.content.clone(),
            token_estimate,
        });
    }

    // Stage 3: Guard
    let guard = check_guard(&input.session, config);
    match guard {
        GuardResult::Allow => {}
        GuardResult::RateLimited { retry_after_ms } => {
            return Err(error::GuardRejectedSnafu {
                reason: format!("rate limited, retry after {retry_after_ms}ms"),
            }
            .build());
        }
        GuardResult::LoopDetected { pattern } => {
            return Err(error::LoopDetectedSnafu {
                iterations: 0u32,
                pattern,
            }
            .build());
        }
        GuardResult::Rejected { reason } => {
            return Err(error::GuardRejectedSnafu { reason }.build());
        }
    }

    // Stage 4: Resolve (stub)

    // Stage 5: Execute
    let result =
        crate::execute::execute(&ctx, &input.session, config, providers, tools, tool_ctx).await?;

    // Stage 6: Finalize
    if let Some(store_mutex) = session_store {
        let store = store_mutex.lock().expect("session store lock");
        let finalize_config = crate::finalize::FinalizeConfig::default();
        match crate::finalize::finalize(
            &store,
            &input.session,
            &input.content,
            &result,
            &finalize_config,
        ) {
            Ok(fr) => {
                debug!(
                    messages = fr.messages_persisted,
                    usage = fr.usage_recorded,
                    "finalize complete"
                );
            }
            Err(e) => {
                error!(error = %e, "finalize failed, returning result without persistence");
            }
        }
    } else {
        debug!("no session store, skipping finalize");
    }

    Ok(result)
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
        let g = GuardResult::RateLimited {
            retry_after_ms: 5000,
        };
        assert_ne!(g, GuardResult::Allow);
        match g {
            GuardResult::RateLimited { retry_after_ms } => assert_eq!(retry_after_ms, 5000),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn guard_result_loop_detected() {
        let g = GuardResult::LoopDetected {
            pattern: "exec:abc".to_owned(),
        };
        match g {
            GuardResult::LoopDetected { pattern } => assert_eq!(pattern, "exec:abc"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn guard_result_rejected() {
        let g = GuardResult::Rejected {
            reason: "unsafe content".to_owned(),
        };
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

    // --- run_pipeline ---

    #[tokio::test]
    async fn run_pipeline_simple() {
        use std::fs;
        use std::path::PathBuf;
        use std::sync::Mutex;

        use aletheia_hermeneus::provider::{LlmProvider, ProviderRegistry};
        use aletheia_hermeneus::types::{
            CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
        };
        use aletheia_koina::id::{NousId, SessionId};
        use aletheia_organon::registry::ToolRegistry;
        use aletheia_organon::types::ToolContext;
        use tempfile::TempDir;

        struct MockProvider {
            response: Mutex<CompletionResponse>,
        }
        impl LlmProvider for MockProvider {
            fn complete(
                &self,
                _request: &CompletionRequest,
            ) -> aletheia_hermeneus::error::Result<CompletionResponse> {
                Ok(self.response.lock().expect("lock").clone())
            }
            fn supported_models(&self) -> &[&str] {
                &["test-model"]
            }
            #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str return")]
            fn name(&self) -> &str {
                "mock"
            }
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("nous/test-agent")).unwrap();
        fs::create_dir_all(root.join("shared")).unwrap();
        fs::create_dir_all(root.join("theke")).unwrap();
        fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").unwrap();

        let oikos = Oikos::from_root(root);
        let nous_config = NousConfig {
            id: "test-agent".to_owned(),
            model: "test-model".to_owned(),
            ..NousConfig::default()
        };
        let pipeline_config = PipelineConfig::default();

        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider {
            response: Mutex::new(CompletionResponse {
                id: "resp-1".to_owned(),
                model: "test-model".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: "Hello from pipeline!".to_owned(),
                }],
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 50,
                    ..Usage::default()
                },
            }),
        }));

        let tools = ToolRegistry::new();
        let tool_ctx = ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
        };

        let session = crate::session::SessionState::new(
            "test-session".to_owned(),
            "main".to_owned(),
            &nous_config,
        );
        let input = PipelineInput {
            content: "Hello".to_owned(),
            session,
            config: pipeline_config.clone(),
        };

        let result = run_pipeline(
            input,
            &oikos,
            &nous_config,
            &pipeline_config,
            &providers,
            &tools,
            &tool_ctx,
            None,
            None,
            None,
            Vec::new(),
        )
        .await
        .expect("pipeline should succeed");

        assert_eq!(result.content, "Hello from pipeline!");
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.usage.llm_calls, 1);
        assert_eq!(result.stop_reason, "end_turn");
    }

    // --- Loop Detector window cap ---

    #[test]
    fn loop_detector_window_cap_evicts_old_calls() {
        let mut det = LoopDetector::new(100); // high threshold so no loop triggers
        for i in 0..25 {
            det.record("tool", &format!("hash{i}"));
        }
        assert_eq!(
            det.call_count(),
            DEFAULT_LOOP_WINDOW,
            "history should be capped at window size"
        );
    }

    #[test]
    fn loop_detector_pattern_count_tracks_repetitions() {
        let mut det = LoopDetector::new(100);
        det.record("exec", "same");
        det.record("exec", "same");
        det.record("exec", "same");
        assert_eq!(det.pattern_count(), 3);
    }

    #[test]
    fn loop_detector_pattern_count_zero_on_empty() {
        let det = LoopDetector::new(3);
        assert_eq!(det.pattern_count(), 0);
    }

    #[test]
    fn loop_detector_pattern_count_resets_on_different() {
        let mut det = LoopDetector::new(100);
        det.record("exec", "hash1");
        det.record("exec", "hash1");
        det.record("read", "hash2");
        assert_eq!(det.pattern_count(), 1, "different call breaks the streak");
    }

    #[test]
    fn loop_detector_window_still_detects_loops() {
        let mut det = LoopDetector::new(3);
        // Fill window with unique calls
        for i in 0..18 {
            det.record("tool", &format!("hash{i}"));
        }
        // Now trigger a loop within the window
        assert!(det.record("exec", "same").is_none());
        assert!(det.record("exec", "same").is_none());
        assert!(det.record("exec", "same").is_some(), "should detect loop even after window eviction");
    }
}
