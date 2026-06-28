// kanon:ignore RUST/file-too-long — pipeline turn orchestration; stage extraction into submodules planned
//! Message processing pipeline.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jiff;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{Instrument, info_span, instrument, warn};

use hermeneus::provider::ProviderRegistry;
use koina::event::EventEmitter;
use mneme::embedding::EmbeddingProvider;
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use organon::registry::ToolRegistry;
use organon::types::ToolContext;
use taxis::oikos::Oikos;

use crate::bootstrap::{BootstrapAssembler, BootstrapSection, TaskHint, classify_task_hint};
use crate::budget::{CompactionMetrics, TimeBudget, TokenBudget};
use crate::config::{NousConfig, PipelineConfig};
use crate::error;
use crate::history::HistoryResult;
use crate::hooks::registry::HookRegistry;
use crate::hooks::{CompactionContext, QueryContext, SessionStartContext, TurnContext};
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;
use crate::working_state::WorkingState;

/// Input to the pipeline: an inbound message.
// kanon:ignore TOPOLOGY/shallow-struct — input bag passed into the pipeline entry point; no in-file behavior by design
#[derive(Debug, Clone)]
pub struct PipelineInput {
    /// The user's message content.
    pub content: String,
    /// Session state.
    pub session: SessionState,
    /// Pipeline configuration.
    pub config: PipelineConfig,
}

impl PipelineInput {
    /// Construct a pipeline input from its constituent parts.
    #[must_use]
    pub fn new(content: String, session: SessionState, config: PipelineConfig) -> Self {
        Self {
            content,
            session,
            config,
        }
    }
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
    /// Compaction metrics from the most recent compaction pass.
    pub compaction_metrics: Option<CompactionMetrics>,
    /// Pre-LLM triage result (intent, sensitivity, tier), if triage was run.
    pub triage_result: Option<triage::TriageResult>,
    /// Reflection stage output, if reflection was run.
    pub reflection_result: Option<ReflectionResult>,
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
            compaction_metrics: None,
            triage_result: None,
            reflection_result: None,
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
    /// WHY(#3781): when true, this message marks a cache breakpoint where
    /// the prefix up to and including this message should be cached.
    /// Typically set on the distilled summary message after compaction.
    #[serde(default)]
    pub cache_breakpoint: bool,
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

/// Assemble a sequence of [`Step`]s from pipeline messages.
///
/// Walks the message stream and groups each assistant message with the
/// contiguous tool-result messages that follow it. Each group becomes one
/// [`Step`] where the assistant content is the `self_note` and the tool
/// results become [`Observation`]s.
///
/// Non-tool user messages (e.g., the original user prompt) act as turn
/// boundaries but do not produce Steps themselves.
///
/// # Edge cases
///
/// - Tool results with no preceding assistant message are attached to the
///   most recent prior step. If no prior step exists, they are dropped.
/// - An assistant message with no trailing tool results produces a step with
///   empty observations.
pub fn assemble_steps(messages: &[PipelineMessage]) -> Vec<crate::memory::step::Step> {
    use crate::memory::step::{Observation, Step};

    let mut steps: Vec<Step> = Vec::new();
    let mut current_note: Option<String> = None;
    let mut current_obs: Vec<Observation> = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "assistant" => {
                if let Some(note) = current_note.take() {
                    steps.push(Step::from_assistant_turn(
                        note,
                        std::mem::take(&mut current_obs),
                        steps.len(),
                    ));
                }
                current_note = Some(msg.content.clone());
            }
            "user" if msg.content.starts_with("[tool:") => {
                let source = extract_tool_name(&msg.content)
                    .map_or_else(|| "unknown".to_owned(), std::borrow::ToOwned::to_owned);
                let obs = Observation::new(source, msg.content.clone());
                if current_note.is_some() {
                    current_obs.push(obs);
                } else if let Some(last) = steps.last_mut() {
                    last.observations.push(obs);
                }
                // NOTE: with no steps at all, the orphan observation is dropped.
            }
            _ => {
                if let Some(note) = current_note.take() {
                    steps.push(Step::from_assistant_turn(
                        note,
                        std::mem::take(&mut current_obs),
                        steps.len(),
                    ));
                }
            }
        }
    }

    if let Some(note) = current_note.take() {
        steps.push(Step::from_assistant_turn(
            note,
            std::mem::take(&mut current_obs),
            steps.len(),
        ));
    }

    steps
}

/// Extract the tool name from a formatted tool result message.
///
/// Expects content starting with `[tool:<name>@<timestamp>]`.
fn extract_tool_name(content: &str) -> Option<&str> {
    let content = content.strip_prefix("[tool:")?;
    let end = content.find('@')?;
    content.get(..end)
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
///
/// Agents can get stuck in loops (repeatedly calling the same failing tool,
/// oscillating between two approaches). The two-tier response gives the LLM
/// a chance to self-correct with a warning before we forcibly halt execution.
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
    /// Maximum cycle length examined during cycle detection. Default: 10.
    cycle_detection_max_len: usize,
}

impl LoopDetector {
    /// Create a new loop detector with the default window size.
    ///
    /// Window size and cycle detection length read from
    /// [`taxis::config::NousBehaviorConfig`] defaults.
    #[must_use]
    pub fn new(threshold: u32) -> Self {
        let b = taxis::config::NousBehaviorConfig::default();
        Self {
            history: VecDeque::with_capacity(b.loop_detection_window),
            threshold,
            error_threshold: 4,
            max_warnings: 2,
            warnings_issued: 0,
            window: b.loop_detection_window,
            cycle_detection_max_len: b.cycle_detection_max_len,
        }
    }

    /// Create a loop detector with full configuration.
    ///
    /// Window size and cycle detection length read from
    /// [`taxis::config::NousBehaviorConfig`] defaults.
    #[must_use]
    pub fn with_limits(threshold: u32, error_threshold: u32, max_warnings: u32) -> Self {
        let b = taxis::config::NousBehaviorConfig::default();
        Self {
            history: VecDeque::with_capacity(b.loop_detection_window),
            threshold,
            error_threshold,
            max_warnings,
            warnings_issued: 0,
            window: b.loop_detection_window,
            cycle_detection_max_len: b.cycle_detection_max_len,
        }
    }

    /// Create a loop detector with explicit window and cycle-detection parameters.
    ///
    /// WHY: lets callers thread configured values without requiring a full
    /// [`taxis::config::NousBehaviorConfig`] reference in the execute stage.
    #[must_use]
    pub fn with_window(
        threshold: u32,
        error_threshold: u32,
        max_warnings: u32,
        window: usize,
        cycle_detection_max_len: usize,
    ) -> Self {
        Self {
            history: VecDeque::with_capacity(window),
            threshold,
            error_threshold,
            max_warnings,
            warnings_issued: 0,
            window,
            cycle_detection_max_len,
        }
    }

    /// Record a tool call and check for loop patterns.
    ///
    /// Returns [`LoopVerdict::Ok`] if no pattern is detected,
    /// [`LoopVerdict::Warn`] on first detection (inject warning and continue),
    /// or [`LoopVerdict::Halt`] after `max_warnings` have been issued.
    ///
    /// The `input_hash` (not full input) is used for comparison to keep
    /// memory usage bounded. Collisions are unlikely with a good hash and
    /// false positives only trigger warnings, not immediate halts.
    // kanon:ignore RUST/doc-promised-observability — doc comment describes algorithm, not tracing; function is pure logic
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

    /// Detect repeating cycles of length 2--`cycle_detection_max_len`.
    fn detect_cycle(&self) -> Option<String> {
        let n = self.history.len();
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold is a small constant, fits in usize"
        )]
        let t = self.threshold as usize; // kanon:ignore RUST/as-cast

        tracing::debug!(
            cycle_detection_max_len = self.cycle_detection_max_len,
            "loop detector: detect_cycle"
        );

        for cycle_len in 2..=self.cycle_detection_max_len {
            let needed = cycle_len * t;
            if n < needed {
                // WHY: cycle_len increases each iteration; if n is too small for this
                // cycle_len, it is too small for all larger lengths. Breaking avoids
                // unnecessary iterations.
                break;
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

/// Result from the optional reflection stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionResult {
    /// Why the stage ended the way it did.
    pub status: ReflectionStatus,
    /// Number of facts emitted (reflected) during this stage.
    pub facts_emitted: u32,
}

/// Reflection stage completion status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReflectionStatus {
    /// Stage did not run because reflection is disabled.
    Disabled,
    /// Stage skipped because there were no eligible facts to reflect.
    Skipped,
    /// Stage skipped because no `KnowledgeStore` is available in the pipeline.
    NoStore,
    /// Reflection completed and facts were emitted.
    Completed,
    /// Reflection attempted durable work and failed.
    Failed,
}

impl ReflectionStatus {
    /// Stable metric/log label for this status.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Skipped => "skipped",
            Self::NoStore => "no_store",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl ReflectionResult {
    /// Create a new reflection result.
    #[must_use]
    pub fn new(status: ReflectionStatus, facts_emitted: u32) -> Self {
        Self {
            status,
            facts_emitted,
        }
    }
}

/// Turn result: the output of processing one turn.
// kanon:ignore TOPOLOGY/shallow-struct — output bag returned from the pipeline; no in-file behavior by design
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
    /// Set when the pipeline is operating in degraded mode (LLM unavailable).
    ///
    /// `None` on all normal turns. `Some` only when the execute stage fell back
    /// to a cached distillation or an honest "unavailable" message.
    /// The TUI and API use this to render a warning banner instead of a normal
    /// response bubble.
    pub degraded: Option<crate::degraded_mode::DegradedMode>,
    /// Reasoning or thinking blocks generated by the model during this turn.
    pub reasoning: String,
    /// Observed model identifier that served the turn.
    ///
    /// WHY: captured from the successful request model at execute time so
    /// `after_action` can record the correct provider in the empirical store
    /// without re-running routing logic at finalize time.
    pub model_used: String,
    /// Observed provider instance that served the turn.
    ///
    /// `None` for degraded turns that did not receive provider output.
    pub provider_used: Option<String>,
    /// Opaque effective tool-surface hash refs observed during this turn.
    pub tool_surface_hashes: Vec<String>,
}

impl TurnResult {
    /// Returns `true` when the pipeline fell back to degraded mode this turn.
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        self.degraded.is_some()
    }
}

/// Re-export so callers can use `pipeline::DegradedMode` as the canonical path.
pub use crate::degraded_mode::DegradedMode;

/// A tool call made during a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool call ID.
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
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
    /// Approval outcome applied before execution, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<String>,
    /// HMAC-SHA256 receipt for hallucination-resistant attestation.
    pub receipt: Option<String>,
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
    assemble_context_conditional(
        oikos,
        nous_config,
        pipeline_config,
        ctx,
        Vec::new(),
        TaskHint::General,
        0,
    )
    .await
}

/// Assemble bootstrap context with extra sections from domain packs.
///
/// Uses [`TaskHint::General`] which loads all workspace files. Use
/// [`assemble_context_conditional`] for task-aware loading.
///
/// # Cancel safety
///
/// Not cancel-safe. Delegates to [`assemble_context_conditional`].
/// If cancelled after partial context assembly, the `PipelineContext`
/// may contain incomplete state. The pipeline runs this stage to
/// completion and checks the budget afterward.
#[instrument(skip_all, fields(nous_id = %nous_config.id))]
pub async fn assemble_context_with_extra(
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_sections: Vec<BootstrapSection>,
) -> crate::error::Result<()> {
    assemble_context_conditional(
        oikos,
        nous_config,
        pipeline_config,
        ctx,
        extra_sections,
        TaskHint::General,
        0,
    )
    .await
}

/// Assemble bootstrap context with conditional file loading.
///
/// Only workspace files relevant to the given [`TaskHint`] are loaded.
/// Identity-tier files always load; operational files load based on the hint.
///
/// `turn_number` controls cold-start recipe selection: turn 1 selects the
/// [`ColdStart`](crate::bootstrap::LlmRecipe::ColdStart) recipe, which gives
/// L1 workspace content Required priority. Other turns select the recipe
/// implied by `task_hint`.
///
/// # Cancel safety
///
/// Not cancel-safe. If cancelled after the bootstrap assembler has
/// partially written to `ctx`, the context will be in an inconsistent
/// state. Callers should not use this in `select!` branches.
///
/// The pipeline honors this by running the context stage to completion
/// and checking the stage budget afterward; the future is never dropped
/// mid-operation.
#[instrument(skip_all, fields(nous_id = %nous_config.id, ?task_hint, turn_number))]
pub async fn assemble_context_conditional(
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_sections: Vec<BootstrapSection>,
    task_hint: TaskHint,
    turn_number: u64,
) -> crate::error::Result<()> {
    let is_cold_start = turn_number == 1;
    assemble_context_conditional_with_cache(
        oikos,
        nous_config,
        pipeline_config,
        ctx,
        extra_sections,
        task_hint,
        turn_number,
        crate::bootstrap::LlmRecipe::from_task_hint(task_hint, is_cold_start),
        None,
    )
    .await
}

/// Variant of [`assemble_context_conditional`] that accepts a shared
/// [`BootstrapFileCache`].
///
/// When `cache` is `Some`, workspace file reads are served from the cache when
/// they are fresh (mtime unchanged and within TTL), eliminating redundant disk
/// reads across pipeline turns (#3388). When `None`, behaviour matches the
/// legacy path that re-reads every file every turn.
///
/// `turn_number` is forwarded to recipe selection so that turn 1 selects the
/// cold-start recipe even when the caller already knows the desired recipe.
#[instrument(skip_all, fields(nous_id = %nous_config.id, ?task_hint, turn_number))]
#[expect(
    clippy::too_many_arguments,
    reason = "recipe parameter required for explicit cold-start/refactor control (#3366)"
)]
pub async fn assemble_context_conditional_with_cache(
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_sections: Vec<BootstrapSection>,
    task_hint: TaskHint,
    turn_number: u64,
    recipe: crate::bootstrap::LlmRecipe,
    cache: Option<&crate::bootstrap::BootstrapFileCache>,
) -> crate::error::Result<()> {
    let recipe = if turn_number == 1 {
        crate::bootstrap::LlmRecipe::ColdStart
    } else {
        recipe
    };
    let mut budget = TokenBudget::new(
        u64::from(nous_config.generation.context_window),
        pipeline_config.history_budget_ratio,
        u64::from(nous_config.generation.max_output_tokens),
        u64::from(nous_config.generation.bootstrap_max_tokens),
    );

    let mut assembler = BootstrapAssembler::new(oikos);
    if let Some(cache) = cache {
        assembler = assembler.with_cache(cache);
    }
    assembler = assembler
        .with_private_workspace(nous_config.private)
        .with_llm_recipe(recipe);
    let result = assembler
        .assemble_conditional_with_recipe(
            &nous_config.id,
            &mut budget,
            extra_sections,
            task_hint,
            recipe,
        )
        .await?;

    ctx.system_prompt = Some(result.system_prompt);
    #[expect(
        clippy::cast_possible_wrap,
        clippy::as_conversions,
        reason = "u64→i64: budget fits in i64 for practical context windows"
    )]
    {
        ctx.remaining_tokens = budget.remaining() as i64; // kanon:ignore RUST/as-cast
        ctx.history_budget = budget.adjusted_history_budget() as i64; // kanon:ignore RUST/as-cast
    }

    Ok(())
}

/// Guard stage: enforce the per-session token cap.
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
/// Stages: context, recall, history, microcompact, full-compact, guard, execute, finalize.
///
/// The [`EventEmitter`] couples metrics and logs: each stage emits a single
/// typed event that simultaneously records a metric and produces a structured
/// log line. Pass `None` to use the production metrics-backed emitter.
///
/// The pipeline uses a mutable `PipelineContext` passed between stages
/// rather than returning values. This allows each stage to build on the
/// work of previous stages (e.g., recall uses remaining tokens after context).
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
    knowledge_store: Option<Arc<KnowledgeStore>>,
    session_store: Option<&Mutex<SessionStore>>,
    extra_bootstrap: Vec<BootstrapSection>,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    approval_gate: Option<&crate::approval::ApprovalGate>,
    emitter: Option<&EventEmitter>,
    hooks: Option<&HookRegistry>,
    bootstrap_cache: Option<&crate::bootstrap::BootstrapFileCache>,
    audit_log: Option<&crate::audit::PromptAuditLog>,
) -> error::Result<TurnResult> {
    let default_emitter = crate::metrics::pipeline_event_emitter();
    let emitter = emitter.unwrap_or(&default_emitter);

    let pipeline_start = Instant::now();
    let _total_timeout = if pipeline_config.stage_budget.total_secs > 0 {
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
        pipeline.configured_model = %config.generation.model,
    );
    // WHY: span.enter() must not be held across .await points — it uses
    // thread-local storage that breaks when the future migrates threads.
    // Wrapping the async body with .instrument() sets the span correctly
    // for each poll without holding a guard across suspension points.
    async move {
        let mut stages_completed: u32 = 0;
        let mut time_budget = TimeBudget::new(pipeline_config.stage_budget.clone());
        enforce_turn_time_budget(&time_budget, config, "turn_start", emitter)?;

        // WHY: fire session_start hooks at the beginning of the pipeline.
        // If this is a new session (turn == 1), hooks can initialize session-level state.
        if let Some(hook_registry) = hooks
            && input.session.turn == 1
        {
            let now = jiff::Timestamp::now().to_string();
            let session_start_ctx = SessionStartContext {
                nous_id: &config.id,
                session_key: &input.session.session_key,
                timestamp: &now,
            };
            // kanon:ignore RUST/no-silent-result-swallow — hook failure must not abort the turn
            let _ = hook_registry.run_session_start(&session_start_ctx).await;
        }

        let mut ctx = PipelineContext::default();
        let task_hint = classify_task_hint(&input.content);
        let recipe =
            crate::bootstrap::LlmRecipe::from_task_hint(task_hint, input.session.turn == 1);

        // WHY(#4713): context assembly is not cancel-safe, so we run it to
        // completion and only check the stage budget afterward. The future is
        // never dropped mid-operation.
        time_budget.begin_stage("context");
        run_context_stage(
            oikos,
            config,
            pipeline_config,
            &mut ctx,
            extra_bootstrap,
            task_hint,
            input.session.turn,
            recipe,
            bootstrap_cache,
            emitter,
        )
        .await?;
        let context_status = if time_budget.stage_exceeded("context") {
            let limit = time_budget
                .stage_limit("context")
                .unwrap_or_default();
            let timeout_secs = u32::try_from(limit.as_secs()).unwrap_or(u32::MAX);
            crate::metrics::record_error(&config.id, "context", "timeout");
            emitter.emit(&events::StageTimeout {
                nous_id: config.id.to_string(),
                stage: "context",
                timeout_secs,
            });
            crate::budget::StageTimingStatus::TimedOut
        } else {
            crate::budget::StageTimingStatus::Completed
        };
        let context_timed_out = context_status == crate::budget::StageTimingStatus::TimedOut;
        time_budget.end_stage(context_status);
        if context_timed_out {
            return Err(error::PipelineTimeoutSnafu {
                stage: "context",
                timeout_secs: time_budget
                    .stage_limit("context")
                    .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX)),
            }
            .build());
        }
        stages_completed += 1;

        time_budget.begin_stage("triage");
        let triage_result = triage::TriageStage::classify(&input.content);
        tracing::info!(
            intent = %triage_result.intent,
            sensitivity = triage_result.sensitivity.as_str(),
            tier = %triage_result.tier,
            input_len = triage_result.input_len,
            "pre_llm_triage"
        );
        ctx.triage_result = Some(triage_result);
        time_budget.end_stage(crate::budget::StageTimingStatus::Completed);
        stages_completed += 1;

        run_stage_with_timeout(
            config,
            "recall",
            &mut time_budget,
            emitter,
            run_recall_stage(
                config,
                pipeline_config,
                &mut ctx,
                &input.content,
                embedding_provider,
                vector_search,
                text_search,
                providers,
                emitter,
                // WHY: pass the session surprise prior (already advanced by this
                // turn, actor-side) for read-only per-candidate scoring. None
                // when surprise scoring is inert, so no clone cost in the common
                // case.
                (config.recall.surprise_weight > f64::EPSILON)
                    .then(|| input.session.surprise_calculator.clone()),
            ),
        )
        .await?;
        stages_completed += 1;

        run_stage_with_timeout(
            config,
            "history",
            &mut time_budget,
            emitter,
            run_history_stage(config, pipeline_config, &mut ctx, &input, session_store, emitter),
        )
        .await?;
        stages_completed += 1;

        // WHY: Fire before_compact hooks before any compaction happens.
        let pre_compact_tokens: u64 = ctx
            .messages
            .iter()
            .map(|m| u64::try_from(m.token_estimate.max(0)).unwrap_or(0))
            .sum();
        let pre_compact_message_count = ctx.messages.len();

        if let Some(hook_registry) = hooks {
            let compact_ctx = CompactionContext {
                nous_id: &config.id,
                messages_distilled: pre_compact_message_count,
                tokens_before: pre_compact_tokens,
                tokens_after: 0, // Not known until after compaction
                distillation_number: 1,
            };
            // kanon:ignore RUST/no-silent-result-swallow — hook failure must not abort the turn
            let _ = hook_registry.run_before_compact(&compact_ctx).await;
        }

        time_budget.begin_stage("microcompact");
        run_microcompact_stage(config, &mut ctx, emitter);
        let micro_status = if time_budget.stage_exceeded("microcompact") {
            emitter.emit(&events::StageTimeout {
                nous_id: config.id.to_string(),
                stage: "microcompact",
                timeout_secs: time_budget
                    .stage_limit("microcompact")
                    .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX)),
            });
            crate::budget::StageTimingStatus::TimedOut
        } else {
            crate::budget::StageTimingStatus::Completed
        };
        time_budget.end_stage(micro_status);
        stages_completed += 1;

        run_stage_with_timeout(
            config,
            "full_compact",
            &mut time_budget,
            emitter,
            run_full_compact_stage(config, &mut ctx, providers, emitter),
        )
        .await?;
        stages_completed += 1;

        // WHY: Fire after_compact hooks after compaction completes.
        if let Some(hook_registry) = hooks {
            let tokens_after: u64 = ctx
                .messages
                .iter()
                .map(|m| u64::try_from(m.token_estimate.max(0)).unwrap_or(0))
                .sum();
            let compact_ctx = CompactionContext {
                nous_id: &config.id,
                messages_distilled: ctx.messages.len(),
                tokens_before: pre_compact_tokens,
                tokens_after,
                distillation_number: 1,
            };
            hook_registry.run_after_compact(&compact_ctx).await;
        }

        run_stage_with_timeout(config, "guard", &mut time_budget, emitter, async {
            run_guard_stage(&input.session, config, emitter)
        })
        .await?;
        stages_completed += 1;

        // WHY: before-query hooks run after guard (so rejected requests never reach
        // hooks) but before execute (so hooks can modify context before the model call).
        enforce_turn_time_budget(&time_budget, config, "execute", emitter)?;
        if let Some(hook_registry) = hooks {
            let mut query_ctx = QueryContext {
                pipeline: &mut ctx,
                nous_id: &config.id,
                session_id: &input.session.id,
                turn_number: input.session.turn,
                user_message: &input.content,
            };
            if let crate::hooks::HookResult::Abort { reason } =
                hook_registry.run_before_query(&mut query_ctx).await
            {
                return Err(error::GuardRejectedSnafu { reason }.build());
            }
        }

        let result = run_execute_stage(
            config,
            pipeline_config,
            &ctx,
            &input,
            providers,
            tools,
            tool_ctx,
            stream_tx,
            approval_gate,
            &mut time_budget,
            emitter,
            hooks,
            session_store,
            audit_log,
        )
        .await?;
        stages_completed += 1;

        let finalize_outcome = run_stage_with_timeout(
            config,
            "finalize",
            &mut time_budget,
            emitter,
            async { Ok(run_finalize_stage(config, &input, &result, session_store, emitter).await) },
        )
        .await
        .unwrap_or(FinalizeOutcome::Failed);
        if finalize_outcome == FinalizeOutcome::Failed {
            tracing::warn!(nous_id = %config.id, "finalize failed; training/DPO capture suppressed for this turn");
        }
        stages_completed += 1;

        enforce_turn_time_budget(&time_budget, config, "reflection", emitter)?;
        run_stage_with_timeout(
            config,
            "reflection",
            &mut time_budget,
            emitter,
            run_reflection_stage(
                config,
                pipeline_config,
                &mut ctx,
                knowledge_store,
                Some(&input.session.id),
                emitter,
            ),
        )
        .await?;
        stages_completed += 1;

        // WHY: training capture runs after finalize (and optional reflection) so only persisted, successful
        // turns enter the training corpus. Errors are logged, never propagated:
        // training capture must never block the pipeline.
        //
        // Episteme labels are computed once and shared by both the training
        // capture and DPO extraction paths.
        let (turn_classification, correction_signal, fact_type) =
            if pipeline_config.training.enabled {
                (
                    mneme::extract::refinement::classify_turn(&input.content),
                    mneme::extract::refinement::detect_correction(&input.content),
                    mneme::extract::refinement::classify_fact(&input.content),
                )
            } else {
                (
                    mneme::extract::refinement::TurnType::Discussion,
                    mneme::extract::refinement::CorrectionSignal {
                        is_correction: false,
                        confidence_boost: 0.0,
                    },
                    mneme::extract::refinement::FactType::Observation,
                )
            };

        if pipeline_config.training.enabled && finalize_outcome == FinalizeOutcome::Persisted {
            let turn_id = input.session.turn_id.to_string();
            match crate::training::TrainingCapture::new(oikos.root(), &pipeline_config.training) {
                Ok(mut capture) => {
                    // NOTE: one entry per tool call with success/error
                    // classification and timing — feeds the DPO/ORPO reward
                    // signal (#3417).
                    let tool_outcomes = if result.tool_calls.is_empty() {
                        None
                    } else {
                        Some(
                            result
                                .tool_calls
                                .iter()
                                .map(|tc| crate::training::ToolOutcome {
                                    name: tc.name.clone(),
                                    success: !tc.is_error,
                                    duration_ms: tc.duration_ms,
                                    // WHY: tool results are free-form
                                    // strings. Extract a short stable
                                    // label by taking the first
                                    // whitespace-separated token (capped
                                    // at 32 chars) so downstream training
                                    // jobs can bucket errors without
                                    // parsing prose. Full result text is
                                    // never stored here — it would defeat
                                    // the PII filter's purpose.
                                    error_kind: if tc.is_error {
                                        tc.result.as_ref().map(|r| {
                                            let first = r
                                                .split_whitespace()
                                                .next()
                                                .unwrap_or("error")
                                                .trim_end_matches(':');
                                            first.chars().take(32).collect::<String>()
                                        })
                                    } else {
                                        None
                                    },
                                })
                                .collect(),
                        )
                    };

                    // WHY(#3418): per-fact entries are left empty because the
                    // injected `recall_section` is not structured at the
                    // pipeline boundary today.
                    let recall_signals = ctx.recall_result.as_ref().map(|r| {
                        let candidates_found =
                            u32::try_from(r.candidates_found).unwrap_or(u32::MAX);
                        let results_injected =
                            u32::try_from(r.results_injected).unwrap_or(u32::MAX);
                        crate::training::RecallSignals {
                            candidates_found,
                            results_injected,
                            tokens_consumed: r.tokens_consumed,
                            facts: Vec::new(),
                        }
                    });

                    capture.maybe_capture(crate::training::CaptureInput {
                        session_id: &input.session.id,
                        nous_id: &config.id,
                        user_message: &input.content,
                        assistant_response: &result.content,
                        model: &result.model_used,
                        tokens: result.usage.total_tokens(),
                        stop_reason: crate::training::CaptureStopReason::parse(&result.stop_reason),
                        has_tool_calls: !result.tool_calls.is_empty(),
                        turn_type: Some(turn_classification.to_string()),
                        is_correction: Some(correction_signal.is_correction),
                        fact_types: Some(vec![fact_type.to_string()]),
                        tool_outcomes,
                        recall_signals,
                        tool_surface_hashes: &result.tool_surface_hashes,
                        turn_id: Some(turn_id.as_str()),
                        turn_seq: input.session.turn,
                        capture_policy_ref: Some("nous-training-capture-v1"),
                        finalization_status: Some("finalized"),
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "training capture initialization failed");
                }
            }
        }

        // WHY: DPO pair extraction runs after training capture so the same
        // quality-filtered turn data feeds both pipelines. Uses a global
        // extractor because the pipeline task has no persistent actor state.
        // Session IDs are ULID-based and globally unique.
        if pipeline_config.training.enabled && finalize_outcome == FinalizeOutcome::Persisted {
            // WHY(#3786): authorship gate skips agent-authored turns to prevent
            // preference pairs derived from AI-generated text.
            let dpo_passes_authorship = if pipeline_config.training.author_classifier_enabled {
                let classifier = aletheia_classify::Classifier::new();
                match classifier.classify(&input.content) {
                    Ok(probs) => {
                        let class = probs.argmax();
                        let confidence = probs.confidence();
                        let passes = class == aletheia_classify::AuthorClass::User
                            || confidence < pipeline_config.training.author_classifier_threshold;
                        if !passes {
                            crate::metrics::record_training_capture_rejected(
                                &config.id,
                                class.as_str(),
                            );
                        }
                        passes
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            session_id = %input.session.id,
                            "DPO authorship classification failed; continuing without filter"
                        );
                        true
                    }
                }
            } else {
                true
            };

            if dpo_passes_authorship {
                let dpo_dir = oikos.root().join(&pipeline_config.training.path);
                if let Ok(writer) = crate::training::DpoWriter::new(&dpo_dir)
                    && let Some(pair) = crate::training::dpo::process_turn_global(
                        &input.session.id,
                        input.session.turn,
                        &input.content,
                        &result.content,
                        correction_signal.is_correction,
                        pipeline_config.training.pii_filter_enabled,
                    )
                {
                    match writer.write_pair(&pair) {
                        Ok(()) => {
                            crate::training::dpo::record_dpo_pair_captured(&config.id);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "DPO pair write failed");
                        }
                    }
                }
            }
        }

        // WHY: on_turn_complete hooks run after finalize so audit hooks see the
        // fully persisted state. Does not short-circuit: all hooks fire.
        if let Some(hook_registry) = hooks {
            let turn_ctx = TurnContext {
                result: &result,
                nous_id: &config.id,
                session_id: &input.session.id,
                turn_number: input.session.turn,
                session_tokens: input.session.cumulative_tokens,
                reinject_identity: (input.session.turn + 1).is_multiple_of(10),
            };
            hook_registry.run_on_turn_complete(&turn_ctx).await;
        }

        if !result.tool_calls.is_empty() {
            let observations = crate::instinct::record_observations(
                &result.tool_calls,
                &input.content,
                &config.id,
                pipeline_config.project_id.as_ref(),
            );
            tracing::debug!(
                observation_count = observations.len(),
                "instinct observations recorded as ephemeral turn telemetry"
            );
        }

        if config.hooks.self_audit_enabled {
            run_post_turn_self_audit(config, &result, emitter);
        }

        run_session_tuning_proposer(config, emitter);

        let current_span = tracing::Span::current();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "u128→u64: pipeline duration fits in u64; usize→u64 for tool call count"
        )]
        {
            current_span.record(
                "pipeline.total_duration_ms",
                pipeline_start.elapsed().as_millis() as u64, // kanon:ignore RUST/as-cast
            );
        }
        current_span.record("pipeline.stages_completed", stages_completed);
        #[expect(
            clippy::as_conversions,
            reason = "usize→u64: tool call count fits in u64"
        )]
        current_span.record("pipeline.tool_calls", result.tool_calls.len() as u64); // kanon:ignore RUST/as-cast

        let duration_ms = u64::try_from(pipeline_start.elapsed().as_millis()).unwrap_or(u64::MAX);
        #[expect(
            clippy::as_conversions,
            reason = "usize→u64: tool call count fits in u64"
        )]
        let tool_calls_count = result.tool_calls.len() as u64; // kanon:ignore RUST/as-cast
        emitter.emit(&events::TurnCompleted {
            nous_id: config.id.to_string(),
            model: result.model_used.clone(),
            provider: result.provider_used.clone(),
            duration_ms,
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
            tool_calls: tool_calls_count,
            stages_completed,
        });

        Ok(result)
    }
    .instrument(pipeline_span)
    .await
}

fn run_post_turn_self_audit(config: &NousConfig, result: &TurnResult, emitter: &EventEmitter) {
    let mut auditor = crate::self_audit::SelfAuditor::new();
    auditor.register_defaults();
    let ctx = crate::self_audit::CheckContext {
        nous_id: config.id.to_string(),
        recent_tool_calls: result
            .tool_calls
            .iter()
            .map(|call| crate::self_audit::ToolCallRecord {
                tool_name: call.name.clone(),
                success: !call.is_error,
            })
            .collect(),
        recent_response_lengths: vec![result.content.len()],
        ..crate::self_audit::CheckContext::default()
    };
    let report = auditor.run_audit(
        &ctx,
        crate::self_audit::AuditTrigger::EventBased { after_n_actions: 1 },
    );
    let findings = report
        .results
        .iter()
        .filter(|check| check.result.status != crate::self_audit::CheckStatus::Pass)
        .count();
    emitter.emit(&events::SelfAuditCompleted {
        nous_id: config.id.to_string(),
        checks: report.results.len(),
        findings,
    });
}

fn run_session_tuning_proposer(config: &NousConfig, emitter: &EventEmitter) {
    if !config.behavior.tuning_eligible {
        return;
    }
    let proposer = crate::tuning::TuningProposer::new(taxis::config::TuningConfig {
        enabled: true,
        ..taxis::config::TuningConfig::default()
    });
    let outcomes = proposer.evaluate(&[], &config.id);
    emitter.emit(&events::TuningProposalsEvaluated {
        nous_id: config.id.to_string(),
        outcomes: outcomes.len(),
    });
}

fn enforce_turn_time_budget(
    budget: &TimeBudget,
    config: &NousConfig,
    stage: &'static str,
    emitter: &EventEmitter,
) -> error::Result<()> {
    let total_secs = config_stage_total_secs(budget);
    if budget.total_exceeded() {
        crate::metrics::record_error(&config.id, stage, "total_timeout");
        emitter.emit(&events::StageTimeout {
            nous_id: config.id.to_string(),
            stage,
            timeout_secs: total_secs,
        });
        return Err(error::PipelineTimeoutSnafu {
            stage,
            timeout_secs: total_secs,
        }
        .build());
    }

    if total_secs > 0 {
        let remaining = budget.total_remaining().as_secs();
        let elapsed = u64::from(total_secs).saturating_sub(remaining);
        if elapsed.saturating_mul(100) >= u64::from(total_secs).saturating_mul(80) {
            warn!(
                nous_id = %config.id,
                stage,
                elapsed_secs = elapsed,
                total_secs,
                "turn time budget over 80%"
            );
        }
    }

    Ok(())
}

fn config_stage_total_secs(budget: &TimeBudget) -> u32 {
    let remaining = budget.total_remaining().as_secs();
    if remaining == u64::MAX {
        0
    } else {
        u32::try_from(budget.total_elapsed().as_secs().saturating_add(remaining))
            .unwrap_or(u32::MAX)
    }
}

async fn run_stage_with_timeout<T, F>(
    config: &NousConfig,
    stage: &'static str,
    time_budget: &mut TimeBudget,
    emitter: &EventEmitter,
    fut: F,
) -> error::Result<T>
where
    F: Future<Output = error::Result<T>>,
{
    time_budget.begin_stage(stage);
    let timeout = time_budget.stage_limit(stage);

    let result = if let Some(limit) = timeout {
        match tokio::time::timeout(limit, fut).await {
            Ok(result) => result,
            Err(_elapsed) => {
                crate::metrics::record_error(&config.id, stage, "timeout");
                emitter.emit(&events::StageTimeout {
                    nous_id: config.id.to_string(),
                    stage,
                    timeout_secs: u32::try_from(limit.as_secs()).unwrap_or(u32::MAX),
                });
                Err(error::PipelineTimeoutSnafu {
                    stage,
                    timeout_secs: u32::try_from(limit.as_secs()).unwrap_or(u32::MAX),
                }
                .build())
            }
        }
    } else {
        fut.await
    };

    let status = if matches!(result, Err(error::Error::PipelineTimeout { .. })) {
        crate::budget::StageTimingStatus::TimedOut
    } else {
        crate::budget::StageTimingStatus::Completed
    };
    time_budget.end_stage(status);
    result
}

/// Typed pipeline events for the internal event system.
pub(crate) mod events;

/// Pre-LLM triage stage: intent, sensitivity, and complexity classification.
pub mod triage;

/// Context stage: assemble bootstrap and system prompt.
mod stages;

use stages::{
    FinalizeOutcome, run_context_stage, run_execute_stage, run_finalize_stage,
    run_full_compact_stage, run_guard_stage, run_history_stage, run_microcompact_stage,
    run_recall_stage, run_reflection_stage,
};

#[cfg(test)]
#[path = "pipeline_tests/mod.rs"]
mod tests;
