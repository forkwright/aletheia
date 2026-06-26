// kanon:ignore RUST/file-too-long — execute stage orchestration; split into dispatch/response modules planned
//! Execute stage: LLM call and tool iteration loop.

mod dispatch;
mod model_fallback;
mod resolve;
mod spawn_guard;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::Arc;

use snafu::ResultExt;
use tokio::sync::mpsc;
use tracing::{debug, info, instrument, warn};

use hermeneus::anthropic::StreamEvent as LlmStreamEvent;
use hermeneus::fallback::FallbackConfig;
use hermeneus::provider::ProviderRegistry;
use hermeneus::types::{
    CompletionRequest, Content, ContentBlock, Message, Role, ServerToolDefinition, StopReason,
    ThinkingConfig, ToolResultContent,
};
use organon::registry::ToolRegistry;
use organon::surface::SurfaceInputs;
use organon::types::ToolContext;

use self::dispatch::{
    DispatchResult, ToolDispatchPolicy, build_messages, classify_signals, dispatch_tools,
};
use self::resolve::{
    process_response_blocks, resolve_active_server_tools, resolve_provider_checked,
    resolve_turn_model,
};
use crate::approval::ApprovalGate;
use crate::config::NousConfig;
use crate::error;
use crate::hooks::registry::HookRegistry;
use crate::hooks::{AfterToolContext, ToolHookContext, ToolHookResult, ToolResultRecord};
use crate::pipeline::{LoopDetector, PipelineContext, ToolCall, TurnResult, TurnUsage};
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

const STOP_REASON_MAX_TOOL_ITERATIONS: &str = "max_tool_iterations";
const STOP_REASON_CLIENT_DISCONNECT: &str = "client_disconnect";

/// Execute stage: calls the LLM and iterates on tool use.
///
/// This is the core agent loop. It:
/// 1. Builds a `CompletionRequest` from pipeline context
/// 2. Calls the LLM
/// 3. Processes `tool_use` blocks by dispatching to the `ToolRegistry`
/// 4. Feeds tool results back and re-calls the LLM
/// 5. Repeats until `EndTurn`, `MaxTokens`, or iteration limit
///
/// # Cancel safety
///
/// Not cancel-safe. If cancelled mid-loop, tool calls may have been
/// dispatched but their results not processed, leaving the session
/// in an inconsistent state. Do not use in `select!` branches.
pub async fn execute(
    ctx: &PipelineContext,
    session: &SessionState,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    hooks: Option<&HookRegistry>,
) -> error::Result<TurnResult> {
    execute_with_dispatch(
        ctx, session, config, providers, tools, tool_ctx, None, None, hooks,
    )
    .await
}

#[expect(
    clippy::too_many_lines,
    reason = "execution loop is inherently sequential, splitting would obscure control flow"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "shared execute core accepts optional streaming and approval adapters"
)]
#[instrument(skip_all, fields(nous_id = %session.nous_id, session_id = %session.id))]
async fn execute_with_dispatch(
    ctx: &PipelineContext,
    session: &SessionState,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    approval_gate: Option<&ApprovalGate>,
    hooks: Option<&HookRegistry>,
) -> error::Result<TurnResult> {
    // WHY: resolve the turn model once — complexity routing pins a tier for
    // the whole turn so tool-iteration continuations don't oscillate between
    // models mid-response. Tool count is approximated as the allowlist size
    // when restricted, else the full registry size; the score only shifts a
    // tier when tool_count crosses small integer breakpoints, so approximation
    // here doesn't bend routing off the correct tier.
    let tool_count = config.tool_allowlist.as_ref().map_or_else(
        || tools.definitions_for_policy(&config.tool_groups).len(),
        Vec::len,
    );
    let turn_model = resolve_turn_model(ctx, config, providers, tool_count);

    let mut messages = build_messages(&ctx.messages);
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut total_usage = TurnUsage::default();
    let mut loop_detector = LoopDetector::with_window(
        config.limits.loop_detection_threshold,
        config.limits.consecutive_error_threshold,
        config.limits.loop_max_warnings,
        config.limits.loop_detection_window,
        config.limits.cycle_detection_max_len,
    );
    let mut iterations: u32 = 0;
    let mut consecutive_tool_only: u32 = 0;
    let mut final_content = String::new();
    let mut final_stop_reason = STOP_REASON_MAX_TOOL_ITERATIONS.to_owned();
    let mut used_server_web_search = false;
    let mut used_server_code_execution = false;
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut tool_surface_hashes: HashSet<String> = HashSet::new();

    let thinking = config
        .generation
        .thinking_enabled
        .then_some(ThinkingConfig {
            enabled: true,
            budget_tokens: config.generation.thinking_budget,
        });
    let fallback_config = (!config.generation.fallback_models.is_empty()).then(|| FallbackConfig {
        fallback_models: config.generation.fallback_models.clone(),
        retries_before_fallback: config.generation.retries_before_fallback,
    });

    // WHY: hoist the config server_tools Vec into an Arc once per turn so the
    // per-iteration backward-compat clone becomes a pointer bump (#3389).
    // Cloning the Arc once at the boundary keeps downstream helpers pure of
    // lifetime concerns.
    let config_server_tools: Arc<Vec<ServerToolDefinition>> = Arc::new(config.server_tools.clone());

    // WHY(#3781): detect cache breakpoint to enable cached-read pricing on
    // the turn after distillation. When a message has cache_breakpoint=true,
    // the prefix up to and including that message should be cached so the
    // next turn benefits from cache_read pricing instead of repaying the
    // prefix cost. Enable cache_turns to mark recent turns as cacheable.
    let has_cache_breakpoint = ctx.messages.iter().any(|m| m.cache_breakpoint);

    loop {
        iterations += 1;

        if iterations > config.limits.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            STOP_REASON_MAX_TOOL_ITERATIONS.clone_into(&mut final_stop_reason);
            break;
        }

        let (active, server_tools) = resolve_active_server_tools(tool_ctx, &config_server_tools);
        let surface = Arc::new(
            tools.effective_surface(SurfaceInputs {
                policy: &config.tool_groups,
                allowlist: config.tool_allowlist.as_deref(),
                active: active.as_ref(),
                server_tools: server_tools.as_slice(),
                server_tool_config: tool_ctx
                    .services
                    .as_deref()
                    .map(|services| &services.server_tool_config),
            }),
        );
        let surface_hash = surface.hash().as_str().to_owned();
        tool_surface_hashes.insert(surface_hash);
        let _surface_binding = tool_ctx.bind_effective_surface(Arc::clone(&surface));
        let dispatch_policy = ToolDispatchPolicy::new(surface);
        let tool_defs = dispatch_policy.tool_definitions();

        let tool_count = tool_defs.len();
        let bytes_serialized = serde_json::to_string(&tool_defs).map_or(0, |s| s.len());
        debug!(tool_count, bytes_serialized, "LLM tool block assembled");

        // WHY: CompletionRequest owns a Vec; this clone is the only unavoidable
        // copy (one per iteration). When nothing has changed, the Arc we hold
        // has refcount > 1 and the underlying Vec is shared across turns, so
        // only this leaf clone pays. Still cheaper than rebuilding every turn.
        let request = CompletionRequest {
            model: turn_model.clone(),
            system: ctx.system_prompt.clone(),
            messages: messages.clone(),
            max_tokens: config.generation.max_output_tokens,
            tools: tool_defs,
            server_tools: dispatch_policy.server_tool_definitions(),
            temperature: None,
            thinking: thinking.clone(),
            stop_sequences: vec![],
            cache_system: config.cache_enabled,
            cache_tools: config.cache_enabled,
            // WHY(#3781): when a cache breakpoint (distilled summary) is present,
            // enable turn caching to allow subsequent turns to benefit from
            // cached-read pricing on the prefix.
            cache_turns: config.cache_enabled && has_cache_breakpoint,
            ..Default::default()
        };

        let completion = if let Some(fallback_config) = &fallback_config {
            model_fallback::complete_with_registry_fallback(providers, &request, fallback_config)
                .await
        } else {
            let provider = resolve_provider_checked(providers, &turn_model)?;
            provider.complete(&request).await
        };

        let response = match completion {
            Ok(resp) => {
                if fallback_config.is_none() {
                    let provider = resolve_provider_checked(providers, &turn_model)?;
                    providers.record_success(provider.name());
                }
                resp
            }
            Err(e) => {
                if fallback_config.is_none()
                    && let Ok(provider) = resolve_provider_checked(providers, &turn_model)
                {
                    providers.record_error(provider.name(), &e);
                }
                return Err(e).context(error::LlmSnafu);
            }
        };

        let hermeneus::types::CompletionResponse {
            content: response_content,
            stop_reason,
            usage,
            ..
        } = response;

        total_usage.input_tokens += usage.input_tokens;
        total_usage.output_tokens += usage.output_tokens;
        total_usage.cache_read_tokens += usage.cache_read_tokens;
        total_usage.cache_write_tokens += usage.cache_write_tokens;
        total_usage.llm_calls += 1;

        let mut extracted = process_response_blocks(&response_content);
        used_server_web_search |= extracted.saw_server_web_search;
        used_server_code_execution |= extracted.saw_server_code_execution;
        final_content = extracted.text_parts.join("");
        reasoning_parts.extend(extracted.reasoning_parts);
        final_stop_reason = stop_reason.to_string();

        let mut denied_blocks: Vec<ContentBlock> = Vec::new();

        // WHY: active hallucination detection — verify any receipt citations in the
        // assistant message before dispatching new tool calls. A fabricated receipt
        // means the model is narrating a fake tool call; halt immediately.
        {
            let ledger = session.receipt_ledger.lock().unwrap_or_else(|poisoned| {
                tracing::warn!("receipt_ledger lock poisoned, recovering with last value");
                poisoned.into_inner()
            });
            organon::receipts::scan_and_verify(&session.receipt_signer, &ledger, &final_content)
                .map_err(|details| error::HallucinationDetectedSnafu { details }.build())?;
        }

        // WHY: spawn-class isolation guard — spawn tools must be the last tool in a turn.
        // If a spawn tool is followed by other tools, truncate and inject errors.
        spawn_guard::enforce_spawn_isolation(&mut extracted.tool_uses, &mut denied_blocks, tools);

        // WHY: only break on no local tool uses: server tool results don't require client tool_result
        if extracted.tool_uses.is_empty() || stop_reason != StopReason::ToolUse {
            break;
        }
        if iterations >= config.limits.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            STOP_REASON_MAX_TOOL_ITERATIONS.clone_into(&mut final_stop_reason);
            break;
        }

        // WHY: Track consecutive iterations that produce tool calls without any
        // reasoning text. When the limit is hit, inject a system message asking
        // the agent to explain its reasoning before continuing.
        let has_reasoning = extracted
            .text_parts
            .iter()
            .any(|t| t.chars().any(|c| !c.is_whitespace()));
        if has_reasoning {
            consecutive_tool_only = 0;
        } else {
            consecutive_tool_only += 1;
        }

        messages.push(Message {
            role: Role::Assistant,
            content: Content::Blocks(response_content),
            cache_breakpoint: false,
        });

        let effective_tool_uses = dispatch_policy.filter_tool_uses(
            extracted.tool_uses,
            tools,
            tool_ctx,
            stream_tx,
            &mut all_tool_calls,
            &mut denied_blocks,
        );

        // WHY: before_tool hooks run after allowlist filtering but before dispatch,
        // so hooks can deny individual tool calls based on budget/scope/policy.
        let effective_tool_uses = if let Some(hook_registry) = hooks {
            let hook_ctx = ToolHookContext {
                nous_id: &session.nous_id,
                turn_usage: &total_usage,
                tool_allowlist: config.tool_allowlist.as_deref(),
            };
            let mut hook_allowed = Vec::with_capacity(effective_tool_uses.len());
            for (id, name, input) in effective_tool_uses {
                match hook_registry
                    .run_before_tool(&name, &input, &hook_ctx)
                    .await
                {
                    ToolHookResult::Allow => hook_allowed.push((id, name, input)),
                    ToolHookResult::Deny { reason } => {
                        warn!(tool = %name, tool_use_id = %id, reason = %reason, "tool call denied by hook");
                        denied_blocks.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: ToolResultContent::Text(reason),
                            is_error: Some(true),
                        });
                    }
                }
            }
            hook_allowed
        } else {
            effective_tool_uses
        };

        let all_tool_calls_before = all_tool_calls.len();
        let DispatchResult {
            mut blocks,
            loop_warning,
        } = dispatch_tools(
            &effective_tool_uses,
            tools,
            tool_ctx,
            &mut loop_detector,
            &mut all_tool_calls,
            iterations,
            stream_tx,
            approval_gate,
            &dispatch_policy,
            config.limits.max_tool_result_bytes,
            Some(&session.receipt_signer),
            Some(&*session.receipt_ledger),
        )
        .await?;

        // WHY: fire after_tool hooks for each tool executed in this iteration.
        // Hooks run in priority order but do not short-circuit (tool is already executed).
        if let Some(hook_registry) = hooks {
            for tool_call in all_tool_calls.get(all_tool_calls_before..).unwrap_or(&[]) {
                let after_tool_ctx = AfterToolContext {
                    nous_id: &session.nous_id,
                    tool_name: &tool_call.name,
                    tool_input: effective_tool_uses
                        .iter()
                        .find(|(_, name, _)| name == &tool_call.name)
                        .map_or(&serde_json::Value::Null, |(_, _, input)| input),
                    tool_result: ToolResultRecord::from_option(tool_call.result.as_deref()),
                    is_error: tool_call.is_error,
                    turn_usage: &total_usage,
                };
                hook_registry.run_after_tool(&after_tool_ctx).await;
            }
        }

        blocks.extend(denied_blocks);

        if let Some(ref warning) = loop_warning {
            debug!(warning = warning.as_str(), "loop warning injected");
            blocks.push(ContentBlock::Text {
                text: format!("[System: {warning}]"),
                citations: None,
            });
        }

        let tool_only_limit = config.limits.max_consecutive_tool_only_iterations;
        if tool_only_limit > 0 && consecutive_tool_only >= tool_only_limit {
            debug!(
                consecutive_tool_only,
                limit = tool_only_limit,
                "tool-only iteration limit reached, injecting reasoning prompt"
            );
            blocks.push(ContentBlock::Text {
                text: "[System: You have made several consecutive tool calls without explaining \
                       your reasoning. Before making more tool calls, briefly explain what you \
                       are trying to accomplish and why these tool calls are needed.]"
                    .to_owned(),
                citations: None,
            });
            consecutive_tool_only = 0;
        }

        messages.push(Message {
            role: Role::User,
            content: Content::Blocks(blocks),
            cache_breakpoint: false,
        });
    }

    info!(
        iterations,
        tool_calls = all_tool_calls.len(),
        llm_calls = total_usage.llm_calls,
        stop_reason = final_stop_reason.as_str(),
        "execute stage complete"
    );

    let signals = classify_signals(
        &all_tool_calls,
        &final_content,
        used_server_web_search,
        used_server_code_execution,
    );

    Ok(TurnResult {
        content: final_content,
        tool_calls: all_tool_calls,
        usage: total_usage,
        signals,
        stop_reason: final_stop_reason,
        degraded: None,
        reasoning: reasoning_parts.join("\n"),
        model_used: turn_model,
        tool_surface_hashes: tool_surface_hashes.into_iter().collect(),
    })
}

/// Record a failed `try_send` of an LLM streaming delta.
///
/// WHY(#4893): Dropped text/thinking deltas must be visible in the same metric
/// family as dropped tool lifecycle events. The event type label distinguishes
/// text, thinking, and other delta shapes so operators can pinpoint saturation.
fn record_llm_stream_send_error(
    nous_id: &str,
    event: &LlmStreamEvent,
    err: &tokio::sync::mpsc::error::TrySendError<TurnStreamEvent>,
) {
    let event_type = match event {
        LlmStreamEvent::TextDelta { .. } => "text_delta",
        LlmStreamEvent::ThinkingDelta { .. } => "thinking_delta",
        LlmStreamEvent::InputJsonDelta { .. } => "input_json_delta",
        _ => "llm_delta",
    };
    match err {
        tokio::sync::mpsc::error::TrySendError::Full(_) => {
            warn!(
                nous_id,
                event_type, "streaming LLM delta dropped: channel buffer full"
            );
            crate::metrics::record_stream_event_dropped(nous_id, event_type, "buffer_full");
        }
        tokio::sync::mpsc::error::TrySendError::Closed(_) => {
            debug!(
                nous_id,
                event_type, "streaming LLM delta dropped: receiver disconnected"
            );
            crate::metrics::record_stream_event_dropped(nous_id, event_type, "disconnected");
        }
    }
}

/// Streaming execute stage: same as [`execute`] but emits real-time events.
///
/// Uses `complete_streaming()` when the provider supports it, falling back to
/// `complete()` otherwise. Tool start/result events are emitted via the channel.
///
/// # Cancel safety
///
/// Not cancel-safe. Same as [`execute`]: if cancelled mid-loop, partial
/// streaming events may have been sent but the final result is lost,
/// leaving the session in an inconsistent state. Do not use in `select!` branches.
#[expect(
    clippy::too_many_lines,
    reason = "streaming agent loop parallels execute() with provider callback — one cohesive operation"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "streaming execute requires provider, tools, context, stream channel, and hooks"
)]
#[instrument(skip_all, fields(nous_id = %session.nous_id, session_id = %session.id))]
pub async fn execute_streaming(
    ctx: &PipelineContext,
    session: &SessionState,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    stream_tx: &mpsc::Sender<TurnStreamEvent>,
    approval_gate: Option<&ApprovalGate>,
    hooks: Option<&HookRegistry>,
) -> error::Result<TurnResult> {
    // WHY: resolve the streaming turn model once — same reasoning as execute().
    // Must come before find_streaming_provider so the streaming provider is
    // looked up for the actual model the turn will use.
    let tool_count = config.tool_allowlist.as_ref().map_or_else(
        || tools.definitions_for_policy(&config.tool_groups).len(),
        Vec::len,
    );
    let turn_model = resolve_turn_model(ctx, config, providers, tool_count);

    let Some(streaming_provider) = providers.find_streaming_provider(&turn_model) else {
        return execute_with_dispatch(
            ctx,
            session,
            config,
            providers,
            tools,
            tool_ctx,
            Some(stream_tx),
            approval_gate,
            hooks,
        )
        .await;
    };

    let provider = resolve_provider_checked(providers, &turn_model)?;

    let mut messages = build_messages(&ctx.messages);
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut total_usage = TurnUsage::default();
    let mut loop_detector = LoopDetector::with_window(
        config.limits.loop_detection_threshold,
        config.limits.consecutive_error_threshold,
        config.limits.loop_max_warnings,
        config.limits.loop_detection_window,
        config.limits.cycle_detection_max_len,
    );
    let mut iterations: u32 = 0;
    let mut consecutive_tool_only: u32 = 0;
    let mut final_content = String::new();
    let mut final_stop_reason = STOP_REASON_CLIENT_DISCONNECT.to_owned();
    let mut client_disconnected = false;
    let mut used_server_web_search = false;
    let mut used_server_code_execution = false;
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut tool_surface_hashes: HashSet<String> = HashSet::new();

    let thinking = config
        .generation
        .thinking_enabled
        .then_some(ThinkingConfig {
            enabled: true,
            budget_tokens: config.generation.thinking_budget,
        });

    // WHY: hoist config server_tools Vec into Arc once per turn (#3389).
    let config_server_tools: Arc<Vec<ServerToolDefinition>> = Arc::new(config.server_tools.clone());

    loop {
        iterations += 1;

        if iterations > config.limits.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            STOP_REASON_MAX_TOOL_ITERATIONS.clone_into(&mut final_stop_reason);
            break;
        }

        // WHY: if the client has disconnected the stream_tx receiver is dropped and the
        // channel is closed.  Continuing to call the LLM wastes compute and credits (#1721).
        if stream_tx.is_closed() {
            info!("client disconnected, stopping LLM turn");
            STOP_REASON_CLIENT_DISCONNECT.clone_into(&mut final_stop_reason);
            break;
        }

        // WHY: derive server tools on each iteration so enable_tool activations take effect.
        // resolve_active_server_tools reuses the hoisted Arc when no dynamic changes occurred.
        let (active, server_tools) = resolve_active_server_tools(tool_ctx, &config_server_tools);
        let surface = Arc::new(
            tools.effective_surface(SurfaceInputs {
                policy: &config.tool_groups,
                allowlist: config.tool_allowlist.as_deref(),
                active: active.as_ref(),
                server_tools: server_tools.as_slice(),
                server_tool_config: tool_ctx
                    .services
                    .as_deref()
                    .map(|services| &services.server_tool_config),
            }),
        );
        let surface_hash = surface.hash().as_str().to_owned();
        tool_surface_hashes.insert(surface_hash);
        let _surface_binding = tool_ctx.bind_effective_surface(Arc::clone(&surface));
        let dispatch_policy = ToolDispatchPolicy::new(surface);
        let tool_defs = dispatch_policy.tool_definitions();

        let request = CompletionRequest {
            model: turn_model.clone(),
            system: ctx.system_prompt.clone(),
            messages: messages.clone(),
            max_tokens: config.generation.max_output_tokens,
            tools: tool_defs.clone(),
            server_tools: dispatch_policy.server_tool_definitions(),
            temperature: None,
            thinking: thinking.clone(),
            stop_sequences: vec![],
            cache_system: config.cache_enabled,
            cache_tools: config.cache_enabled,
            cache_turns: config.cache_enabled,
            ..Default::default()
        };

        let tx = stream_tx.clone();
        let nous_id = tool_ctx.nous_id.clone();
        let response = match streaming_provider
            .complete_streaming(&request, &mut |event| {
                if let Err(e) = tx.try_send(TurnStreamEvent::LlmDelta(event.clone())) {
                    record_llm_stream_send_error(nous_id.as_ref(), &event, &e);
                }
            })
            .await
        {
            Ok(resp) => {
                providers.record_success(provider.name());
                resp
            }
            Err(e) => {
                providers.record_error(provider.name(), &e);
                return Err(e).context(error::LlmSnafu);
            }
        };

        // WHY: the receiver may be dropped while `complete_streaming` is in flight.  Capture
        // the disconnect so we override any model stop_reason with `client_disconnect` and do
        // not dispatch further tools (#4915).
        if stream_tx.is_closed() {
            client_disconnected = true;
        }

        let hermeneus::types::CompletionResponse {
            content: response_content,
            stop_reason,
            usage,
            ..
        } = response;

        total_usage.input_tokens += usage.input_tokens;
        total_usage.output_tokens += usage.output_tokens;
        total_usage.cache_read_tokens += usage.cache_read_tokens;
        total_usage.cache_write_tokens += usage.cache_write_tokens;
        total_usage.llm_calls += 1;

        let mut extracted = process_response_blocks(&response_content);
        used_server_web_search |= extracted.saw_server_web_search;
        used_server_code_execution |= extracted.saw_server_code_execution;
        final_content = extracted.text_parts.join("");
        reasoning_parts.extend(extracted.reasoning_parts);
        final_stop_reason = stop_reason.to_string();

        // WHY: if the client disconnected during the in-flight call we still capture the
        // partial/full response content, but we must not dispatch further tools or report the
        // model's stop_reason as a clean completion (#4915).
        client_disconnected |= stream_tx.is_closed();
        if client_disconnected {
            STOP_REASON_CLIENT_DISCONNECT.clone_into(&mut final_stop_reason);
            break;
        }

        let mut denied_blocks: Vec<ContentBlock> = Vec::new();

        // WHY: active hallucination detection — verify any receipt citations in the
        // assistant message before dispatching new tool calls. A fabricated receipt
        // means the model is narrating a fake tool call; halt immediately.
        {
            let ledger = session.receipt_ledger.lock().unwrap_or_else(|poisoned| {
                tracing::warn!("receipt_ledger lock poisoned, recovering with last value");
                poisoned.into_inner()
            });
            organon::receipts::scan_and_verify(&session.receipt_signer, &ledger, &final_content)
                .map_err(|details| error::HallucinationDetectedSnafu { details }.build())?;
        }

        // WHY: spawn-class isolation guard — spawn tools must be the last tool in a turn.
        spawn_guard::enforce_spawn_isolation(&mut extracted.tool_uses, &mut denied_blocks, tools);

        if extracted.tool_uses.is_empty() || stop_reason != StopReason::ToolUse {
            break;
        }
        if iterations >= config.limits.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            STOP_REASON_MAX_TOOL_ITERATIONS.clone_into(&mut final_stop_reason);
            break;
        }

        let has_reasoning = extracted
            .text_parts
            .iter()
            .any(|t| t.chars().any(|c| !c.is_whitespace()));
        if has_reasoning {
            consecutive_tool_only = 0;
        } else {
            consecutive_tool_only += 1;
        }

        messages.push(Message {
            role: Role::Assistant,
            content: Content::Blocks(response_content),
            cache_breakpoint: false,
        });

        let effective_tool_uses = dispatch_policy.filter_tool_uses(
            extracted.tool_uses,
            tools,
            tool_ctx,
            Some(stream_tx),
            &mut all_tool_calls,
            &mut denied_blocks,
        );

        // WHY: before_tool hooks filter tool calls before streaming dispatch
        let effective_tool_uses = if let Some(hook_registry) = hooks {
            let hook_ctx = ToolHookContext {
                nous_id: &session.nous_id,
                turn_usage: &total_usage,
                tool_allowlist: config.tool_allowlist.as_deref(),
            };
            let mut hook_allowed = Vec::with_capacity(effective_tool_uses.len());
            for (id, name, input) in effective_tool_uses {
                match hook_registry
                    .run_before_tool(&name, &input, &hook_ctx)
                    .await
                {
                    ToolHookResult::Allow => hook_allowed.push((id, name, input)),
                    ToolHookResult::Deny { reason } => {
                        warn!(tool = %name, tool_use_id = %id, reason = %reason, "tool call denied by hook");
                        denied_blocks.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: ToolResultContent::Text(reason),
                            is_error: Some(true),
                        });
                    }
                }
            }
            hook_allowed
        } else {
            effective_tool_uses
        };

        let all_tool_calls_before = all_tool_calls.len();
        let DispatchResult {
            mut blocks,
            loop_warning,
        } = dispatch_tools(
            &effective_tool_uses,
            tools,
            tool_ctx,
            &mut loop_detector,
            &mut all_tool_calls,
            iterations,
            Some(stream_tx),
            approval_gate,
            &dispatch_policy,
            config.limits.max_tool_result_bytes,
            Some(&session.receipt_signer),
            Some(&*session.receipt_ledger),
        )
        .await?;

        // WHY: fire after_tool hooks for each tool executed in this iteration.
        // Hooks run in priority order but do not short-circuit (tool is already executed).
        if let Some(hook_registry) = hooks {
            for tool_call in all_tool_calls.get(all_tool_calls_before..).unwrap_or(&[]) {
                let after_tool_ctx = AfterToolContext {
                    nous_id: &session.nous_id,
                    tool_name: &tool_call.name,
                    tool_input: effective_tool_uses
                        .iter()
                        .find(|(_, name, _)| name == &tool_call.name)
                        .map_or(&serde_json::Value::Null, |(_, _, input)| input),
                    tool_result: ToolResultRecord::from_option(tool_call.result.as_deref()),
                    is_error: tool_call.is_error,
                    turn_usage: &total_usage,
                };
                hook_registry.run_after_tool(&after_tool_ctx).await;
            }
        }

        blocks.extend(denied_blocks);

        if let Some(ref warning) = loop_warning {
            debug!(warning = warning.as_str(), "loop warning injected");
            blocks.push(ContentBlock::Text {
                text: format!("[System: {warning}]"),
                citations: None,
            });
        }

        let tool_only_limit = config.limits.max_consecutive_tool_only_iterations;
        if tool_only_limit > 0 && consecutive_tool_only >= tool_only_limit {
            debug!(
                consecutive_tool_only,
                limit = tool_only_limit,
                "tool-only iteration limit reached, injecting reasoning prompt"
            );
            blocks.push(ContentBlock::Text {
                text: "[System: You have made several consecutive tool calls without explaining \
                       your reasoning. Before making more tool calls, briefly explain what you \
                       are trying to accomplish and why these tool calls are needed.]"
                    .to_owned(),
                citations: None,
            });
            consecutive_tool_only = 0;
        }

        messages.push(Message {
            role: Role::User,
            content: Content::Blocks(blocks),
            cache_breakpoint: false,
        });
    }

    info!(
        iterations,
        tool_calls = all_tool_calls.len(),
        llm_calls = total_usage.llm_calls,
        stop_reason = final_stop_reason.as_str(),
        "streaming execute stage complete"
    );

    let signals = classify_signals(
        &all_tool_calls,
        &final_content,
        used_server_web_search,
        used_server_code_execution,
    );

    Ok(TurnResult {
        content: final_content,
        tool_calls: all_tool_calls,
        usage: total_usage,
        signals,
        stop_reason: final_stop_reason,
        degraded: None,
        reasoning: reasoning_parts.join("\n"),
        model_used: turn_model,
        tool_surface_hashes: tool_surface_hashes.into_iter().collect(),
    })
}
