//! Distillation prompt construction.

use std::fmt::Write;

use aletheia_hermeneus::types::{Content, ContentBlock, Message, Role};

pub const DISTILLATION_SYSTEM_PROMPT: &str = "\
You are a context distillation engine. Your task is to compress a conversation \
history into a structured summary that preserves all essential information for \
continuing the work.

Produce a summary with EXACTLY these sections. Omit a section only if it has \
no content.

## Summary
One sentence describing what this conversation is about.

## Task Context
What was being worked on and why. Include the agent/nous identity if relevant.

## Completed Work
- Bullet list of concrete actions taken and their outcomes
- Include file paths, function names, and specific details
- Focus on results, not process

## Key Decisions
- Decisions made with their rationale — these MUST be preserved
- Format: \"Decision: X. Reason: Y.\"

## Current State
Where things stand right now. What is done, what is in progress, what is half-finished.

## Open Threads
- Unfinished items, pending questions, next steps
- Items deferred for later

## Corrections
- Anything that was wrong and corrected
- Mistakes made and how they were fixed
- These prevent repeating errors

Rules:
- Use first person: \"I was...\", \"I decided...\"
- Be specific: file paths, line numbers, function names, exact values
- Preserve names, identifiers, and numbers exactly
- Target 400-600 words total
- Every fact in the summary must be traceable to the conversation";

/// Format conversation messages into readable text for the distillation LLM.
pub fn format_messages(messages: &[Message], include_tool_calls: bool) -> String {
    let mut output = String::new();

    for msg in messages {
        let role_label = match msg.role {
            Role::System => "SYSTEM",
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
        };

        match &msg.content {
            Content::Text(text) => {
                let _ = writeln!(output, "[{role_label}]\n{text}\n");
            }
            Content::Blocks(blocks) => {
                let mut block_text = String::new();
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => {
                            block_text.push_str(text);
                            block_text.push('\n');
                        }
                        ContentBlock::ToolUse { name, input, .. } if include_tool_calls => {
                            let _ = writeln!(block_text, "[Tool call: {name}({input})]");
                        }
                        ContentBlock::ToolResult {
                            content, is_error, ..
                        } if include_tool_calls => {
                            let prefix = if *is_error == Some(true) {
                                "Tool error"
                            } else {
                                "Tool result"
                            };
                            let truncated = truncate_tool_result(content);
                            let _ = writeln!(block_text, "[{prefix}: {truncated}]");
                        }
                        ContentBlock::Thinking { thinking } => {
                            let _ = writeln!(block_text, "[Thinking: {thinking}]");
                        }
                        _ => {}
                    }
                }
                if !block_text.is_empty() {
                    let _ = writeln!(output, "[{role_label}]\n{block_text}");
                }
            }
        }
    }

    output
}

/// Truncate long tool results to keep the distillation input manageable.
fn truncate_tool_result(content: &str) -> &str {
    const MAX_TOOL_RESULT_LEN: usize = 500;
    if content.len() <= MAX_TOOL_RESULT_LEN {
        content
    } else {
        // Find a safe UTF-8 boundary near the limit
        let mut end = MAX_TOOL_RESULT_LEN;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        &content[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: Content::Text(text.to_owned()),
        }
    }

    #[test]
    fn system_prompt_contains_all_sections() {
        let sections = [
            "## Summary",
            "## Task Context",
            "## Completed Work",
            "## Key Decisions",
            "## Current State",
            "## Open Threads",
            "## Corrections",
        ];
        for section in sections {
            assert!(
                DISTILLATION_SYSTEM_PROMPT.contains(section),
                "missing section: {section}"
            );
        }
    }

    #[test]
    fn format_text_messages() {
        let messages = vec![
            text_msg(Role::User, "Hello"),
            text_msg(Role::Assistant, "Hi there"),
        ];
        let formatted = format_messages(&messages, true);
        assert!(formatted.contains("[USER]"));
        assert!(formatted.contains("Hello"));
        assert!(formatted.contains("[ASSISTANT]"));
        assert!(formatted.contains("Hi there"));
    }

    #[test]
    fn format_includes_tool_calls_when_enabled() {
        let messages = vec![Message {
            role: Role::Assistant,
            content: Content::Blocks(vec![
                ContentBlock::Text {
                    text: "Let me check.".to_owned(),
                },
                ContentBlock::ToolUse {
                    id: "t1".to_owned(),
                    name: "read_file".to_owned(),
                    input: serde_json::json!({"path": "/tmp/test"}),
                },
            ]),
        }];
        let with_tools = format_messages(&messages, true);
        assert!(with_tools.contains("[Tool call: read_file"));

        let without_tools = format_messages(&messages, false);
        assert!(!without_tools.contains("[Tool call:"));
    }

    #[test]
    fn format_excludes_tool_results_when_disabled() {
        let messages = vec![Message {
            role: Role::User,
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_owned(),
                content: "file contents here".to_owned(),
                is_error: Some(false),
            }]),
        }];
        let with_tools = format_messages(&messages, true);
        assert!(with_tools.contains("[Tool result:"));

        let without_tools = format_messages(&messages, false);
        assert!(!without_tools.contains("[Tool result:"));
    }

    #[test]
    fn truncate_long_tool_result() {
        let long = "x".repeat(1000);
        let result = truncate_tool_result(&long);
        assert!(result.len() <= 500);
    }

    #[test]
    fn truncate_short_tool_result_unchanged() {
        let short = "short result";
        assert_eq!(truncate_tool_result(short), short);
    }
}
