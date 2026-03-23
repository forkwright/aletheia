//! Execute stage: LLM call and tool iteration loop.

mod dispatch;

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use snafu::ResultExt;
use tokio::sync::mpsc;
use tracing::{debug, info, instrument, warn};

use aletheia_hermeneus::health::ProviderHealth;
use aletheia_hermeneus::provider::{LlmProvider, ProviderRegistry};
use aletheia_hermeneus::types::{
    CompletionRequest, Content, ContentBlock, Message, Role, ServerToolDefinition, StopReason,
    ThinkingConfig, ToolResultContent,
};
use aletheia_koina::id::ToolName;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolContext;

use self::dispatch::{
    DispatchResult, build_messages, classify_signals, dispatch_tools, dispatch_tools_streaming,
};
use crate::config::NousConfig;
use crate::error;
use crate::pipeline::{LoopDetector, PipelineContext, ToolCall, TurnResult, TurnUsage};
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

/// Resolve the LLM provider for `model` and verify it is not marked down.
fn resolve_provider_checked<'a>(
    providers: &'a ProviderRegistry,
    model: &str,
) -> error::Result<&'a dyn LlmProvider> {
    let provider = providers.find_provider(model).ok_or_else(|| {
        error::PipelineStageSnafu {
            stage: "execute",
            message: format!("no provider for model: {model}"),
        }
        .build()
    })?;

    if let Some(health) = providers.provider_health(provider.name())
        && matches!(health, ProviderHealth::Down { .. })
    {
        return Err(error::PipelineStageSnafu {
            stage: "execute",
            message: format!("provider '{}' is currently unavailable", provider.name()),
        }
        .build());
    }

    Ok(provider)
}

/// Read the current active-tools set and derive server-tool definitions.
///
/// Returns `(active_set, server_tools)` so callers can also filter local tool
/// definitions against the same snapshot of `active`.
fn resolve_active_server_tools(
    tool_ctx: &ToolContext,
    config: &NousConfig,
) -> (HashSet<ToolName>, Vec<ServerToolDefinition>) {
    let active = tool_ctx
        .active_tools
        .read()
        .unwrap_or_else(|poisoned| {
            warn!("active_tools lock poisoned by prior panic, recovering with last value");
            poisoned.into_inner()
        })
        .clone();

    let server_tools = if let Some(services) = tool_ctx.services.as_deref() {
        let mut st = services.server_tool_config.active_definitions(&active);
        // WHY: also include raw server_tools from NousConfig for backward compatibility
        st.extend(config.server_tools.clone());
        st
    } else {
        config.server_tools.clone()
    };

    (active, server_tools)
}

/// Extracted text, tool uses, and server-tool flags from a single LLM response.
struct ResponseExtract {
    text_parts: Vec<String>,
    tool_uses: Vec<(String, String, serde_json::Value)>,
    saw_server_web_search: bool,
    saw_server_code_execution: bool,
}

/// Process response content blocks into text, tool-use tuples, and server-tool flags.
fn process_response_blocks(content: &[ContentBlock]) -> ResponseExtract {
    let mut extract = ResponseExtract {
        text_parts: Vec::new(),
        tool_uses: Vec::new(),
        saw_server_web_search: false,
        saw_server_code_execution: false,
    };

    for block in content {
        match block {
            ContentBlock::Text { text, .. } => extract.text_parts.push(text.clone()),
            ContentBlock::ToolUse { id, name, input } => {
                extract
                    .tool_uses
                    .push((id.clone(), name.clone(), input.clone()));
            }
            ContentBlock::Thinking { thinking, .. } => {
                debug!(len = thinking.len(), "thinking block received");
            }
            ContentBlock::ServerToolUse { name, .. } if name == "web_search" => {
                extract.saw_server_web_search = true;
            }
            ContentBlock::ServerToolUse { name, .. } if name == "code_execution" => {
                extract.saw_server_code_execution = true;
            }
            ContentBlock::CodeExecutionResult {
                code, return_code, ..
            } => {
                extract.saw_server_code_execution = true;
                debug!(
                    code_len = code.len(),
                    return_code, "server code execution result received"
                );
            }
            // NOTE: other content block types (images, etc.) are not tracked in extraction
            _ => {}
        }
    }

    extract
}

/// Execute stage: calls the LLM and iterates on tool use.
///
/// This is the core agent loop. It:
/// 1. Builds a `CompletionRequest` from pipeline context
/// 2. Calls the LLM
/// 3. Processes `tool_use` blocks by dispatching to the `ToolRegistry`
/// 4. Feeds tool results back and re-calls the LLM
/// 5. Repeats until `EndTurn`, `MaxTokens`, or iteration limit
#[expect(
    clippy::too_many_lines,
    reason = "execution loop is inherently sequential, splitting would obscure control flow"
)]
#[instrument(skip_all, fields(nous_id = %session.nous_id, session_id = %session.id))]
pub async fn execute(
    ctx: &PipelineContext,
    session: &SessionState,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
) -> error::Result<TurnResult> {
    let provider = resolve_provider_checked(providers, &config.generation.model)?;

    let mut messages = build_messages(&ctx.messages);
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut total_usage = TurnUsage::default();
    let mut loop_detector = LoopDetector::with_limits(
        config.limits.loop_detection_threshold,
        config.limits.consecutive_error_threshold,
        config.limits.loop_max_warnings,
    );
    let mut iterations: u32 = 0;
    let mut consecutive_tool_only: u32 = 0;
    let mut final_content = String::new();
    let mut final_stop_reason = String::new();
    let mut used_server_web_search = false;
    let mut used_server_code_execution = false;

    let thinking = config
        .generation
        .thinking_enabled
        .then_some(ThinkingConfig {
            enabled: true,
            budget_tokens: config.generation.thinking_budget,
        });

    loop {
        iterations += 1;

        if iterations > config.limits.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            break;
        }

        let (active, server_tools) = resolve_active_server_tools(tool_ctx, config);
        let mut tool_defs = tools.to_hermeneus_tools_filtered(&active);

        if let Some(allowlist) = &config.tool_allowlist {
            tool_defs.retain(|td| allowlist.iter().any(|a| a == &td.name));
        }

        let request = CompletionRequest {
            model: config.generation.model.clone(),
            system: ctx.system_prompt.clone(),
            messages: messages.clone(),
            max_tokens: config.generation.max_output_tokens,
            tools: tool_defs,
            server_tools,
            temperature: None,
            thinking: thinking.clone(),
            stop_sequences: vec![],
            cache_system: config.cache_enabled,
            cache_tools: config.cache_enabled,
            cache_turns: config.cache_enabled,
            ..Default::default()
        };

        let response = match provider.complete(&request).await {
            Ok(resp) => {
                providers.record_success(provider.name());
                resp
            }
            Err(e) => {
                providers.record_error(provider.name(), &e);
                return Err(e).context(error::LlmSnafu);
            }
        };

        let aletheia_hermeneus::types::CompletionResponse {
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

        let extracted = process_response_blocks(&response_content);
        used_server_web_search |= extracted.saw_server_web_search;
        used_server_code_execution |= extracted.saw_server_code_execution;
        final_content = extracted.text_parts.join("");
        final_stop_reason = stop_reason.to_string();

        // WHY: only break on no local tool uses: server tool results don't require client tool_result
        if extracted.tool_uses.is_empty() || stop_reason != StopReason::ToolUse {
            break;
        }

        // WHY: Track consecutive iterations that produce tool calls without any
        // reasoning text. When the limit is hit, inject a system message asking
        // the agent to explain its reasoning before continuing. Closes #1980.
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
        });

        // WHY: belt-and-suspenders enforcement of role tool restrictions at execution time,
        // in addition to the presentation-level filtering above
        let mut denied_blocks: Vec<ContentBlock> = Vec::new();
        let effective_tool_uses: Vec<_> = if let Some(allowlist) = &config.tool_allowlist {
            let (allowed, denied): (Vec<_>, Vec<_>) = extracted
                .tool_uses
                .into_iter()
                .partition(|(_, name, _)| allowlist.iter().any(|a| a == name));

            for (id, name, _) in &denied {
                warn!(tool = %name, tool_use_id = %id, "tool call denied by role policy");
                denied_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: ToolResultContent::Text(format!(
                        "Tool '{name}' is not available for this role. Available tools: {}",
                        allowlist.join(", ")
                    )),
                    is_error: Some(true),
                });
            }

            allowed
        } else {
            extracted.tool_uses
        };

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
            config.limits.max_tool_result_bytes,
        )
        .await?;

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
    })
}

/// Streaming execute stage: same as [`execute`] but emits real-time events.
///
/// Uses `complete_streaming()` when the provider supports it, falling back to
/// `complete()` otherwise. Tool start/result events are emitted via the channel.
#[expect(
    clippy::too_many_lines,
    reason = "streaming agent loop parallels execute() with provider callback — one cohesive operation"
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
) -> error::Result<TurnResult> {
    let Some(streaming_provider) = providers.find_streaming_provider(&config.generation.model)
    else {
        // NOTE: fall back to non-streaming execute if no streaming provider is registered
        return execute(ctx, session, config, providers, tools, tool_ctx).await;
    };

    let provider = resolve_provider_checked(providers, &config.generation.model)?;

    let mut messages = build_messages(&ctx.messages);
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut total_usage = TurnUsage::default();
    let mut loop_detector = LoopDetector::with_limits(
        config.limits.loop_detection_threshold,
        config.limits.consecutive_error_threshold,
        config.limits.loop_max_warnings,
    );
    let mut iterations: u32 = 0;
    let mut consecutive_tool_only: u32 = 0;
    let mut final_content = String::new();
    let mut final_stop_reason = String::new();
    let mut used_server_web_search = false;
    let mut used_server_code_execution = false;

    let thinking = config
        .generation
        .thinking_enabled
        .then_some(ThinkingConfig {
            enabled: true,
            budget_tokens: config.generation.thinking_budget,
        });

    let mut tool_defs = tools.to_hermeneus_tools();
    if let Some(allowlist) = &config.tool_allowlist {
        tool_defs.retain(|td| allowlist.iter().any(|a| a == &td.name));
    }

    loop {
        iterations += 1;

        if iterations > config.limits.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            break;
        }

        // WHY: if the client has disconnected the stream_tx receiver is dropped and the
        // channel is closed.  Continuing to call the LLM wastes compute and credits (#1721).
        if stream_tx.is_closed() {
            info!("client disconnected, stopping LLM turn");
            break;
        }

        // WHY: derive server tools on each iteration so enable_tool activations take effect
        let (_active, server_tools) = resolve_active_server_tools(tool_ctx, config);

        let request = CompletionRequest {
            model: config.generation.model.clone(),
            system: ctx.system_prompt.clone(),
            messages: messages.clone(),
            max_tokens: config.generation.max_output_tokens,
            tools: tool_defs.clone(),
            server_tools,
            temperature: None,
            thinking: thinking.clone(),
            stop_sequences: vec![],
            cache_system: config.cache_enabled,
            cache_tools: config.cache_enabled,
            cache_turns: config.cache_enabled,
            ..Default::default()
        };

        let tx = stream_tx.clone();
        let response = match streaming_provider
            .complete_streaming(&request, &mut |event| {
                let _ = tx.try_send(TurnStreamEvent::LlmDelta(event));
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

        let aletheia_hermeneus::types::CompletionResponse {
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

        let extracted = process_response_blocks(&response_content);
        used_server_web_search |= extracted.saw_server_web_search;
        used_server_code_execution |= extracted.saw_server_code_execution;
        final_content = extracted.text_parts.join("");
        final_stop_reason = stop_reason.to_string();

        if extracted.tool_uses.is_empty() || stop_reason != StopReason::ToolUse {
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
        });

        let mut denied_blocks: Vec<ContentBlock> = Vec::new();
        let effective_tool_uses: Vec<_> = if let Some(allowlist) = &config.tool_allowlist {
            let (allowed, denied): (Vec<_>, Vec<_>) = extracted
                .tool_uses
                .into_iter()
                .partition(|(_, name, _)| allowlist.iter().any(|a| a == name));

            for (id, name, _) in &denied {
                warn!(tool = %name, tool_use_id = %id, "tool call denied by role policy");
                denied_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: ToolResultContent::Text(format!(
                        "Tool '{name}' is not available for this role. Available tools: {}",
                        allowlist.join(", ")
                    )),
                    is_error: Some(true),
                });
            }

            allowed
        } else {
            extracted.tool_uses
        };

        let DispatchResult {
            mut blocks,
            loop_warning,
        } = dispatch_tools_streaming(
            &effective_tool_uses,
            tools,
            tool_ctx,
            &mut loop_detector,
            &mut all_tool_calls,
            iterations,
            stream_tx,
            config.limits.max_tool_result_bytes,
        )
        .await?;

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
    })
}
