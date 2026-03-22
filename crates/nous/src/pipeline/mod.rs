//! Message processing pipeline.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{info_span, instrument};

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_koina::event::EventEmitter;
use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::store::SessionStore;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolContext;
use aletheia_taxis::oikos::Oikos;

use crate::bootstrap::{BootstrapAssembler, BootstrapSection};
use crate::budget::TokenBudget;
use crate::config::{NousConfig, PipelineConfig};
use crate::error;
use crate::history::HistoryResult;
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;
use crate::working_state::WorkingState;

/// Input to the pipeline: an inbound message.
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
    /// Working state from the previous turn (loaded from persistence).
    pub working_state: Option<WorkingState>,
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
            working_state: None,
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
#[expect(
    missing_docs,
    reason = "variant fields (retry_after_ms, pattern, reason) are self-documenting by name"
)]
#[non_exhaustive]
pub enum GuardResult {
    /// Request is allowed.
    Allow,
    /// Request is rate-limited (retry after ms).
    RateLimited { retry_after_ms: u64 },
    /// Loop detected: abort.
    LoopDetected { pattern: String },
    /// Request rejected for safety.
    Rejected { reason: String },
}

/// Verdict from loop detection after recording a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoopVerdict {
    /// No loop detected.
    Ok,
    /// Loop pattern detected; inject a warning and continue.
    Warn {
        /// Detected pattern description.
        pattern: String,
        /// Human-readable warning to inject into conversation.
        message: String,
    },
    /// Loop confirmed after repeated warnings; halt execution.
    Halt {
        /// Detected pattern description.
        pattern: String,
        /// Human-readable halt message.
        message: String,
    },
}

/// A recorded tool call with execution outcome.
#[derive(Debug, Clone)]
struct CallRecord {
    /// `tool_name:input_hash` signature for identity comparison.
    signature: String,
    /// Tool name (without hash) for pattern descriptions.
    tool_name: String,
    /// Whether the tool call returned an error.
    is_error: bool,
}

/// Loop detector: tracks repeated tool call patterns with a capped ring buffer.
///
/// Detects four patterns:
/// 1. Same tool called with identical arguments N times
/// 2. Alternating between two failing tools
/// 3. Consecutive errors from the same tool (escalating retries)
/// 4. Circular tool chains (A → B → C → A with same context)
///
/// Uses a two-tier response: first `Warn`, then `Halt` after `max_warnings`.
#[derive(Debug, Clone)]
pub struct LoopDetector {
    /// Recent tool call records (ring buffer, capped at `window` entries).
    history: VecDeque<CallRecord>,
    /// Threshold for identical consecutive calls.
    threshold: u32,
    /// Threshold for consecutive error detection.
    error_threshold: u32,
    /// Maximum warnings before escalating to halt.
    max_warnings: u32,
    /// Number of warnings issued so far.
    warnings_issued: u32,
    /// Maximum history entries retained.
    window: usize,
}

const DEFAULT_LOOP_WINDOW: usize = 50;

/// Maximum cycle length tested during the cycle-detection pass.
const CYCLE_DETECTION_MAX_LEN: usize = 10;

impl LoopDetector {
    /// Create a new loop detector with the default window size (50).
    #[must_use]
    pub fn new(threshold: u32) -> Self {
        Self {
            history: VecDeque::with_capacity(DEFAULT_LOOP_WINDOW),
            threshold,
            error_threshold: 4,
            max_warnings: 2,
            warnings_issued: 0,
            window: DEFAULT_LOOP_WINDOW,
        }
    }

    /// Create a loop detector with full configuration.
    #[must_use]
    pub fn with_limits(threshold: u32, error_threshold: u32, max_warnings: u32) -> Self {
        Self {
            history: VecDeque::with_capacity(DEFAULT_LOOP_WINDOW),
            threshold,
            error_threshold,
            max_warnings,
            warnings_issued: 0,
            window: DEFAULT_LOOP_WINDOW,
        }
    }

    /// Record a tool call and check for loop patterns.
    ///
    /// Returns [`LoopVerdict::Ok`] if no pattern is detected,
    /// [`LoopVerdict::Warn`] on first detection (inject warning and continue),
    /// or [`LoopVerdict::Halt`] after `max_warnings` have been issued.
    pub fn record(&mut self, tool_name: &str, input_hash: &str, is_error: bool) -> LoopVerdict {
        let signature = format!("{tool_name}:{input_hash}");
        self.history.push_back(CallRecord {
            signature,
            tool_name: tool_name.to_owned(),
            is_error,
        });

        if self.history.len() > self.window {
            self.history.pop_front();
        }

        if let Some(pattern) = self.detect_same_args() {
            return self.emit_verdict(pattern);
        }

        if let Some(pattern) = self.detect_alternating_failure() {
            return self.emit_verdict(pattern);
        }

        if let Some(pattern) = self.detect_consecutive_errors() {
            return self.emit_verdict(pattern);
        }

        if let Some(pattern) = self.detect_cycle() {
            return self.emit_verdict(pattern);
        }

        LoopVerdict::Ok
    }

    /// Reset the detector (e.g. on new turn).
    pub fn reset(&mut self) {
        self.history.clear();
        self.warnings_issued = 0;
    }

    /// Number of calls currently in the history window.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.history.len()
    }

    /// Count consecutive identical signatures at the tail of the history.
    #[must_use]
    pub fn pattern_count(&self) -> usize {
        let Some(last) = self.history.back() else {
            return 0;
        };
        self.history
            .iter()
            .rev()
            .take_while(|r| r.signature == last.signature)
            .count()
    }

    /// Number of loop warnings issued during this detector's lifetime.
    #[must_use]
    pub fn warnings_issued(&self) -> u32 {
        self.warnings_issued
    }

    /// Convert a detected pattern into a `Warn` or `Halt` verdict.
    fn emit_verdict(&mut self, pattern: String) -> LoopVerdict {
        if self.warnings_issued >= self.max_warnings {
            LoopVerdict::Halt {
                message: format!(
                    "Loop confirmed after {} warnings. Pattern: {pattern}. \
                     Stopping execution — user intervention required.",
                    self.warnings_issued
                ),
                pattern,
            }
        } else {
            self.warnings_issued += 1;
            LoopVerdict::Warn {
                message: format!("Loop detected: {pattern}. Try a different approach."),
                pattern,
            }
        }
    }

    /// Detect N consecutive identical tool call signatures.
    fn detect_same_args(&self) -> Option<String> {
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold is a small constant, fits in usize"
        )]
        let t = self.threshold as usize; // kanon:ignore RUST/as-cast
        if self.history.len() < t {
            return None;
        }

        let last = self.history.back()?;
        let count = self
            .history
            .iter()
            .rev()
            .take(t)
            .filter(|r| r.signature == last.signature)
            .count();

        if count >= t {
            Some(last.signature.clone())
        } else {
            None
        }
    }

    /// Detect two failing tools alternating: A(err) → B(err) → A(err) → B(err).
    fn detect_alternating_failure(&self) -> Option<String> {
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold is a small constant, fits in usize"
        )]
        let t = self.threshold as usize; // kanon:ignore RUST/as-cast
        let needed = 2 * t;
        let n = self.history.len();
        if n < needed {
            return None;
        }

        let tail_start = n - needed;
        let first = self.history.get(tail_start)?;
        let second = self.history.get(tail_start + 1)?;

        if !first.is_error || !second.is_error || first.tool_name == second.tool_name {
            return None;
        }

        let matches = (0..t).all(|rep| {
            let a_idx = tail_start + rep * 2;
            let b_idx = a_idx + 1;
            match (self.history.get(a_idx), self.history.get(b_idx)) {
                (Some(a), Some(b)) => {
                    a.is_error
                        && b.is_error
                        && a.tool_name == first.tool_name
                        && b.tool_name == second.tool_name
                }
                _ => false,
            }
        });

        if matches {
            Some(format!(
                "alternating failures: {} and {}",
                first.tool_name, second.tool_name
            ))
        } else {
            None
        }
    }

    /// Detect N consecutive errors from the same tool (escalating retries).
    fn detect_consecutive_errors(&self) -> Option<String> {
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: error_threshold is a small constant, fits in usize"
        )]
        let t = self.error_threshold as usize; // kanon:ignore RUST/as-cast
        if self.history.len() < t {
            return None;
        }

        let trailing: Vec<&CallRecord> = self
            .history
            .iter()
            .rev()
            .take_while(|r| r.is_error)
            .collect();

        if trailing.len() < t {
            return None;
        }

        // WHY: check if all trailing errors are from the same tool (escalating retries on one tool)
        let tool = &trailing.first()?.tool_name;
        let same_tool = trailing.iter().take(t).all(|r| r.tool_name == *tool);

        if same_tool {
            Some(format!(
                "escalating retries: {} failed {} consecutive times",
                tool,
                trailing.len()
            ))
        } else {
            Some(format!(
                "consecutive failures: {} errors in a row",
                trailing.len()
            ))
        }
    }

    /// Detect repeating cycles of length 2-`CYCLE_DETECTION_MAX_LEN`.
    fn detect_cycle(&self) -> Option<String> {
        let n = self.history.len();
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold is a small constant, fits in usize"
        )]
        let t = self.threshold as usize; // kanon:ignore RUST/as-cast

        for cycle_len in 2..=CYCLE_DETECTION_MAX_LEN {
            let needed = cycle_len * t;
            if n < needed {
                continue;
            }
            let pattern_start = n - cycle_len;
            let all_match = (1..t).all(|rep| {
                let seg_start = n - cycle_len * (rep + 1);
                (0..cycle_len).all(|i| {
                    match (
                        self.history.get(pattern_start + i),
                        self.history.get(seg_start + i),
                    ) {
                        (Some(a), Some(b)) => a.signature == b.signature,
                        _ => false,
                    }
                })
            });
            if all_match {
                let pattern: String = self
                    .history
                    .iter()
                    .skip(pattern_start)
                    .map(|r| r.signature.as_str())
                    .fold(String::new(), |mut acc, s| {
                        if !acc.is_empty() {
                            acc.push(',');
                        }
                        acc.push_str(s);
                        acc
                    });
                return Some(pattern);
            }
        }

        None
    }
}

/// Interaction signal: classifies what kind of work a turn involved.
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

/// Turn result: the output of processing one turn.
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
/// Returns an error if required workspace files
/// (e.g. SOUL.md) are missing.
#[instrument(skip_all, fields(nous_id = %nous_config.id))]
pub async fn assemble_context(
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
) -> crate::error::Result<()> {
    assemble_context_with_extra(oikos, nous_config, pipeline_config, ctx, Vec::new()).await
}

/// Assemble bootstrap context with extra sections from domain packs.
#[instrument(skip_all, fields(nous_id = %nous_config.id))]
pub async fn assemble_context_with_extra(
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_sections: Vec<BootstrapSection>,
) -> crate::error::Result<()> {
    let mut budget = TokenBudget::new(
        u64::from(nous_config.generation.context_window),
        pipeline_config.history_budget_ratio,
        u64::from(nous_config.generation.max_output_tokens),
        u64::from(nous_config.generation.bootstrap_max_tokens),
    );

    let assembler = BootstrapAssembler::new(oikos);
    let result = assembler
        .assemble_with_extra(&nous_config.id, &mut budget, extra_sections)
        .await?;

    ctx.system_prompt = Some(result.system_prompt);
    #[expect(
        clippy::cast_possible_wrap,
        clippy::as_conversions,
        reason = "u64→i64: budget fits in i64 for practical context windows"
    )]
    {
        ctx.remaining_tokens = budget.remaining() as i64; // kanon:ignore RUST/as-cast
        ctx.history_budget = budget.history_budget() as i64; // kanon:ignore RUST/as-cast
    }

    Ok(())
}

/// Guard stage: check rate limits, loop detection, safety.
///
/// Enforces the per-session token spending cap from
/// `NousConfig::session_token_cap`. A cap of `0` disables the check.
#[must_use]
pub fn check_guard(session: &SessionState, config: &NousConfig) -> GuardResult {
    if config.limits.session_token_cap > 0
        && session.cumulative_tokens >= config.limits.session_token_cap
    {
        return GuardResult::Rejected {
            reason: format!(
                "session token budget exhausted: {} of {} tokens used",
                session.cumulative_tokens, config.limits.session_token_cap
            ),
        };
    }
    GuardResult::Allow
}

/// Run the full pipeline for one turn.
///
/// Stages: context → recall → history → guard → execute → finalize.
///
/// The [`EventEmitter`] couples metrics and logs: each stage emits a single
/// typed event that simultaneously records a metric and produces a structured
/// log line. Pass `None` to use a default log-only emitter.
#[expect(
    clippy::too_many_arguments,
    reason = "pipeline threading requires all dependencies until config struct refactor"
)]
#[expect(
    clippy::too_many_lines,
    reason = "pipeline orchestration is sequential, splitting adds indirection"
)]
pub(crate) async fn run_pipeline(
    input: PipelineInput,
    oikos: &Oikos,
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    embedding_provider: Option<&dyn EmbeddingProvider>,
    vector_search: Option<&dyn crate::recall::VectorSearch>,
    text_search: Option<&dyn crate::recall::TextSearch>,
    session_store: Option<&Mutex<SessionStore>>,
    extra_bootstrap: Vec<BootstrapSection>,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    emitter: Option<&EventEmitter>,
) -> error::Result<TurnResult> {
    let default_emitter = EventEmitter::new();
    let emitter = emitter.unwrap_or(&default_emitter);

    let pipeline_start = Instant::now();
    let total_timeout = if pipeline_config.stage_budget.total_secs > 0 {
        Some(Duration::from_secs(u64::from(
            pipeline_config.stage_budget.total_secs,
        )))
    } else {
        None
    };
    let pipeline_span = info_span!("pipeline",
        nous_id = %config.id,
        session_id = %input.session.id,
        pipeline.total_duration_ms = tracing::field::Empty,
        pipeline.stages_completed = tracing::field::Empty,
        pipeline.tool_calls = tracing::field::Empty,
        pipeline.model = %config.generation.model,
    );
    let _pipeline_guard = pipeline_span.enter();
    let mut stages_completed: u32 = 0;

    let mut ctx = PipelineContext::default();

    run_context_stage(
        oikos,
        config,
        pipeline_config,
        &mut ctx,
        extra_bootstrap,
        emitter,
    )
    .await?;
    stages_completed += 1;

    run_recall_stage(
        config,
        pipeline_config,
        &mut ctx,
        &input.content,
        embedding_provider,
        vector_search,
        text_search,
        emitter,
    )
    .await;
    stages_completed += 1;

    run_history_stage(config, &mut ctx, &input, session_store, emitter).await?;
    stages_completed += 1;

    run_guard_stage(&input.session, config, emitter)?;
    stages_completed += 1;

    let result = run_execute_stage(
        config,
        pipeline_config,
        &ctx,
        &input,
        providers,
        tools,
        tool_ctx,
        stream_tx,
        pipeline_start,
        total_timeout,
        emitter,
    )
    .await?;
    stages_completed += 1;

    run_finalize_stage(config, &input, &result, session_store, emitter).await;
    stages_completed += 1;

    if !result.tool_calls.is_empty() {
        crate::instinct::record_observations(&result.tool_calls, &input.content, &config.id);
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "u128→u64: pipeline duration fits in u64; usize→u64 for tool call count"
    )]
    {
        pipeline_span.record(
            "pipeline.total_duration_ms",
            pipeline_start.elapsed().as_millis() as u64, // kanon:ignore RUST/as-cast
        );
    }
    pipeline_span.record("pipeline.stages_completed", stages_completed);
    #[expect(
        clippy::as_conversions,
        reason = "usize→u64: tool call count fits in u64"
    )]
    pipeline_span.record("pipeline.tool_calls", result.tool_calls.len() as u64); // kanon:ignore RUST/as-cast

    // Single event emission replaces separate metrics::record_turn + tracing::info.
    crate::metrics::record_turn(&config.id);
    let duration_ms = u64::try_from(pipeline_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    #[expect(
        clippy::as_conversions,
        reason = "usize→u64: tool call count fits in u64"
    )]
    let tool_calls_count = result.tool_calls.len() as u64; // kanon:ignore RUST/as-cast
    emitter.emit(&events::TurnCompleted {
        nous_id: config.id.clone(),
        model: config.generation.model.clone(),
        duration_ms,
        input_tokens: result.usage.input_tokens,
        output_tokens: result.usage.output_tokens,
        tool_calls: tool_calls_count,
        stages_completed,
    });

    Ok(result)
}

/// Typed pipeline events for the internal event system.
pub(crate) mod events;

/// Context stage: assemble bootstrap and system prompt.
mod stages;

use stages::{
    run_context_stage, run_execute_stage, run_finalize_stage, run_guard_stage, run_history_stage,
    run_recall_stage,
};

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
