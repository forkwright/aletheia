//! Message processing pipeline.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::{debug, error, info_span, instrument, warn};

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::store::SessionStore;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolContext;
use aletheia_taxis::oikos::Oikos;
use tokio::sync::mpsc;

use crate::bootstrap::{BootstrapAssembler, BootstrapSection};
use crate::budget::TokenBudget;
use crate::config::{NousConfig, PipelineConfig};
use crate::error;
use crate::history::{self, HistoryConfig, HistoryResult};
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
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// Returns [`crate::error::Error::ContextAssembly`] if required workspace files
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
        reason = "budget fits in i64 for practical context windows"
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
/// Resolve (stage 4) is future work.
#[expect(
    clippy::too_many_arguments,
    reason = "pipeline threading requires all dependencies until config struct refactor"
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
) -> error::Result<TurnResult> {
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

    run_context_stage(oikos, config, pipeline_config, &mut ctx, extra_bootstrap).await?;
    stages_completed += 1;

    run_recall_stage(
        config,
        pipeline_config,
        &mut ctx,
        &input.content,
        embedding_provider,
        vector_search,
        text_search,
    )
    .await;
    stages_completed += 1;

    run_history_stage(config, &mut ctx, &input, session_store).await?;
    stages_completed += 1;

    run_guard_stage(&input.session, config)?;
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
    )
    .await?;
    stages_completed += 1;

    run_finalize_stage(config, &input, &result, session_store).await;
    stages_completed += 1;

    if !result.tool_calls.is_empty() {
        crate::instinct::record_observations(&result.tool_calls, &input.content, &config.id);
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "pipeline duration fits in u64"
    )]
    {
        pipeline_span.record(
            "pipeline.total_duration_ms",
            pipeline_start.elapsed().as_millis() as u64,
        );
    }
    pipeline_span.record("pipeline.stages_completed", stages_completed);
    pipeline_span.record("pipeline.tool_calls", result.tool_calls.len() as u64);

    crate::metrics::record_turn(&config.id);

    let duration_ms = u64::try_from(pipeline_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        input_tokens = result.usage.input_tokens,
        output_tokens = result.usage.output_tokens,
        tool_calls_count = result.tool_calls.len() as u64,
        duration_ms,
        model = %config.model,
        "turn_completed"
    );

    Ok(result)
}

/// Context stage: assemble bootstrap and system prompt.
async fn run_context_stage(
    oikos: &Oikos,
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_bootstrap: Vec<BootstrapSection>,
) -> error::Result<()> {
    let span = info_span!(
        "pipeline_stage",
        stage = "context",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = Instant::now();
    assemble_context_with_extra(oikos, config, pipeline_config, ctx, extra_bootstrap)
        .await
        .inspect_err(|_| {
            crate::metrics::record_error(&config.id, "context", "assembly_failed");
        })?;
    record_stage_duration(&span, &start);
    span.record("status", "ok");
    crate::metrics::record_stage(&config.id, "context", start.elapsed().as_secs_f64());
    Ok(())
}

/// Recall stage: retrieve relevant knowledge from vector/BM25 search.
async fn run_recall_stage(
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    content: &str,
    embedding_provider: Option<&dyn EmbeddingProvider>,
    vector_search: Option<&dyn crate::recall::VectorSearch>,
    text_search: Option<&dyn crate::recall::TextSearch>,
) {
    let span = info_span!(
        "pipeline_stage",
        stage = "recall",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = Instant::now();
    let recall_timeout_secs = pipeline_config.stage_budget.recall_secs;
    let is_mock_embedding =
        embedding_provider.is_some_and(|ep| ep.model_name() == "mock-embedding");
    #[expect(
        clippy::cast_sign_loss,
        reason = "remaining_tokens is positive after context assembly"
    )]
    let budget = ctx.remaining_tokens.max(0) as u64;

    // NOTE: BM25-only fallback when mock embedding provider is in use.
    // Vector recall would produce meaningless results from hash-based embeddings.
    if is_mock_embedding {
        if let Some(ts) = text_search {
            debug!("mock embedding provider — using BM25-only recall");
            let recall_stage = crate::recall::RecallStage::new(config.recall.clone());
            let result = recall_stage.run_bm25(content, &config.id, ts, budget);
            apply_recall_result(result, ctx, &span);
        } else {
            debug!("recall skipped: mock embedding provider with no text search");
            span.record("status", "skipped");
        }
    } else if let (Some(ep), Some(vs)) = (embedding_provider, vector_search) {
        let recall_stage = crate::recall::RecallStage::new(config.recall.clone());

        let recall_result_opt = if recall_timeout_secs > 0 {
            match tokio::time::timeout(Duration::from_secs(u64::from(recall_timeout_secs)), async {
                recall_stage.run(content, &config.id, ep, vs, budget)
            })
            .await
            {
                Ok(result) => Some(result),
                Err(_elapsed) => {
                    warn!(
                        timeout_secs = recall_timeout_secs,
                        "recall stage timed out, continuing without recalled knowledge"
                    );
                    span.record("status", "timeout");
                    None
                }
            }
        } else {
            Some(recall_stage.run(content, &config.id, ep, vs, budget))
        };

        if let Some(result) = recall_result_opt {
            apply_recall_result(result, ctx, &span);
        }
    } else {
        debug!("recall skipped: embedding provider or vector search not configured");
        span.record("status", "skipped");
    }
    record_stage_duration(&span, &start);
    crate::metrics::record_stage(&config.id, "recall", start.elapsed().as_secs_f64());
}

/// History stage: load conversation history within token budget.
async fn run_history_stage(
    config: &NousConfig,
    ctx: &mut PipelineContext,
    input: &PipelineInput,
    session_store: Option<&Mutex<SessionStore>>,
) -> error::Result<()> {
    let span = info_span!(
        "pipeline_stage",
        stage = "history",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = Instant::now();

    // NOTE: Waterfall: unused system-prompt budget flows into the history budget
    // so tokens not consumed by bootstrap or recall are not wasted.
    ctx.history_budget += ctx.remaining_tokens.max(0);

    let history_config = HistoryConfig::default();
    if let Some(store_mutex) = session_store {
        // WHY: guard scoped and dropped before execute .await
        let store = store_mutex.lock().await;
        let (messages, hist_result) = history::load_history(
            &store,
            &input.session.id,
            ctx.history_budget,
            &history_config,
            &input.content,
        )
        .inspect_err(|_| crate::metrics::record_error(&config.id, "history", "load_failed"))?;
        ctx.messages = messages;
        ctx.history_budget -= hist_result.tokens_consumed;
        ctx.history_result = Some(hist_result);
    } else {
        #[expect(clippy::cast_possible_wrap, reason = "message length fits in i64")]
        let token_estimate = (input.content.len() as i64 + 3) / 4;
        ctx.messages.push(PipelineMessage {
            role: "user".to_owned(),
            content: input.content.clone(),
            token_estimate,
        });
    }
    record_stage_duration(&span, &start);
    span.record("status", "ok");
    crate::metrics::record_stage(&config.id, "history", start.elapsed().as_secs_f64());
    Ok(())
}

/// Guard stage: check rate limits, loop detection, safety.
fn run_guard_stage(session: &SessionState, config: &NousConfig) -> error::Result<()> {
    let span = info_span!(
        "pipeline_stage",
        stage = "guard",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = Instant::now();
    let guard = check_guard(session, config);
    record_stage_duration(&span, &start);
    crate::metrics::record_stage(&config.id, "guard", start.elapsed().as_secs_f64());
    match guard {
        GuardResult::Allow => {
            span.record("status", "ok");
            Ok(())
        }
        GuardResult::RateLimited { retry_after_ms } => {
            span.record("status", "error");
            crate::metrics::record_error(&config.id, "guard", "rate_limited");
            Err(error::GuardRejectedSnafu {
                reason: format!("rate limited, retry after {retry_after_ms}ms"),
            }
            .build())
        }
        GuardResult::LoopDetected { pattern } => {
            span.record("status", "error");
            crate::metrics::record_error(&config.id, "guard", "loop_detected");
            Err(error::LoopDetectedSnafu {
                iterations: 0u32,
                pattern,
            }
            .build())
        }
        GuardResult::Rejected { reason } => {
            span.record("status", "error");
            crate::metrics::record_error(&config.id, "guard", "rejected");
            Err(error::GuardRejectedSnafu { reason }.build())
        }
    }
}

/// Execute stage: call LLM with optional timeout and streaming.
#[expect(
    clippy::too_many_arguments,
    reason = "stage receives all pipeline dependencies"
)]
async fn run_execute_stage(
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &PipelineContext,
    input: &PipelineInput,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    pipeline_start: Instant,
    total_timeout: Option<Duration>,
) -> error::Result<TurnResult> {
    let span = info_span!(
        "pipeline_stage",
        stage = "execute",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty,
        tokens_in = tracing::field::Empty,
        tokens_out = tracing::field::Empty,
    );
    let _guard = span.enter();
    let start = Instant::now();

    // NOTE: prefer per-stage budget, fall back to remaining time from total pipeline budget, whichever is tighter.
    let execute_secs = pipeline_config.stage_budget.execute_secs;
    let effective_execute_timeout = match (execute_secs > 0, total_timeout) {
        (true, Some(total)) => {
            let stage = Duration::from_secs(u64::from(execute_secs));
            let remaining = total.saturating_sub(pipeline_start.elapsed());
            Some(stage.min(remaining))
        }
        (true, None) => Some(Duration::from_secs(u64::from(execute_secs))),
        (false, Some(total)) => Some(total.saturating_sub(pipeline_start.elapsed())),
        (false, None) => None,
    };

    let execute_fut = async {
        if let Some(tx) = stream_tx {
            crate::execute::execute_streaming(
                ctx,
                &input.session,
                config,
                providers,
                tools,
                tool_ctx,
                tx,
            )
            .await
        } else {
            crate::execute::execute(ctx, &input.session, config, providers, tools, tool_ctx).await
        }
    };

    let result = if let Some(timeout_dur) = effective_execute_timeout {
        match tokio::time::timeout(timeout_dur, execute_fut).await {
            Ok(res) => res.inspect_err(|_| {
                crate::metrics::record_error(&config.id, "execute", "pipeline_error");
            })?,
            Err(_elapsed) => {
                let secs = execute_secs.max(pipeline_config.stage_budget.total_secs);
                span.record("status", "timeout");
                crate::metrics::record_error(&config.id, "execute", "timeout");
                return Err(error::PipelineTimeoutSnafu {
                    stage: "execute",
                    timeout_secs: secs,
                }
                .build());
            }
        }
    } else {
        execute_fut.await.inspect_err(|_| {
            crate::metrics::record_error(&config.id, "execute", "pipeline_error");
        })?
    };

    record_stage_duration(&span, &start);
    span.record("tokens_in", result.usage.input_tokens);
    span.record("tokens_out", result.usage.output_tokens);
    span.record("status", "ok");
    crate::metrics::record_stage(&config.id, "execute", start.elapsed().as_secs_f64());
    Ok(result)
}

/// Finalize stage: persist turn results to durable storage.
async fn run_finalize_stage(
    config: &NousConfig,
    input: &PipelineInput,
    result: &TurnResult,
    session_store: Option<&Mutex<SessionStore>>,
) {
    let span = info_span!(
        "pipeline_stage",
        stage = "finalize",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = Instant::now();
    if let Some(store_mutex) = session_store {
        let store = store_mutex.lock().await;
        let finalize_config = crate::finalize::FinalizeConfig::default();
        match crate::finalize::finalize(
            &store,
            &input.session,
            &input.content,
            result,
            &finalize_config,
        ) {
            Ok(fr) => {
                debug!(
                    messages = fr.messages_persisted,
                    usage = fr.usage_recorded,
                    "finalize complete"
                );
                span.record("status", "ok");
            }
            Err(e) => {
                error!(error = %e, "finalize failed, returning result without persistence");
                span.record("status", "error");
            }
        }
    } else {
        debug!("no session store, skipping finalize");
        span.record("status", "skipped");
    }
    record_stage_duration(&span, &start);
    crate::metrics::record_stage(&config.id, "finalize", start.elapsed().as_secs_f64());
}

/// Record elapsed duration on a pipeline stage span.
fn record_stage_duration(span: &tracing::Span, start: &Instant) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "stage duration fits in u64"
    )]
    {
        span.record("duration_ms", start.elapsed().as_millis() as u64);
    }
}

/// Apply a recall result (vector or BM25) to the pipeline context.
///
/// Appends the recall section to the system prompt and records token consumption.
fn apply_recall_result(
    result: error::Result<crate::recall::RecallStageResult>,
    ctx: &mut PipelineContext,
    span: &tracing::Span,
) {
    match result {
        Ok(recall_result) => {
            if let Some(ref section) = recall_result.recall_section {
                if let Some(ref mut prompt) = ctx.system_prompt {
                    prompt.push_str("\n\n");
                    prompt.push_str(section);
                }
                // WHY: saturating_sub followed by max(0) ensures remaining_tokens
                // never goes negative regardless of recall token accounting.
                #[expect(clippy::cast_possible_wrap, reason = "recall tokens fit in i64")]
                {
                    ctx.remaining_tokens = ctx
                        .remaining_tokens
                        .saturating_sub(recall_result.tokens_consumed as i64)
                        .max(0);
                }
            }
            ctx.recall_result = Some(recall_result);
            span.record("status", "ok");
        }
        Err(e) => {
            warn!(error = %e, "recall stage failed, continuing without recalled knowledge");
            span.record("status", "error");
        }
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
