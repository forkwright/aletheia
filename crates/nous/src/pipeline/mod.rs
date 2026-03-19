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

/// Loop detector: tracks repeated tool call patterns with a capped ring buffer.
#[derive(Debug, Clone)]
pub struct LoopDetector {
    /// Recent tool call signatures (ring buffer, capped at `window` entries).
    history: VecDeque<String>,
    /// Threshold for identical consecutive calls.
    threshold: u32,
    /// Maximum history entries retained.
    window: usize,
}

const DEFAULT_LOOP_WINDOW: usize = 50;

/// Maximum cycle length tested during the cycle-detection pass.
///
/// Patterns longer than this are not detected. Limiting the scan keeps
/// detection O(`CYCLE_DETECTION_MAX_LEN` × threshold) per call, which is
/// negligible for practical threshold and cycle-length values.
const CYCLE_DETECTION_MAX_LEN: usize = 10;

impl LoopDetector {
    /// Create a new loop detector with the default window size (50).
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
    /// Returns `Some(pattern)` if a loop is detected: either N consecutive
    /// identical calls, or a repeating sequence of length
    /// 2–`CYCLE_DETECTION_MAX_LEN` repeated at least N times, where
    /// N = threshold. This catches both single-tool hammering and longer
    /// cycles such as A → B → C → A.
    pub fn record(&mut self, tool_name: &str, input_hash: &str) -> Option<String> {
        let signature = format!("{tool_name}:{input_hash}");
        self.history.push_back(signature.clone());

        if self.history.len() > self.window {
            self.history.pop_front();
        }

        let n = self.history.len();
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold is a small constant, fits in usize"
        )]
        let t = self.threshold as usize;

        let recent = self.history.iter().rev().take(t);
        let all_same = recent.clone().count() >= t && recent.clone().all(|s| *s == signature);
        if all_same {
            return Some(signature);
        }

        // NOTE: cycle detection: a cycle of length L is confirmed when the last L×threshold
        // history entries consist of exactly `threshold` repetitions of the same L-length pattern;
        // `VecDeque::get` is used to avoid allocation in the inner comparison.
        for cycle_len in 2..=CYCLE_DETECTION_MAX_LEN {
            let needed = cycle_len * t;
            if n < needed {
                continue;
            }
            let pattern_start = n - cycle_len;
            let all_match = (1..t).all(|rep| {
                let seg_start = n - cycle_len * (rep + 1);
                (0..cycle_len)
                    .all(|i| self.history.get(pattern_start + i) == self.history.get(seg_start + i))
            });
            if all_match {
                let pattern: String = self
                    .history
                    .iter()
                    .skip(pattern_start)
                    .map(String::as_str)
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
        self.history.iter().rev().take_while(|s| *s == last).count()
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
        u64::from(nous_config.context_window),
        pipeline_config.history_budget_ratio,
        u64::from(nous_config.max_output_tokens),
        u64::from(nous_config.bootstrap_max_tokens),
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
        ctx.remaining_tokens = budget.remaining() as i64;
        ctx.history_budget = budget.history_budget() as i64;
    }

    Ok(())
}

/// Guard stage: check rate limits, loop detection, safety.
///
/// Enforces the per-session token spending cap from
/// [`NousConfig::session_token_cap`]. A cap of `0` disables the check.
#[must_use]
pub fn check_guard(session: &SessionState, config: &NousConfig) -> GuardResult {
    if config.session_token_cap > 0 && session.cumulative_tokens >= config.session_token_cap {
        return GuardResult::Rejected {
            reason: format!(
                "session token budget exhausted: {} of {} tokens used",
                session.cumulative_tokens, config.session_token_cap
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
        pipeline.model = %config.model,
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
            pipeline_start.elapsed().as_millis() as u64,
        );
    }
    pipeline_span.record("pipeline.stages_completed", stages_completed);
    #[expect(
        clippy::as_conversions,
        reason = "usize→u64: tool call count fits in u64"
    )]
    pipeline_span.record("pipeline.tool_calls", result.tool_calls.len() as u64);

    // Single event emission replaces separate metrics::record_turn + tracing::info.
    crate::metrics::record_turn(&config.id);
    let duration_ms = u64::try_from(pipeline_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    #[expect(
        clippy::as_conversions,
        reason = "usize→u64: tool call count fits in u64"
    )]
    let tool_calls_count = result.tool_calls.len() as u64;
    emitter.emit(&events::TurnCompleted {
        nous_id: config.id.clone(),
        model: config.model.clone(),
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
