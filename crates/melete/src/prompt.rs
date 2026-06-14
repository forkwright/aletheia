//! Distillation prompt construction.

use hermeneus::types::{Content, ContentBlock, Message, Role};

use crate::distill::DistillSection;

/// Generate the distillation system prompt from configured sections.
#[must_use]
pub(crate) fn build_system_prompt(sections: &[DistillSection]) -> String {
    let mut prompt = String::from(
        "You are a context distillation engine. Your task is to compress a conversation \
         history into a structured summary that preserves all essential information for \
         continuing the work.\n\n\
         Produce a summary with EXACTLY these sections:\n\n",
    );

    for section in sections {
        prompt.push_str(&section.heading());
        prompt.push('\n');
        prompt.push_str(section.description());
        prompt.push_str("\n\n");
    }

    prompt.push_str(
        "Rules:\n\
         - Use first person: \"I was...\", \"I decided...\"\n\
         - Be specific: file paths, line numbers, function names, exact values\n\
         - Preserve names, identifiers, and numbers exactly\n\
         - Target 400-600 words total\n\
         - Every fact in the summary must be traceable to the conversation\n\
         - If a section has no content, omit it entirely (don't include empty sections)",
    );

    prompt
}

/// Format conversation messages into readable text for the distillation LLM.
#[must_use]
pub(crate) fn format_messages(messages: &[Message], include_tool_calls: bool) -> String {
    let mut output = String::new();

    for msg in messages {
        let role_label = match msg.role {
            Role::System => "SYSTEM",
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
            _ => "UNKNOWN",
        };

        match &msg.content {
            Content::Text(text) => {
                output.push('[');
                output.push_str(role_label);
                output.push_str("]\n");
                output.push_str(text);
                output.push_str("\n\n");
            }
            Content::Blocks(blocks) => {
                let mut block_text = String::new();
                for block in blocks {
                    match block {
                        ContentBlock::Text { text, .. } => {
                            block_text.push_str(text);
                            block_text.push('\n');
                        }
                        ContentBlock::ToolUse { name, input, .. } if include_tool_calls => {
                            block_text.push_str("[Tool call: ");
                            block_text.push_str(name);
                            block_text.push('(');
                            block_text.push_str(&input.to_string());
                            block_text.push_str(")]\n");
                        }
                        ContentBlock::ToolResult {
                            content, is_error, ..
                        } if include_tool_calls => {
                            let prefix = if *is_error == Some(true) {
                                "Tool error"
                            } else {
                                "Tool result"
                            };
                            let summary = content.text_summary();
                            let truncated = truncate_tool_result(&summary);
                            block_text.push('[');
                            block_text.push_str(prefix);
                            block_text.push_str(": ");
                            block_text.push_str(truncated);
                            block_text.push_str("]\n");
                        }
                        ContentBlock::Thinking { thinking, .. } => {
                            block_text.push_str("[Thinking: ");
                            block_text.push_str(thinking);
                            block_text.push_str("]\n");
                        }
                        _ => {
                            // NOTE: other content block types not rendered in prompt summary
                        }
                    }
                }
                if !block_text.is_empty() {
                    output.push('[');
                    output.push_str(role_label);
                    output.push_str("]\n");
                    output.push_str(&block_text);
                    output.push('\n');
                }
            }
            _ => {
                // NOTE: future content variants rendered as empty
            }
        }
    }

    output
}

/// Default maximum character length for truncated tool results in distillation prompts.
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::distillation_max_tool_result_len`.
pub(crate) const DEFAULT_MAX_TOOL_RESULT_LEN: usize = 500;

/// Truncate long tool results to keep the distillation input manageable.
fn truncate_tool_result(content: &str) -> &str {
    if content.len() <= DEFAULT_MAX_TOOL_RESULT_LEN {
        content
    } else {
        let mut end = DEFAULT_MAX_TOOL_RESULT_LEN;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        content.get(..end).unwrap_or(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: Content::Text(text.to_owned()),
            cache_breakpoint: false,
        }
    }

    #[test]
    fn build_system_prompt_contains_all_standard_sections() {
        let prompt = build_system_prompt(&DistillSection::all_standard());
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
            assert!(prompt.contains(section), "missing section: {section}");
        }
    }

    #[test]
    fn build_system_prompt_default_sections() {
        let prompt = build_system_prompt(&DistillSection::all_standard());
        let expected = [
            "## Summary",
            "## Task Context",
            "## Completed Work",
            "## Key Decisions",
            "## Current State",
            "## Open Threads",
            "## Corrections",
        ];
        for section in expected {
            assert!(prompt.contains(section), "missing section: {section}");
        }
    }

    #[test]
    fn build_system_prompt_custom_section() {
        let sections = vec![
            DistillSection::Summary,
            DistillSection::Custom {
                name: "Architecture Notes".to_owned(),
                description: "Record architectural decisions and their trade-offs.".to_owned(),
            },
        ];
        let prompt = build_system_prompt(&sections);
        assert!(prompt.contains("## Architecture Notes"));
        assert!(prompt.contains("Record architectural decisions"));
    }

    #[test]
    fn build_system_prompt_contains_omit_rule() {
        let prompt = build_system_prompt(&DistillSection::all_standard());
        assert!(prompt.contains("omit it entirely"));
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
                    citations: None,
                },
                ContentBlock::ToolUse {
                    id: "t1".to_owned(),
                    name: "read_file".to_owned(),
                    input: serde_json::json!({"path": "/tmp/test"}),
                },
            ]),
            cache_breakpoint: false,
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
                content: hermeneus::types::ToolResultContent::text("file contents here"),
                is_error: Some(false),
            }]),
            cache_breakpoint: false,
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
