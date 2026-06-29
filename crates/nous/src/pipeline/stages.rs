// kanon:ignore RUST/file-too-long — pipeline stage implementations; per-stage module extraction planned
//! Pipeline stage implementations.

use std::cmp::Reverse;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use sha2::{Digest as _, Sha256};
use snafu::ResultExt;
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task;
use tracing::{Instrument, debug, error, info_span, warn};

use hermeneus::provider::ProviderRegistry;
use hermeneus::types::{CompletionRequest, Content, ContentBlock, Message, Role};
use koina::event::EventEmitter;
use mneme::embedding::EmbeddingProvider;
use mneme::id::FactId;
use mneme::knowledge::{EpistemicTier, Fact};
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use organon::registry::ToolRegistry;
use organon::types::ToolContext;
use taxis::oikos::Oikos;

use crate::bootstrap::{BootstrapFileCache, BootstrapSection, LlmRecipe, TaskHint};
use crate::compact::{CompactConfig, CompactReason, map_strategy, select_prompt};
use crate::config::{NousConfig, PipelineConfig};
use crate::error;
use crate::history::{self, HistoryConfig};
use crate::hooks::registry::HookRegistry;
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

use super::events::{ReflectionOutcome, StageCompleted, StageError, StageSkipped, StageTimeout};
use super::{
    GuardResult, PipelineContext, PipelineInput, PipelineMessage, ReflectionResult,
    ReflectionStatus, TurnResult, assemble_context_conditional_with_cache, check_guard,
};

struct CachedDistillation {
    summary: String,
    source_id: Option<String>,
}

struct ProviderRecallBridge<'a> {
    providers: &'a ProviderRegistry,
    model: &'a str,
}

impl ProviderRecallBridge<'_> {
    fn complete_blocking(&self, system: &str, user_message: &str) -> Result<String, String> {
        let provider = self
            .providers
            .find_provider(self.model)
            .ok_or_else(|| format!("no provider registered for model {}", self.model))?;
        let request = CompletionRequest {
            model: self.model.to_owned(),
            system: Some(system.to_owned()),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text(user_message.to_owned()),
                cache_breakpoint: false,
            }],
            max_tokens: 512,
            temperature: Some(0.0),
            ..CompletionRequest::default()
        };
        // WHY: this method is invoked from the synchronous recall trait path
        // that now runs inside `tokio::task::spawn_blocking`. Blocking on the
        // returned future is safe there and avoids pinning a Tokio worker
        // thread for the full LLM round-trip. (#5665)
        let response = Handle::current()
            .block_on(provider.complete(&request))
            .map_err(|e| e.to_string())?;
        let text = response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            Err("provider returned no text content".to_owned())
        } else {
            Ok(text)
        }
    }
}

impl mneme::query_rewrite::RewriteProvider for ProviderRecallBridge<'_> {
    fn complete(
        &self,
        system: &str,
        user_message: &str,
    ) -> Result<String, mneme::query_rewrite::RewriteError> {
        self.complete_blocking(system, user_message)
            .map_err(mneme::query_rewrite::RewriteError::LlmCall)
    }
}

impl mneme::side_query::SideQueryRanker for ProviderRecallBridge<'_> {
    fn rank_memories(
        &self,
        query: &str,
        manifest_text: &str,
        max_results: usize,
    ) -> Result<Vec<String>, mneme::side_query::SideQueryError> {
        let system = format!(
            "Select up to {max_results} relevant memory source IDs for the query. Respond with only a JSON array of source ID strings."
        );
        let user = format!("Query: {query}\n\nMemory manifest:\n{manifest_text}");
        let text = self
            .complete_blocking(&system, &user)
            .map_err(|message| mneme::side_query::RankerFailedSnafu { message }.build())?;
        let ids: Vec<String> = serde_json::from_str(text.trim()).map_err(|e| {
            mneme::side_query::RankerFailedSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        // WHY(#5560): side-query output is a hint, not an authority. Bound it
        // to the manifest before applying `max_results`.
        let valid_ids: HashSet<String> = manifest_text
            .lines()
            .filter_map(|line| line.strip_prefix("- "))
            .filter_map(|rest| rest.split_whitespace().next())
            .map(String::from)
            .collect();
        Ok(ids
            .into_iter()
            .filter(|id| valid_ids.contains(id))
            .take(max_results)
            .collect())
    }
}

fn provider_name_for_model(providers: &ProviderRegistry, model: &str) -> Option<String> {
    providers
        .providers()
        .into_iter()
        .find(|provider| provider.match_specificity(model).is_some())
        .map(|provider| provider.name().to_owned())
}

fn cached_distillation_for_session(
    store: &SessionStore,
    session_id: &str,
) -> Result<Option<CachedDistillation>, mneme::error::Error> {
    let Some(message) = store
        .get_history_filtered(session_id, Some(1), Some(1))?
        .into_iter()
        .find(|message| message.seq == 0 && !message.is_distilled)
    else {
        return Ok(None);
    };

    Ok(Some(CachedDistillation {
        source_id: Some(format!("message:{}:{}", message.session_id, message.id)),
        summary: message.content,
    }))
}

#[expect(
    clippy::too_many_arguments,
    reason = "context stage forwards pipeline dependencies plus optional bootstrap cache"
)]
pub(super) async fn run_context_stage(
    oikos: &Oikos,
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_bootstrap: Vec<BootstrapSection>,
    task_hint: TaskHint,
    turn_number: u64,
    recipe: LlmRecipe,
    bootstrap_cache: Option<&BootstrapFileCache>,
    emitter: &EventEmitter,
) -> error::Result<()> {
    let span = info_span!(
        "pipeline_stage",
        stage = "context",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let start = Instant::now();
    assemble_context_conditional_with_cache(
        oikos,
        config,
        pipeline_config,
        ctx,
        extra_bootstrap,
        task_hint,
        turn_number,
        recipe,
        bootstrap_cache,
    )
    .instrument(span.clone())
    .await
    .inspect_err(|_| {
        emitter.emit(&StageError {
            nous_id: config.id.to_string(),
            stage: "context",
            error_type: "assembly_failed".to_owned(),
        });
    })?;
    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(&span, &start);
    span.record("status", "ok");
    emitter.emit(&StageCompleted {
        nous_id: config.id.to_string(),
        stage: "context",
        duration_secs,
    });
    Ok(())
}

/// Recall stage: retrieve relevant knowledge from vector/BM25 search.
#[expect(
    clippy::too_many_arguments,
    reason = "stage receives all search dependencies and owns recall branch lifecycle"
)]
pub(super) async fn run_recall_stage(
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    content: &str,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    text_search: Option<&dyn crate::recall::TextSearch>,
    providers: Arc<ProviderRegistry>,
    emitter: &EventEmitter,
    surprise_calc: Option<mneme::surprise::SurpriseCalculator>,
) -> error::Result<()> {
    // WHY(#3404, #3413): resolve deployment target so the sovereignty filter drops facts the provider
    // cannot receive; unregistered models default to Cloud (Public-only) rather than leaking Internal data.
    let deployment_target = providers
        .find_provider(&config.generation.model)
        .map_or(hermeneus::provider::DeploymentTarget::Cloud, |p| {
            p.deployment_target()
        });
    let span = info_span!(
        "pipeline_stage",
        stage = "recall",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let start = Instant::now();
    // WHY(#3380): both "mock-embedding" (hash-based) and "degraded-embedding" (startup failure) produce
    // meaningless vector results; skip to BM25-only at the pipeline level for both cases.
    let bm25_only = embedding_provider.is_some_and(|ep| {
        let name = ep.model_name();
        name == "mock-embedding" || name == mneme::embedding::DegradedEmbeddingProvider::MODEL_NAME
    });
    #[expect(
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "i64→u64: remaining_tokens is positive after context assembly"
    )]
    let budget = ctx.remaining_tokens.max(0) as u64; // kanon:ignore RUST/as-cast

    // NOTE: BM25-only fallback when embeddings are unavailable (mock or degraded).
    // Vector recall would produce meaningless (mock) or failing (degraded) results.
    if bm25_only {
        if let Some(ts) = text_search {
            debug!(
                provider = embedding_provider.map_or("none", EmbeddingProvider::model_name),
                "embeddings unavailable — using BM25-only recall"
            );
            let recall_stage = crate::recall::RecallStage::new(config.recall.clone())
                .with_deployment_target(deployment_target)
                .with_project_scope(project_recall_scope(pipeline_config))
                .with_surprise_calculator(surprise_calc.clone());
            let result = recall_stage.run_bm25(content, &config.id, ts, budget);
            apply_recall_result(
                result,
                ctx,
                &span,
                config.recall.late_inject_anchor,
                emitter,
                config.id.as_ref(),
            );
        } else {
            span.record("status", "skipped");
            emitter.emit(&StageSkipped {
                nous_id: config.id.to_string(),
                stage: "recall",
                reason: "mock embedding provider with no text search".to_owned(),
            });
        }
    } else if let (Some(ep), Some(vs)) = (embedding_provider, vector_search) {
        // WHY: the synchronous recall-enhancement path calls an LLM provider
        // through a synchronous trait. Running it directly on the async worker
        // thread blocks that thread for the entire network round-trip. Move the
        // whole path onto a blocking thread and release the worker. (#5665)
        let content = content.to_owned();
        let nous_id = config.id.clone();
        let model = config.generation.model.clone();
        let recall_config = config.recall.clone();
        let project_scope = project_recall_scope(pipeline_config);
        let surprise_calc = surprise_calc.clone();
        let providers = Arc::clone(&providers);
        let result = task::spawn_blocking(move || {
            let recall_stage = crate::recall::RecallStage::new(recall_config)
                .with_deployment_target(deployment_target)
                .with_project_scope(project_scope)
                .with_surprise_calculator(surprise_calc);
            let recall_bridge = ProviderRecallBridge {
                providers: &providers,
                model: model.as_str(),
            };
            recall_stage.run_with_recall_enhancements(
                &content,
                &nous_id,
                ep.as_ref(),
                vs.as_ref(),
                budget,
                Some(&recall_bridge),
                Some(&recall_bridge),
            )
        })
        .await
        .map_err(|e| {
            error::PipelineStageSnafu {
                stage: "recall".to_owned(),
                message: format!("recall enhancement task failed: {e}"),
            }
            .build()
        })?;
        apply_recall_result(
            result,
            ctx,
            &span,
            config.recall.late_inject_anchor,
            emitter,
            config.id.as_ref(),
        );
    } else {
        span.record("status", "skipped");
        emitter.emit(&StageSkipped {
            nous_id: config.id.to_string(),
            stage: "recall",
            reason: "embedding provider or vector search not configured".to_owned(),
        });
    }
    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(&span, &start);
    emitter.emit(&StageCompleted {
        nous_id: config.id.to_string(),
        stage: "recall",
        duration_secs,
    });
    Ok(())
}

fn project_recall_scope(pipeline_config: &PipelineConfig) -> mneme::recall::ProjectRecallScope {
    pipeline_config.project_id.clone().map_or(
        mneme::recall::ProjectRecallScope::Global,
        mneme::recall::ProjectRecallScope::Project,
    )
}

/// History stage: load conversation history within token budget.
pub(super) async fn run_history_stage(
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    input: &PipelineInput,
    session_store: Option<&Mutex<SessionStore>>,
    emitter: &EventEmitter,
) -> error::Result<()> {
    let span = info_span!(
        "pipeline_stage",
        stage = "history",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let start = Instant::now();

    // NOTE: Waterfall: unused system-prompt budget flows into the history budget
    // so tokens not consumed by bootstrap or recall are not wasted.
    ctx.history_budget += ctx.remaining_tokens.max(0);

    // WHY: the turn-history policy lives on PipelineConfig so operators,
    // diagnostics, and UI can inspect and tune load behavior without
    // reaching into local constants in `history.rs`.
    let history_policy = pipeline_config.history.clone();
    let history_config = HistoryConfig {
        max_messages: history_policy.max_messages,
        reserve_for_current: history_policy.reserve_for_current,
        include_tool_messages: history_policy.include_tool_messages,
    };
    if let Some(store_mutex) = session_store {
        let store = store_mutex.lock().instrument(span.clone()).await;
        let (messages, mut hist_result) = history::load_history(
            &store,
            &input.session.id,
            ctx.history_budget,
            &history_config,
            &input.content,
        )
        .inspect_err(|_| {
            emitter.emit(&StageError {
                nous_id: config.id.to_string(),
                stage: "history",
                error_type: "load_failed".to_owned(),
            });
        })?;
        // WHY: surface the effective policy in the run record so reviewers can
        // explain inclusion/exclusion decisions without re-reading config.
        hist_result.policy = history_policy;
        ctx.messages = messages;
        ctx.history_budget -= hist_result.tokens_consumed;
        ctx.history_result = Some(hist_result);
    } else {
        #[expect(
            clippy::cast_possible_wrap,
            clippy::as_conversions,
            reason = "usize→i64: message length fits in i64"
        )]
        let token_estimate = (input.content.len() as i64 + 3) / 4; // kanon:ignore RUST/as-cast
        ctx.messages.push(PipelineMessage::text(
            "user",
            input.content.clone(),
            token_estimate,
        ));
    }
    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(&span, &start);
    span.record("status", "ok");
    emitter.emit(&StageCompleted {
        nous_id: config.id.to_string(),
        stage: "history",
        duration_secs,
    });
    Ok(())
}

/// Microcompaction stage: clear expired tool results in-place.
///
/// Runs every turn as a cheap synchronous pass. Replaces tool results older
/// than their per-type TTL with cleared markers, preserving the last N results
/// per tool type. No-op when no tool results are expired.
pub(super) fn run_microcompact_stage(
    config: &NousConfig,
    ctx: &mut PipelineContext,
    emitter: &EventEmitter,
) {
    let span = info_span!(
        "pipeline_stage",
        stage = "microcompact",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty,
        results_cleared = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = Instant::now();

    let compact_config = CompactConfig::default();
    let now = jiff::Timestamp::now();
    let metrics =
        crate::compact::micro::run_microcompaction(&mut ctx.messages, &compact_config, now);

    span.record("results_cleared", metrics.results_cleared);

    if metrics.results_cleared > 0 {
        // NOTE: update history budget with reclaimed tokens
        #[expect(
            clippy::cast_possible_wrap,
            clippy::as_conversions,
            reason = "u64→i64: reclaimed tokens fit in i64"
        )]
        {
            ctx.history_budget += metrics.tokens_reclaimed() as i64; // kanon:ignore RUST/as-cast
        }
        ctx.compaction_metrics = Some(metrics);
        span.record("status", "ok");
    } else {
        span.record("status", "noop");
    }

    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(&span, &start);
    emitter.emit(&StageCompleted {
        nous_id: config.id.to_string(),
        stage: "microcompact",
        duration_secs,
    });
}

/// Full compaction stage: check threshold and summarize via the configured LLM.
///
/// Checks whether token usage exceeds the configured threshold. If so, it asks
/// the selected provider for a compaction summary and falls back to a structural
/// summary only when no provider is available or the provider call fails.
///
/// No-op when token usage is below threshold.
pub(super) async fn run_full_compact_stage(
    config: &NousConfig,
    ctx: &mut PipelineContext,
    providers: &ProviderRegistry,
    emitter: &EventEmitter,
) -> error::Result<()> {
    let span = info_span!(
        "pipeline_stage",
        stage = "full_compact",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = Instant::now();

    let compact_config = CompactConfig {
        strategy: map_strategy(config.behavior.compaction_strategy),
        ..CompactConfig::default()
    };
    let context_window = u64::from(config.generation.context_window);

    // NOTE: estimate consumed tokens from messages in context
    #[expect(
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "i64→u64: token estimates are non-negative in practice"
    )]
    let consumed: u64 = ctx
        .messages
        .iter()
        .map(|m| m.token_estimate.max(0) as u64) // kanon:ignore RUST/as-cast
        .sum();

    if !crate::compact::full::should_trigger(consumed, context_window, &compact_config) {
        span.record("status", "noop");
        let duration_secs = start.elapsed().as_secs_f64();
        record_stage_duration(&span, &start);
        emitter.emit(&StageSkipped {
            nous_id: config.id.to_string(),
            stage: "full_compact",
            reason: format!("token usage {consumed}/{context_window} below threshold"),
        });
        emitter.emit(&StageCompleted {
            nous_id: config.id.to_string(),
            stage: "full_compact",
            duration_secs,
        });
        return Ok(());
    }

    let critical_files =
        crate::compact::full::identify_critical_files(&ctx.messages, &compact_config);
    let prompt = select_prompt(CompactReason::TokenBudget);
    let (request, preserved) =
        crate::compact::full::build_summary_request(&ctx.messages, &compact_config, prompt);

    let mut fallback_used = false;
    let summary = match compact_with_llm(config, providers, request).await {
        Ok(summary) => summary,
        Err(error) => {
            fallback_used = true;
            warn!(
                error = %error,
                nous_id = %config.id,
                "full compaction LLM call failed; using structural fallback"
            );
            build_structural_summary(&ctx.messages, &compact_config)
        }
    };

    let result = crate::compact::full::apply_compaction(
        &summary,
        preserved,
        critical_files,
        consumed,
        &compact_config,
    );

    ctx.messages = result.messages;
    ctx.compaction_metrics = Some(result.metrics);

    if fallback_used {
        span.record("status", "degraded");
        emitter.emit(&StageError {
            nous_id: config.id.to_string(),
            stage: "full_compact",
            error_type: "llm_fallback".to_owned(),
        });
    } else {
        span.record("status", "ok");
    }

    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(&span, &start);
    emitter.emit(&StageCompleted {
        nous_id: config.id.to_string(),
        stage: "full_compact",
        duration_secs,
    });
    Ok(())
}

async fn compact_with_llm(
    config: &NousConfig,
    providers: &ProviderRegistry,
    request_text: String,
) -> error::Result<String> {
    let model = &config.generation.model;
    let Some(provider) = providers.find_provider(model) else {
        return Err(hermeneus::error::UnsupportedModelSnafu {
            model: model.clone(),
        }
        .build())
        .context(error::LlmSnafu);
    };

    let request = CompletionRequest {
        model: model.clone(),
        system: Some("Summarize this conversation for context compaction. Preserve decisions, open tasks, file paths, and unresolved risks.".to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text(request_text),
            cache_breakpoint: false,
        }],
        max_tokens: config.generation.max_output_tokens,
        temperature: Some(0.0),
        ..CompletionRequest::default()
    };

    let response = provider.complete(&request).await.context(error::LlmSnafu)?;
    let summary = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if summary.trim().is_empty() {
        return Err(hermeneus::error::ApiRequestSnafu {
            message: "compaction provider returned empty summary".to_owned(),
        }
        .build())
        .context(error::LlmSnafu);
    }
    Ok(summary)
}

/// Build a structural summary without a model call.
///
/// Extracts key information from messages: tool calls, decisions, file paths.
/// Used as a fallback until the task registry enables background model calls.
fn build_structural_summary(messages: &[PipelineMessage], config: &CompactConfig) -> String {
    use std::fmt::Write;

    let preserve_count = config.preserve_turns.min(messages.len());
    let split_point = messages.len().saturating_sub(preserve_count);
    let to_summarize = messages.get(..split_point).unwrap_or(&[]);

    let mut summary = String::from("Previous conversation context:\n");
    let mut turn_count = 0;

    for msg in to_summarize {
        // NOTE: include truncated content to preserve key context
        let truncated: String = msg.content.chars().take(200).collect();
        let role = &msg.role;
        // kanon:ignore RUST/no-silent-result-swallow — write! on String is infallible
        let _ = write!(summary, "- [{role}] {truncated}");
        if msg.content.len() > 200 {
            summary.push_str("...");
        }
        summary.push('\n');
        turn_count += 1;
    }

    // kanon:ignore RUST/no-silent-result-swallow — write! on String is infallible
    let _ = write!(summary, "\n({turn_count} messages summarized)");
    summary
}

/// Guard stage: enforce the per-session token cap.
pub(super) fn run_guard_stage(
    session: &SessionState,
    config: &NousConfig,
    emitter: &EventEmitter,
) -> error::Result<()> {
    let span = info_span!(
        "pipeline_stage",
        stage = "guard",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = Instant::now();
    let guard = check_guard(session, config);
    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(&span, &start);
    match guard {
        GuardResult::Allow => {
            span.record("status", "ok");
            emitter.emit(&StageCompleted {
                nous_id: config.id.to_string(),
                stage: "guard",
                duration_secs,
            });
            Ok(())
        }
        GuardResult::RateLimited { retry_after_ms } => {
            span.record("status", "error");
            emitter.emit(&StageError {
                nous_id: config.id.to_string(),
                stage: "guard",
                error_type: "rate_limited".to_owned(),
            });
            Err(error::GuardRejectedSnafu {
                reason: format!("rate limited, retry after {retry_after_ms}ms"),
            }
            .build())
        }
        GuardResult::LoopDetected { pattern } => {
            span.record("status", "error");
            emitter.emit(&StageError {
                nous_id: config.id.to_string(),
                stage: "guard",
                error_type: "loop_detected".to_owned(),
            });
            Err(error::LoopDetectedSnafu {
                iterations: 0u32,
                pattern,
            }
            .build())
        }
        GuardResult::Rejected { reason } => {
            span.record("status", "error");
            emitter.emit(&StageError {
                nous_id: config.id.to_string(),
                stage: "guard",
                error_type: "rejected".to_owned(),
            });
            Err(error::GuardRejectedSnafu { reason }.build())
        }
    }
}

/// Execute stage: call LLM with optional cooperative deadline and streaming.
///
/// On transient LLM failures (rate limit, 5xx) this stage falls back to
/// [`crate::degraded_mode::build_degraded_response`] instead of propagating
/// the error. The happy path is unchanged.
///
/// # Cooperative timeout
///
/// This stage no longer wraps the entire execute future in
/// `tokio::time::timeout`. Instead, a deadline is passed into the execute
/// loop and observed at safe boundaries (between LLM calls and after tool
/// results have been processed) and around each LLM call. When the deadline
/// expires, execute returns a [`DegradedMode::TurnBudgetExceeded`] result
/// preserving any tool results already observed.
#[expect(
    clippy::too_many_arguments,
    reason = "stage receives all pipeline dependencies"
)]
#[expect(
    clippy::too_many_lines,
    reason = "execute stage orchestrates cooperative timeout, streaming, and degraded-mode fallback — splitting adds indirection"
)]
pub(super) async fn run_execute_stage(
    config: &NousConfig,
    _pipeline_config: &PipelineConfig,
    ctx: &PipelineContext,
    input: &PipelineInput,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    approval_gate: Option<&crate::approval::ApprovalGate>,
    time_budget: &mut crate::budget::TimeBudget,
    emitter: &EventEmitter,
    hooks: Option<&HookRegistry>,
    session_store: Option<&Mutex<SessionStore>>,
    audit_log: Option<&crate::audit::PromptAuditLog>,
) -> error::Result<TurnResult> {
    time_budget.begin_stage("execute");
    let span = info_span!(
        "pipeline_stage",
        stage = "execute",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty,
        tokens_in = tracing::field::Empty,
        tokens_out = tracing::field::Empty,
    );
    let start = Instant::now();

    let execute_deadline = time_budget.stage_deadline("execute");

    let execute_fut = async {
        if let Some(tx) = stream_tx {
            crate::execute::execute_streaming_with_deadline(
                ctx,
                &input.session,
                config,
                providers,
                tools,
                tool_ctx,
                tx,
                approval_gate,
                hooks,
                execute_deadline,
                audit_log,
            )
            .await
        } else {
            crate::execute::execute_with_deadline(
                ctx,
                &input.session,
                config,
                providers,
                tools,
                tool_ctx,
                None,
                approval_gate,
                hooks,
                execute_deadline,
                audit_log,
            )
            .await
        }
    }
    .instrument(span.clone());

    let execute_result = execute_fut.await;

    // WHY: transient LLM errors (rate limit, 5xx) must degrade gracefully to cached context or an
    // unavailable message; non-transient errors (auth, config, panic) propagate for operator action.
    let result = match execute_result {
        Ok(turn_result)
            if matches!(
                turn_result.degraded,
                Some(crate::pipeline::DegradedMode::TurnBudgetExceeded { .. })
            ) =>
        {
            let elapsed_secs = u32::try_from(start.elapsed().as_secs()).unwrap_or(u32::MAX);
            span.record("status", "turn_timeout");
            emitter.emit(&StageTimeout {
                nous_id: config.id.to_string(),
                stage: "execute",
                timeout_secs: elapsed_secs,
            });
            emitter.emit(&StageError {
                nous_id: config.id.to_string(),
                stage: "execute",
                error_type: "turn_timeout".to_owned(),
            });
            time_budget.end_stage(crate::budget::StageTimingStatus::TimedOut);
            return Ok(turn_result);
        }
        Ok(turn_result) => turn_result,
        Err(ref err) if crate::degraded_mode::is_transient_llm_error(err) => {
            // WHY(#4730, #5245): use a bounded wait instead of try_lock() so a
            // briefly-contended store does not silently produce a no-cache result.
            // 50ms is well under any LLM latency — contention at this point is transient.
            let recent_distillation = if let Some(store_mutex) = session_store {
                match tokio::time::timeout(std::time::Duration::from_millis(50), store_mutex.lock())
                    .await
                {
                    Ok(store) => match cached_distillation_for_session(&store, &input.session.id) {
                        Ok(summary) => summary,
                        Err(e) => {
                            // WHY(#5245): a store read error is distinct from a genuine
                            // no-cache; surface it instead of silently collapsing both to None.
                            tracing::warn!(
                                nous_id = %config.id,
                                error = ?e,
                                "degraded recovery: session store read error; no cache available"
                            );
                            None
                        }
                    },
                    Err(_contended) => {
                        tracing::warn!(
                            nous_id = %config.id,
                            "degraded recovery: session store lock contended; no cache available"
                        );
                        None
                    }
                }
            } else {
                None
            };

            emitter.emit(&StageError {
                nous_id: config.id.to_string(),
                stage: "execute",
                error_type: "degraded_mode".to_owned(),
            });

            span.record("status", "degraded");
            let routed_model = crate::execute::routed_model_for_turn(ctx, config, providers, tools);
            let attempt = crate::degraded_mode::DegradedAttemptContext {
                attempted_provider: provider_name_for_model(providers, &routed_model),
                configured_model: config.generation.model.clone(),
                routed_model,
                source_id: recent_distillation
                    .as_ref()
                    .and_then(|distillation| distillation.source_id.clone()),
            };
            crate::degraded_mode::build_degraded_response_with_provenance(
                &config.id,
                &input.session.id,
                err,
                recent_distillation
                    .as_ref()
                    .map(|distillation| distillation.summary.as_str()),
                attempt,
            )
        }
        Err(err) => {
            emitter.emit(&StageError {
                nous_id: config.id.to_string(),
                stage: "execute",
                error_type: "pipeline_error".to_owned(),
            });
            time_budget.end_stage(crate::budget::StageTimingStatus::Completed);
            return Err(err);
        }
    };

    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(&span, &start);
    span.record("tokens_in", result.usage.input_tokens);
    span.record("tokens_out", result.usage.output_tokens);
    if result.degraded.is_none() {
        span.record("status", "ok");
    }
    emitter.emit(&StageCompleted {
        nous_id: config.id.to_string(),
        stage: "execute",
        duration_secs,
    });
    time_budget.end_stage(crate::budget::StageTimingStatus::Completed);
    Ok(result)
}

/// Finalize stage: persist turn results to durable storage.
/// Return value for `run_finalize_stage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FinalizeOutcome {
    /// Turn was persisted to the session store.
    Persisted,
    /// No session store was configured; turn was not persisted.
    NoStore,
    /// Persistence failed with an error.
    Failed,
}

pub(super) async fn run_finalize_stage(
    config: &NousConfig,
    input: &PipelineInput,
    result: &TurnResult,
    session_store: Option<&Mutex<SessionStore>>,
    emitter: &EventEmitter,
) -> FinalizeOutcome {
    let span = info_span!(
        "pipeline_stage",
        stage = "finalize",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let start = Instant::now();
    let outcome = if let Some(store_mutex) = session_store {
        let store = store_mutex.lock().instrument(span.clone()).await;
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
                    messages = fr.messages_persisted(),
                    usage = fr.usage_recorded(),
                    "finalize complete"
                );
                span.record("status", "ok");
                FinalizeOutcome::Persisted
            }
            Err(e) => {
                error!(error = %e, "finalize failed, returning result without persistence");
                span.record("status", "error");
                emitter.emit(&StageError {
                    nous_id: config.id.to_string(),
                    stage: "finalize",
                    error_type: "persistence_failed".to_owned(),
                });
                FinalizeOutcome::Failed
            }
        }
    } else {
        span.record("status", "skipped");
        emitter.emit(&StageSkipped {
            nous_id: config.id.to_string(),
            stage: "finalize",
            reason: "no session store".to_owned(),
        });
        FinalizeOutcome::NoStore
    };
    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(&span, &start);
    emitter.emit(&StageCompleted {
        nous_id: config.id.to_string(),
        stage: "finalize",
        duration_secs,
    });
    outcome
}

const REFLECTION_SOURCE_FACT_LIMIT: i64 = 64;
const REFLECTION_EMIT_FACT_LIMIT: usize = 16;

/// Reflection stage: read recent facts and emit durable reflected facts.
///
/// The stage is intentionally conservative: it promotes existing current facts
/// into deterministic `Reflected` facts without inventing new content. The
/// deterministic ID makes repeated reflection passes idempotent.
pub(super) async fn run_reflection_stage(
    config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    knowledge_store: Option<Arc<KnowledgeStore>>,
    source_session_id: Option<&str>,
    emitter: &EventEmitter,
) -> error::Result<()> {
    let span = info_span!(
        "pipeline_stage",
        stage = "reflection",
        duration_ms = tracing::field::Empty,
        status = tracing::field::Empty,
        facts_emitted = tracing::field::Empty,
    );
    let start = Instant::now();

    if !pipeline_config.reflection_enabled {
        span.record("status", "skipped");
        emitter.emit(&StageSkipped {
            nous_id: config.id.to_string(),
            stage: "reflection",
            reason: "reflection disabled".to_owned(),
        });
        record_reflection_outcome(config, ctx, emitter, ReflectionStatus::Disabled, 0);
        complete_reflection_stage(config, emitter, &span, &start);
        return Ok(());
    }

    let Some(store) = knowledge_store else {
        span.record("status", "no_store");
        emitter.emit(&StageSkipped {
            nous_id: config.id.to_string(),
            stage: "reflection",
            reason: "knowledge store unavailable".to_owned(),
        });
        record_reflection_outcome(config, ctx, emitter, ReflectionStatus::NoStore, 0);
        complete_reflection_stage(config, emitter, &span, &start);
        return Ok(());
    };

    let outcome =
        persist_reflected_facts(config, store, source_session_id, jiff::Timestamp::now()).await;
    match outcome.status {
        ReflectionStatus::Failed => {
            span.record("status", "failed");
            if let Some(error_type) = outcome.error_type {
                emitter.emit(&StageError {
                    nous_id: config.id.to_string(),
                    stage: "reflection",
                    error_type: error_type.to_owned(),
                });
            }
        }
        ReflectionStatus::Skipped => {
            span.record("status", "skipped");
            emitter.emit(&StageSkipped {
                nous_id: config.id.to_string(),
                stage: "reflection",
                reason: "no eligible facts".to_owned(),
            });
        }
        ReflectionStatus::Completed => {
            span.record("status", "ok");
            span.record("facts_emitted", outcome.facts_emitted);
        }
        ReflectionStatus::Disabled | ReflectionStatus::NoStore => {}
    }
    record_reflection_outcome(config, ctx, emitter, outcome.status, outcome.facts_emitted);
    complete_reflection_stage(config, emitter, &span, &start);
    Ok(())
}

struct ReflectionWorkOutcome {
    status: ReflectionStatus,
    facts_emitted: u32,
    error_type: Option<&'static str>,
}

impl ReflectionWorkOutcome {
    const fn completed(facts_emitted: u32) -> Self {
        Self {
            status: ReflectionStatus::Completed,
            facts_emitted,
            error_type: None,
        }
    }

    const fn skipped() -> Self {
        Self {
            status: ReflectionStatus::Skipped,
            facts_emitted: 0,
            error_type: None,
        }
    }

    const fn failed(error_type: &'static str, facts_emitted: u32) -> Self {
        Self {
            status: ReflectionStatus::Failed,
            facts_emitted,
            error_type: Some(error_type),
        }
    }
}

async fn persist_reflected_facts(
    config: &NousConfig,
    store: Arc<KnowledgeStore>,
    source_session_id: Option<&str>,
    recorded_at: jiff::Timestamp,
) -> ReflectionWorkOutcome {
    let now_str = mneme::knowledge::format_timestamp(&recorded_at);
    let mut source_facts = match store
        .query_facts_async(config.id.to_string(), now_str, REFLECTION_SOURCE_FACT_LIMIT)
        .await
    {
        Ok(facts) => facts,
        Err(err) => {
            warn!(error = %err, "reflection query failed");
            return ReflectionWorkOutcome::failed("query_failed", 0);
        }
    };

    source_facts.sort_by_key(|fact| Reverse(fact.temporal.recorded_at));

    let mut facts_persisted = 0_u32;
    for source_fact in source_facts
        .into_iter()
        .filter(is_reflection_candidate)
        .take(REFLECTION_EMIT_FACT_LIMIT)
    {
        let reflected = match reflected_fact(&source_fact, config, source_session_id, recorded_at) {
            Ok(reflected) => reflected,
            Err(err) => {
                warn!(source_fact_id = %source_fact.id, error = %err, "reflection id build failed");
                return ReflectionWorkOutcome::failed("id_build_failed", facts_persisted);
            }
        };
        if let Err(err) = store.insert_fact_async(reflected).await {
            warn!(source_fact_id = %source_fact.id, error = %err, "reflection write failed");
            return ReflectionWorkOutcome::failed("persistence_failed", facts_persisted);
        }
        facts_persisted = facts_persisted.saturating_add(1);
    }

    if facts_persisted == 0 {
        ReflectionWorkOutcome::skipped()
    } else {
        ReflectionWorkOutcome::completed(facts_persisted)
    }
}

fn record_reflection_outcome(
    config: &NousConfig,
    ctx: &mut PipelineContext,
    emitter: &EventEmitter,
    status: ReflectionStatus,
    facts_emitted: u32,
) {
    ctx.reflection_result = Some(ReflectionResult::new(status, facts_emitted));
    emitter.emit(&ReflectionOutcome {
        nous_id: config.id.to_string(),
        status: status.as_str(),
        facts_emitted,
    });
}

fn complete_reflection_stage(
    config: &NousConfig,
    emitter: &EventEmitter,
    span: &tracing::Span,
    start: &Instant,
) {
    let duration_secs = start.elapsed().as_secs_f64();
    record_stage_duration(span, start);
    emitter.emit(&StageCompleted {
        nous_id: config.id.to_string(),
        stage: "reflection",
        duration_secs,
    });
}

fn is_reflection_candidate(fact: &Fact) -> bool {
    matches!(
        fact.provenance.tier,
        EpistemicTier::Inferred | EpistemicTier::Assumed
    ) && !fact.id.as_str().starts_with("reflection-")
}

fn reflected_fact(
    source: &Fact,
    config: &NousConfig,
    source_session_id: Option<&str>,
    recorded_at: jiff::Timestamp,
) -> Result<Fact, mneme::id::IdValidationError> {
    let mut reflected = source.clone();
    reflected.id = reflected_fact_id(source)?;
    reflected.nous_id = config.id.to_string();
    reflected.temporal.recorded_at = recorded_at;
    reflected.provenance.tier = EpistemicTier::Reflected;
    reflected.provenance.source_session_id = source_session_id.map(str::to_owned);
    reflected.access.access_count = 0;
    reflected.access.last_accessed_at = None;
    reflected.lifecycle.superseded_by = None;
    reflected.lifecycle.is_forgotten = false;
    reflected.lifecycle.forgotten_at = None;
    reflected.lifecycle.forget_reason = None;
    Ok(reflected)
}

fn reflected_fact_id(source: &Fact) -> Result<FactId, mneme::id::IdValidationError> {
    let mut hasher = Sha256::new();
    hasher.update(source.nous_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(source.id.as_str().as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in &digest {
        hex.push(hex_nibble(*byte >> 4));
        hex.push(hex_nibble(*byte & 0x0f));
    }
    FactId::new(format!("reflection-{hex}"))
}

fn hex_nibble(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => '0',
    }
}

/// Record elapsed duration on a pipeline stage span.
// kanon:ignore RUST/doc-promised-observability — function directly calls span.record() which is tracing observability
fn record_stage_duration(span: &tracing::Span, start: &Instant) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "u128→u64: stage duration fits in u64"
    )]
    {
        span.record("duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
    }
}

/// Apply a recall result (vector or BM25) to the pipeline context.
///
/// Appends the recall section to the system prompt and records token consumption.
fn apply_recall_result(
    result: error::Result<crate::recall::RecallStageResult>,
    ctx: &mut PipelineContext,
    span: &tracing::Span,
    late_inject_anchor: bool,
    emitter: &EventEmitter,
    nous_id: &str,
) {
    match result {
        Ok(recall_result) => {
            if let Some(ref section) = recall_result.recall_section {
                if late_inject_anchor {
                    #[expect(
                        clippy::cast_possible_wrap,
                        clippy::as_conversions,
                        reason = "u64→i64: recall tokens fit in i64"
                    )]
                    let token_estimate = recall_result.tokens_consumed as i64; // kanon:ignore RUST/as-cast
                    ctx.messages.push(PipelineMessage::text(
                        "system",
                        section.clone(),
                        token_estimate,
                    ));
                } else if let Some(ref mut prompt) = ctx.system_prompt {
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
                        .saturating_sub(recall_result.tokens_consumed as i64) // kanon:ignore RUST/as-cast
                        .max(0);
                }
            }
            ctx.recall_result = Some(recall_result);
            span.record("status", "ok");
        }
        Err(e) => {
            tracing::warn!(error = %e, "recall stage failed, continuing without recalled knowledge");
            span.record("status", "error");
            emitter.emit(&StageError {
                nous_id: nous_id.to_owned(),
                stage: "recall",
                error_type: "recall_failed".to_owned(),
            });
        }
    }
}

#[cfg(test)]
#[path = "stages_tests.rs"]
mod stages_tests;
