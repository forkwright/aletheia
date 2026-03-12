//! Dispatch helpers — tool execution, signal classification, message conversion.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use tracing::debug;

use aletheia_hermeneus::types::{ContentBlock, ToolResultContent};
use aletheia_koina::id::ToolName;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{ToolContext, ToolInput};
use tokio::sync::mpsc;

use crate::error;
use crate::pipeline::{InteractionSignal, LoopDetector, ToolCall};
use crate::stream::TurnStreamEvent;

/// Hash a JSON value for loop detection using the standard library hasher.
pub(super) fn simple_hash(value: &serde_json::Value) -> String {
    let mut hasher = DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Classify the interaction signals based on tool calls and content.
pub(super) fn classify_signals(
    tool_calls: &[ToolCall],
    _content: &str,
    used_server_web_search: bool,
    used_server_code_execution: bool,
) -> Vec<InteractionSignal> {
    let mut signals = Vec::new();
    let used_any_server_tool = used_server_web_search || used_server_code_execution;

    if tool_calls.is_empty() && !used_any_server_tool {
        signals.push(InteractionSignal::Conversation);
    } else {
        if !tool_calls.is_empty() || used_any_server_tool {
            signals.push(InteractionSignal::ToolExecution);
        }

        let code_tools = ["write", "edit", "exec"];
        if used_server_code_execution
            || tool_calls
                .iter()
                .any(|tc| code_tools.contains(&tc.name.as_str()))
        {
            signals.push(InteractionSignal::CodeGeneration);
        }

        let research_tools = ["web_search", "web_fetch"];
        if used_server_web_search
            || tool_calls
                .iter()
                .any(|tc| research_tools.contains(&tc.name.as_str()))
        {
            signals.push(InteractionSignal::Research);
        }

        if tool_calls.iter().any(|tc| tc.is_error) {
            signals.push(InteractionSignal::ErrorRecovery);
        }
    }

    signals
}

/// Convert pipeline messages to hermeneus messages.
pub(super) fn build_messages(
    pipeline_messages: &[crate::pipeline::PipelineMessage],
) -> Vec<aletheia_hermeneus::types::Message> {
    use aletheia_hermeneus::types::{Content, Message, Role};

    pipeline_messages
        .iter()
        .map(|m| Message {
            role: match m.role.as_str() {
                "assistant" => Role::Assistant,
                _ => Role::User,
            },
            content: Content::Text(m.content.clone()),
        })
        .collect()
}

/// Dispatch tool calls from an LLM response and collect results.
pub(super) async fn dispatch_tools(
    tool_uses: &[(String, String, serde_json::Value)],
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    loop_detector: &mut LoopDetector,
    all_tool_calls: &mut Vec<ToolCall>,
    iterations: u32,
) -> error::Result<Vec<ContentBlock>> {
    let mut tool_results: Vec<ContentBlock> = Vec::new();

    for (tool_id, tool_name, tool_input) in tool_uses {
        let input_hash = simple_hash(tool_input);
        if let Some(pattern) = loop_detector.record(tool_name, &input_hash) {
            return Err(error::LoopDetectedSnafu {
                iterations,
                pattern,
            }
            .build());
        }

        let tool_name_id = ToolName::new(tool_name.as_str()).map_err(|_err| {
            error::PipelineStageSnafu {
                stage: "execute",
                message: format!("invalid tool name: {tool_name}"),
            }
            .build()
        })?;

        let start = std::time::Instant::now();
        let result = tools
            .execute(
                &ToolInput {
                    name: tool_name_id,
                    tool_use_id: tool_id.clone(),
                    arguments: tool_input.clone(),
                },
                tool_ctx,
            )
            .await;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "tool execution duration won't exceed u64::MAX milliseconds"
        )]
        let duration_ms = start.elapsed().as_millis() as u64;

        let (content, is_error) = match result {
            Ok(r) => (r.content, r.is_error),
            Err(e) => (ToolResultContent::text(format!("Tool error: {e}")), true),
        };

        debug!(
            tool = tool_name.as_str(),
            duration_ms, is_error, "tool executed"
        );

        all_tool_calls.push(ToolCall {
            id: tool_id.clone(),
            name: tool_name.clone(),
            input: tool_input.clone(),
            result: Some(content.text_summary()),
            is_error,
            duration_ms,
        });

        tool_results.push(ContentBlock::ToolResult {
            tool_use_id: tool_id.clone(),
            content,
            is_error: Some(is_error),
        });
    }

    Ok(tool_results)
}

/// Dispatch tool calls with streaming events emitted to the channel.
pub(super) async fn dispatch_tools_streaming(
    tool_uses: &[(String, String, serde_json::Value)],
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    loop_detector: &mut LoopDetector,
    all_tool_calls: &mut Vec<ToolCall>,
    iterations: u32,
    stream_tx: &mpsc::Sender<TurnStreamEvent>,
) -> error::Result<Vec<ContentBlock>> {
    let mut tool_results: Vec<ContentBlock> = Vec::new();

    for (tool_id, tool_name, tool_input) in tool_uses {
        let input_hash = simple_hash(tool_input);
        if let Some(pattern) = loop_detector.record(tool_name, &input_hash) {
            return Err(error::LoopDetectedSnafu {
                iterations,
                pattern,
            }
            .build());
        }

        let tool_name_id = ToolName::new(tool_name.as_str()).map_err(|_err| {
            error::PipelineStageSnafu {
                stage: "execute",
                message: format!("invalid tool name: {tool_name}"),
            }
            .build()
        })?;

        let _ = stream_tx.try_send(TurnStreamEvent::ToolStart {
            tool_id: tool_id.clone(),
            tool_name: tool_name.clone(),
            input: tool_input.clone(),
        });

        let start = std::time::Instant::now();
        let result = tools
            .execute(
                &ToolInput {
                    name: tool_name_id,
                    tool_use_id: tool_id.clone(),
                    arguments: tool_input.clone(),
                },
                tool_ctx,
            )
            .await;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "tool execution duration won't exceed u64::MAX milliseconds"
        )]
        let duration_ms = start.elapsed().as_millis() as u64;

        let (content, is_error) = match result {
            Ok(r) => (r.content, r.is_error),
            Err(e) => (ToolResultContent::text(format!("Tool error: {e}")), true),
        };

        let result_summary = content.text_summary();

        debug!(
            tool = tool_name.as_str(),
            duration_ms, is_error, "tool executed"
        );

        let _ = stream_tx.try_send(TurnStreamEvent::ToolResult {
            tool_id: tool_id.clone(),
            tool_name: tool_name.clone(),
            result: result_summary.clone(),
            is_error,
            duration_ms,
        });

        all_tool_calls.push(ToolCall {
            id: tool_id.clone(),
            name: tool_name.clone(),
            input: tool_input.clone(),
            result: Some(result_summary),
            is_error,
            duration_ms,
        });

        tool_results.push(ContentBlock::ToolResult {
            tool_use_id: tool_id.clone(),
            content,
            is_error: Some(is_error),
        });
    }

    Ok(tool_results)
}
