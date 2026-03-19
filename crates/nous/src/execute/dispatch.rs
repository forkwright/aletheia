//! Dispatch helpers: tool execution, signal classification, message conversion.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use tracing::debug;

use tokio::sync::mpsc;

use aletheia_hermeneus::types::{ContentBlock, ToolResultBlock, ToolResultContent};
use aletheia_koina::id::ToolName;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{ToolContext, ToolInput};

use crate::error;
use crate::pipeline::{InteractionSignal, LoopDetector, ToolCall};
use crate::stream::TurnStreamEvent;

/// Truncate a tool result if it exceeds `max_bytes`.
///
/// Only text content is truncated; image and document blocks are left
/// intact because they are binary data that cannot be meaningfully
/// split at arbitrary byte boundaries. When truncation occurs, the
/// text is cut at the last char boundary within the limit and a
/// `[truncated: {original} -> {truncated} bytes]` indicator is appended.
///
/// A `max_bytes` of `0` disables truncation entirely.
pub(crate) fn truncate_tool_result(
    content: ToolResultContent,
    max_bytes: u32,
) -> ToolResultContent {
    if max_bytes == 0 {
        return content;
    }
    #[expect(
        clippy::as_conversions,
        reason = "u32→usize: max_bytes always fits in usize"
    )]
    let limit = max_bytes as usize;

    match content {
        ToolResultContent::Text(text) => {
            if text.len() <= limit {
                return ToolResultContent::Text(text);
            }
            let original_len = text.len();
            // WHY: truncate at a char boundary to avoid producing invalid UTF-8.
            let truncated = truncate_at_char_boundary(&text, limit);
            let indicator = format!(
                "\n[truncated: {} -> {} bytes]",
                original_len,
                truncated.len()
            );
            debug!(
                original_bytes = original_len,
                truncated_bytes = truncated.len(),
                "tool result truncated"
            );
            ToolResultContent::Text(format!("{truncated}{indicator}"))
        }
        ToolResultContent::Blocks(blocks) => {
            let total: usize = blocks
                .iter()
                .map(|b| match b {
                    ToolResultBlock::Text { text } => text.len(),
                    _ => 0,
                })
                .sum();

            if total <= limit {
                return ToolResultContent::Blocks(blocks);
            }

            debug!(
                original_bytes = total,
                limit_bytes = limit,
                "tool result blocks truncated"
            );

            let mut remaining = limit;
            let mut out = Vec::with_capacity(blocks.len());
            for block in blocks {
                match block {
                    ToolResultBlock::Text { text } => {
                        if remaining == 0 {
                            continue;
                        }
                        if text.len() <= remaining {
                            remaining -= text.len();
                            out.push(ToolResultBlock::Text { text });
                        } else {
                            let truncated = truncate_at_char_boundary(&text, remaining);
                            remaining = 0;
                            out.push(ToolResultBlock::Text {
                                text: truncated.to_owned(),
                            });
                        }
                    }
                    other => out.push(other),
                }
            }
            let indicator = format!("\n[truncated: {total} -> {limit} bytes]");
            out.push(ToolResultBlock::Text { text: indicator });
            ToolResultContent::Blocks(out)
        }
        _ => content,
    }
}

/// Find the largest prefix of `s` that is at most `max_bytes` bytes and
/// ends on a UTF-8 char boundary.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // WHY: floor_char_boundary rounds down to the nearest char boundary,
    // avoiding a panic or invalid slice from splitting mid-codepoint.
    let end = s.floor_char_boundary(max_bytes);
    s.get(..end).unwrap_or(s)
}

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
    max_tool_result_bytes: u32,
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
            clippy::as_conversions,
            reason = "u128→u64: tool execution duration won't exceed u64::MAX milliseconds"
        )]
        let duration_ms = start.elapsed().as_millis() as u64;

        let (content, is_error) = match result {
            Ok(r) => (r.content, r.is_error),
            Err(e) => (ToolResultContent::text(format!("Tool error: {e}")), true),
        };

        let content = truncate_tool_result(content, max_tool_result_bytes);

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
#[expect(
    clippy::too_many_arguments,
    reason = "streaming dispatch inherently needs context, detector, channel, and limit"
)]
pub(super) async fn dispatch_tools_streaming(
    tool_uses: &[(String, String, serde_json::Value)],
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    loop_detector: &mut LoopDetector,
    all_tool_calls: &mut Vec<ToolCall>,
    iterations: u32,
    stream_tx: &mpsc::Sender<TurnStreamEvent>,
    max_tool_result_bytes: u32,
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
            clippy::as_conversions,
            reason = "u128→u64: tool execution duration won't exceed u64::MAX milliseconds"
        )]
        let duration_ms = start.elapsed().as_millis() as u64;

        let (content, is_error) = match result {
            Ok(r) => (r.content, r.is_error),
            Err(e) => (ToolResultContent::text(format!("Tool error: {e}")), true),
        };

        let content = truncate_tool_result(content, max_tool_result_bytes);
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn text_within_limit_passes_through() {
        let content = ToolResultContent::text("hello world");
        let result = truncate_tool_result(content, 100);
        match result {
            ToolResultContent::Text(s) => assert_eq!(s, "hello world"),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn text_at_exact_limit_passes_through() {
        let text = "a".repeat(50);
        let content = ToolResultContent::text(text.clone());
        let result = truncate_tool_result(content, 50);
        match result {
            ToolResultContent::Text(s) => assert_eq!(s, text),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn text_over_limit_is_truncated_with_indicator() {
        let text = "a".repeat(100);
        let result = truncate_tool_result(ToolResultContent::text(text), 50);
        match result {
            ToolResultContent::Text(s) => {
                assert!(
                    s.contains("[truncated: 100 -> 50 bytes]"),
                    "missing truncation indicator in: {s}"
                );
                assert!(
                    s.starts_with("aaaa"),
                    "truncated content should preserve prefix"
                );
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn zero_limit_disables_truncation() {
        let text = "a".repeat(100_000);
        let content = ToolResultContent::text(text.clone());
        let result = truncate_tool_result(content, 0);
        match result {
            ToolResultContent::Text(s) => assert_eq!(s.len(), 100_000),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn multibyte_chars_truncated_at_char_boundary() {
        let text = "\u{1F600}\u{1F601}\u{1F602}";
        assert_eq!(text.len(), 12, "test setup: 3 emojis = 12 bytes");

        let result = truncate_tool_result(ToolResultContent::text(text), 5);
        match result {
            ToolResultContent::Text(s) => {
                assert!(
                    s.starts_with('\u{1F600}'),
                    "should keep first complete emoji"
                );
                assert!(
                    s.contains("[truncated: 12 -> 4 bytes]"),
                    "indicator should show char-boundary size: {s}"
                );
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn blocks_within_limit_pass_through() {
        let blocks = vec![
            ToolResultBlock::Text {
                text: "hello".to_owned(),
            },
            ToolResultBlock::Text {
                text: "world".to_owned(),
            },
        ];
        let content = ToolResultContent::Blocks(blocks);
        let result = truncate_tool_result(content, 100);
        match result {
            ToolResultContent::Blocks(bs) => {
                assert_eq!(bs.len(), 2, "both blocks should pass through");
            }
            _ => panic!("expected Blocks variant"),
        }
    }

    #[test]
    fn blocks_over_limit_truncates_text_preserves_images() {
        let blocks = vec![
            ToolResultBlock::Text {
                text: "a".repeat(80),
            },
            ToolResultBlock::Image {
                source: aletheia_hermeneus::types::ImageSource {
                    source_type: "base64".to_owned(),
                    media_type: "image/png".to_owned(),
                    data: "base64data".to_owned(),
                },
            },
            ToolResultBlock::Text {
                text: "b".repeat(40),
            },
        ];
        let content = ToolResultContent::Blocks(blocks);
        let result = truncate_tool_result(content, 50);
        match result {
            ToolResultContent::Blocks(bs) => {
                let has_image = bs
                    .iter()
                    .any(|b| matches!(b, ToolResultBlock::Image { .. }));
                assert!(has_image, "image blocks should be preserved");

                let indicator_block = bs.last().expect("should have indicator block");
                match indicator_block {
                    ToolResultBlock::Text { text } => {
                        assert!(
                            text.contains("[truncated: 120 -> 50 bytes]"),
                            "indicator should show total text sizes: {text}"
                        );
                    }
                    _ => panic!("last block should be text indicator"),
                }
            }
            _ => panic!("expected Blocks variant"),
        }
    }
}
