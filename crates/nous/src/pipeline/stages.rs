//! Pipeline stage implementations.

use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, error, info_span, warn};

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::store::SessionStore;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolContext;
use aletheia_taxis::oikos::Oikos;

use crate::bootstrap::BootstrapSection;
use crate::config::{NousConfig, PipelineConfig};
use crate::error;
use crate::history::{self, HistoryConfig};
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

use super::{
    GuardResult, PipelineContext, PipelineInput, PipelineMessage, TurnResult,
    assemble_context_with_extra, check_guard,
};

pub(super) async fn run_context_stage(
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
pub(super) async fn run_recall_stage(
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
        clippy::as_conversions,
        reason = "i64→u64: remaining_tokens is positive after context assembly"
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
pub(super) async fn run_history_stage(
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
        #[expect(
            clippy::cast_possible_wrap,
            clippy::as_conversions,
            reason = "usize→i64: message length fits in i64"
        )]
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
pub(super) fn run_guard_stage(session: &SessionState, config: &NousConfig) -> error::Result<()> {
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
pub(super) async fn run_execute_stage(
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
pub(super) async fn run_finalize_stage(
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
        clippy::as_conversions,
        reason = "u128→u64: stage duration fits in u64"
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
                #[expect(
                    clippy::cast_possible_wrap,
                    clippy::as_conversions,
                    reason = "u64→i64: recall tokens fit in i64"
                )]
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
