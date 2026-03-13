//! Execute stage — LLM call and tool iteration loop.

// RwLock::read().expect() is infallible under normal operation; poisoning only
// occurs on a prior panic which makes the process state undefined anyway.
#![expect(
    clippy::expect_used,
    reason = "RwLock read is infallible under normal operation"
)]

mod dispatch;

#[cfg(test)]
mod tests;

use snafu::ResultExt;
use tracing::{debug, info, instrument, warn};

use aletheia_hermeneus::health::ProviderHealth;
use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_hermeneus::types::{
    CompletionRequest, Content, ContentBlock, Message, Role, StopReason, ThinkingConfig,
};
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolContext;
use tokio::sync::mpsc;

use crate::config::NousConfig;
use crate::error;
use crate::pipeline::{LoopDetector, PipelineContext, ToolCall, TurnResult, TurnUsage};
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

use dispatch::{build_messages, classify_signals, dispatch_tools, dispatch_tools_streaming};

/// Execute stage — calls the LLM and iterates on tool use.
///
/// This is the core agent loop. It:
/// 1. Builds a `CompletionRequest` from pipeline context
/// 2. Calls the LLM
/// 3. Processes `tool_use` blocks by dispatching to the `ToolRegistry`
/// 4. Feeds tool results back and re-calls the LLM
/// 5. Repeats until `EndTurn`, `MaxTokens`, or iteration limit
#[expect(
    clippy::too_many_lines,
    reason = "health check additions keep the loop cohesive"
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
    let provider = providers.find_provider(&config.model).ok_or_else(|| {
        error::PipelineStageSnafu {
            stage: "execute",
            message: format!("no provider for model: {}", config.model),
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

    let mut messages = build_messages(&ctx.messages);
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut total_usage = TurnUsage::default();
    let mut loop_detector = LoopDetector::new(config.loop_detection_threshold);
    let mut iterations: u32 = 0;
    let mut final_content = String::new();
    let mut final_stop_reason = String::new();
    let mut used_server_web_search = false;
    let mut used_server_code_execution = false;

    let thinking = if config.thinking_enabled {
        Some(ThinkingConfig {
            enabled: true,
            budget_tokens: config.thinking_budget,
        })
    } else {
        None
    };

    loop {
        iterations += 1;

        if iterations > config.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            break;
        }

        // Rebuild tool list each iteration so enable_tool activations take effect
        let active = tool_ctx
            .active_tools
            .read()
            .expect("active_tools lock") // INVARIANT: RwLock read, short critical section, poisoned = prior panic
            .clone();
        let tool_defs = tools.to_hermeneus_tools_filtered(&active);

        // Derive server tools from config + active_tools (enable_tool activations)
        let server_tools = if let Some(services) = tool_ctx.services.as_deref() {
            let mut st = services.server_tool_config.active_definitions(&active);
            // Also include any raw server_tools from NousConfig for backward compat
            st.extend(config.server_tools.clone());
            st
        } else {
            config.server_tools.clone()
        };

        let request = CompletionRequest {
            model: config.model.clone(),
            system: ctx.system_prompt.clone(),
            messages: messages.clone(),
            max_tokens: config.max_output_tokens,
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

        total_usage.input_tokens += response.usage.input_tokens;
        total_usage.output_tokens += response.usage.output_tokens;
        total_usage.cache_read_tokens += response.usage.cache_read_tokens;
        total_usage.cache_write_tokens += response.usage.cache_write_tokens;
        total_usage.llm_calls += 1;

        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_uses: Vec<(String, String, serde_json::Value)> = Vec::new();

        for block in &response.content {
            match block {
                ContentBlock::Text { text, .. } => text_parts.push(text.clone()),
                ContentBlock::ToolUse { id, name, input } => {
                    tool_uses.push((id.clone(), name.clone(), input.clone()));
                }
                ContentBlock::Thinking { thinking, .. } => {
                    debug!(len = thinking.len(), "thinking block received");
                }
                ContentBlock::ServerToolUse { name, .. } if name == "web_search" => {
                    used_server_web_search = true;
                }
                ContentBlock::ServerToolUse { name, .. } if name == "code_execution" => {
                    used_server_code_execution = true;
                }
                ContentBlock::CodeExecutionResult {
                    code, return_code, ..
                } => {
                    used_server_code_execution = true;
                    debug!(
                        code_len = code.len(),
                        return_code, "server code execution result received"
                    );
                }
                _ => {}
            }
        }

        final_content = text_parts.join("");
        final_stop_reason = response.stop_reason.to_string();

        // Only break if there are no LOCAL tool uses to dispatch.
        // Server tool results (web search, code execution) do not require client tool_result.
        if tool_uses.is_empty() || response.stop_reason != StopReason::ToolUse {
            break;
        }

        messages.push(Message {
            role: Role::Assistant,
            content: Content::Blocks(response.content.clone()),
        });

        let tool_results = dispatch_tools(
            &tool_uses,
            tools,
            tool_ctx,
            &mut loop_detector,
            &mut all_tool_calls,
            iterations,
        )
        .await?;

        messages.push(Message {
            role: Role::User,
            content: Content::Blocks(tool_results),
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

/// Streaming execute stage — same as [`execute`] but emits real-time events.
///
/// Uses `complete_streaming()` when the provider supports it, falling back to
/// `complete()` otherwise. Tool start/result events are emitted via the channel.
#[expect(
    clippy::too_many_lines,
    reason = "streaming variant parallels execute() structure"
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
    let streaming_provider = providers.find_streaming_provider(&config.model);

    // Fall back to non-streaming if no streaming provider available
    let Some(streaming_provider) = streaming_provider else {
        return execute(ctx, session, config, providers, tools, tool_ctx).await;
    };

    let provider = providers.find_provider(&config.model).ok_or_else(|| {
        error::PipelineStageSnafu {
            stage: "execute",
            message: format!("no provider for model: {}", config.model),
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

    let mut messages = build_messages(&ctx.messages);
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut total_usage = TurnUsage::default();
    let mut loop_detector = LoopDetector::new(config.loop_detection_threshold);
    let mut iterations: u32 = 0;
    let mut final_content = String::new();
    let mut final_stop_reason = String::new();
    let mut used_server_web_search = false;
    let mut used_server_code_execution = false;

    let thinking = if config.thinking_enabled {
        Some(ThinkingConfig {
            enabled: true,
            budget_tokens: config.thinking_budget,
        })
    } else {
        None
    };

    let tool_defs = tools.to_hermeneus_tools();

    loop {
        iterations += 1;

        if iterations > config.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            break;
        }

        // Derive server tools from ServerToolConfig + NousConfig
        let active = tool_ctx
            .active_tools
            .read()
            .expect("active_tools lock")
            .clone();
        let server_tools = if let Some(services) = tool_ctx.services.as_deref() {
            let mut st = services.server_tool_config.active_definitions(&active);
            st.extend(config.server_tools.clone());
            st
        } else {
            config.server_tools.clone()
        };

        let request = CompletionRequest {
            model: config.model.clone(),
            system: ctx.system_prompt.clone(),
            messages: messages.clone(),
            max_tokens: config.max_output_tokens,
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
            .complete_streaming(&request, |event| {
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

        total_usage.input_tokens += response.usage.input_tokens;
        total_usage.output_tokens += response.usage.output_tokens;
        total_usage.cache_read_tokens += response.usage.cache_read_tokens;
        total_usage.cache_write_tokens += response.usage.cache_write_tokens;
        total_usage.llm_calls += 1;

        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_uses: Vec<(String, String, serde_json::Value)> = Vec::new();

        for block in &response.content {
            match block {
                ContentBlock::Text { text, .. } => text_parts.push(text.clone()),
                ContentBlock::ToolUse { id, name, input } => {
                    tool_uses.push((id.clone(), name.clone(), input.clone()));
                }
                ContentBlock::Thinking { thinking, .. } => {
                    debug!(len = thinking.len(), "thinking block received");
                }
                ContentBlock::ServerToolUse { name, .. } if name == "web_search" => {
                    used_server_web_search = true;
                }
                ContentBlock::ServerToolUse { name, .. } if name == "code_execution" => {
                    used_server_code_execution = true;
                }
                ContentBlock::CodeExecutionResult {
                    code, return_code, ..
                } => {
                    used_server_code_execution = true;
                    debug!(
                        code_len = code.len(),
                        return_code, "server code execution result received"
                    );
                }
                _ => {}
            }
        }

        final_content = text_parts.join("");
        final_stop_reason = response.stop_reason.to_string();

        if tool_uses.is_empty() || response.stop_reason != StopReason::ToolUse {
            break;
        }

        messages.push(Message {
            role: Role::Assistant,
            content: Content::Blocks(response.content.clone()),
        });

        let tool_results = dispatch_tools_streaming(
            &tool_uses,
            tools,
            tool_ctx,
            &mut loop_detector,
            &mut all_tool_calls,
            iterations,
            stream_tx,
        )
        .await?;

        messages.push(Message {
            role: Role::User,
            content: Content::Blocks(tool_results),
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
