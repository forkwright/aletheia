//! Tests for context limit enforcement, `nous_id` sanitization, token estimation, and summary parsing.
use aletheia_hermeneus::types::{ContentBlock, ToolResultContent};

use super::super::super::*;
use super::{MOCK_SUMMARY, default_engine, sample_conversation, success_provider, text_msg};

#[test]
fn enforce_context_limit_returns_zero_when_within_window() {
    let mut messages = sample_conversation();
    let count_before = messages.len();
    let dropped = enforce_context_limit(&mut messages, 1_000_000);
    assert_eq!(
        dropped, 0,
        "no messages should be dropped when within the context window"
    );
    assert_eq!(
        messages.len(),
        count_before,
        "message count should be unchanged when within the context window"
    );
}

#[test]
fn enforce_context_limit_drops_oldest_messages_when_over() {
    let mut messages: Vec<Message> = (0..10)
        .map(|i| text_msg(Role::User, &"x".repeat(100 + i)))
        .collect();
    let initial_count = messages.len();
    let dropped = enforce_context_limit(&mut messages, 4);
    assert!(dropped > 0, "should have dropped some messages");
    assert_eq!(
        messages.len(),
        initial_count - dropped,
        "remaining message count should equal initial minus dropped"
    );
}

#[test]
fn enforce_context_limit_keeps_at_least_one_message() {
    let mut messages = vec![text_msg(Role::User, "x".repeat(1000).as_str())];
    // NOTE: window of 1 token: impossible to satisfy, but we must keep the last message
    let dropped = enforce_context_limit(&mut messages, 1);
    assert_eq!(dropped, 0, "single message must not be dropped");
    assert_eq!(
        messages.len(),
        1,
        "single message should remain after enforce_context_limit even if oversized"
    );
}

#[test]
fn enforce_context_limit_drops_from_front() {
    let mut messages = vec![
        text_msg(Role::User, &"a".repeat(400)), // oldest: should be dropped
        text_msg(Role::User, &"b".repeat(400)),
        text_msg(Role::User, &"c".repeat(4)), // newest: should be kept
    ];
    // NOTE: 201 total tokens, window of 2 keeps last ~8 chars = 2 tokens
    let dropped = enforce_context_limit(&mut messages, 2);
    assert!(
        dropped > 0,
        "at least one oversized message should be dropped"
    );
    assert!(
        messages
            .last()
            .expect("messages should not be empty after enforce_context_limit")
            .content
            .text()
            .starts_with('c'),
        "newest message (starting with 'c') should be kept"
    );
}

#[test]
fn sanitize_nous_id_clean_string_unchanged() {
    assert_eq!(
        sanitize_nous_id("my-agent-01"),
        "my-agent-01",
        "clean string with no special characters should be unchanged"
    );
}

#[test]
fn sanitize_nous_id_removes_backtick() {
    assert_eq!(
        sanitize_nous_id("agent`hack"),
        "agenthack",
        "backtick should be removed from nous_id"
    );
}

#[test]
fn sanitize_nous_id_removes_newline() {
    assert_eq!(
        sanitize_nous_id("agent\nhack"),
        "agenthack",
        "newline should be removed from nous_id"
    );
}

#[test]
fn sanitize_nous_id_removes_carriage_return() {
    assert_eq!(
        sanitize_nous_id("agent\rhack"),
        "agenthack",
        "carriage return should be removed from nous_id"
    );
}

#[test]
fn sanitize_nous_id_removes_control_chars() {
    assert_eq!(
        sanitize_nous_id("agent\x00\x1bhack"),
        "agenthack",
        "control characters should be removed from nous_id"
    );
}

#[test]
fn build_prompt_sanitizes_backtick_in_nous_id() {
    let engine = default_engine();
    let request = engine.build_prompt(&sample_conversation(), "id`injection");
    let user_text = request.messages[0].content.text();
    assert!(
        !user_text.contains('`'),
        "backtick must not appear in prompt"
    );
    assert!(
        user_text.contains("idinjection"),
        "sanitized nous_id without backtick should appear in prompt"
    );
}

#[test]
fn build_prompt_sanitizes_newline_in_nous_id() {
    let engine = default_engine();
    let request = engine.build_prompt(&sample_conversation(), "id\ninjection");
    let user_text = request.messages[0].content.text();
    // NOTE: newline must be stripped from inside the nous_id quoted span
    assert!(
        !user_text.contains("\"id\ninjection\""),
        "raw newline must not appear inside the quoted nous_id"
    );
    assert!(
        user_text.contains("\"idinjection\""),
        "sanitized nous_id should appear without the embedded newline"
    );
}

#[test]
fn estimate_tokens_includes_tool_use_input() {
    let msg_text_only = Message {
        role: Role::Assistant,
        content: Content::Blocks(vec![ContentBlock::Text {
            text: "checking".to_owned(),
            citations: None,
        }]),
    };
    let msg_with_tool = Message {
        role: Role::Assistant,
        content: Content::Blocks(vec![
            ContentBlock::Text {
                text: "checking".to_owned(),
                citations: None,
            },
            ContentBlock::ToolUse {
                id: "t1".to_owned(),
                name: "read_file".to_owned(),
                input: serde_json::json!({"path": "/very/long/path/to/some/file.rs"}),
            },
        ]),
    };
    let tokens_text = estimate_tokens(&[msg_text_only]);
    let tokens_tool = estimate_tokens(&[msg_with_tool]);
    assert!(
        tokens_tool > tokens_text,
        "tool use input should increase token estimate: {tokens_tool} vs {tokens_text}"
    );
}

#[test]
fn estimate_tokens_includes_tool_result_content() {
    let msg_empty_result = Message {
        role: Role::User,
        content: Content::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "t1".to_owned(),
            content: ToolResultContent::text(""),
            is_error: Some(false),
        }]),
    };
    let msg_large_result = Message {
        role: Role::User,
        content: Content::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "t1".to_owned(),
            content: ToolResultContent::text("x".repeat(400)),
            is_error: Some(false),
        }]),
    };
    let tokens_empty = estimate_tokens(&[msg_empty_result]);
    let tokens_large = estimate_tokens(&[msg_large_result]);
    assert!(
        tokens_large > tokens_empty,
        "tool result content should increase token estimate: {tokens_large} vs {tokens_empty}"
    );
}

#[tokio::test]
async fn distill_result_contains_memory_flush_field() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = success_provider(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed with a valid provider");
    // NOTE: assert the field exists: mock summary has no decisions to assert content on
    let _ = &result.memory_flush;
}

#[test]
fn parse_summary_extracts_key_decisions() {
    let summary = "\
## Summary
Fixed login bug.

## Key Decisions
- Decision: Use null check. Reason: Minimal fix.
- Decision: Keep auth module. Reason: Already tested.

## Corrections
- Wrong file initially.
";
    let flush = parse_summary_to_flush(summary, "2026-03-13T00:00:00Z");
    assert_eq!(
        flush.decisions.len(),
        2,
        "should extract exactly 2 decisions from the summary"
    );
    assert!(
        flush.decisions[0]
            .content
            .contains("Decision: Use null check"),
        "first extracted decision should contain the null check decision"
    );
    assert!(
        flush.decisions[1]
            .content
            .contains("Decision: Keep auth module"),
        "second extracted decision should contain the keep auth module decision"
    );
}

#[test]
fn parse_summary_extracts_corrections() {
    let summary = "\
## Summary
Fixed auth.

## Corrections
- Wrong file at first.
- Missed the null check.
";
    let flush = parse_summary_to_flush(summary, "2026-03-13T00:00:00Z");
    assert_eq!(
        flush.corrections.len(),
        2,
        "should extract exactly 2 corrections from the summary"
    );
    assert!(
        flush.corrections[0].content.contains("Wrong file at first"),
        "first correction should describe the wrong-file mistake"
    );
}

#[test]
fn parse_summary_extracts_task_context() {
    let summary = "\
## Summary
Auth work.

## Task Context
Working on the login flow for nous agent \"syn\".
Fixing the null pointer crash.

## Current State
Done.
";
    let flush = parse_summary_to_flush(summary, "2026-03-13T00:00:00Z");
    assert!(
        flush.task_state.is_some(),
        "task_state should be populated when Task Context section is present"
    );
    let state = flush
        .task_state
        .expect("task_state should be Some when Task Context section is present");
    assert!(
        state.contains("login flow"),
        "task_state should contain the login flow context from the summary"
    );
}

#[test]
fn parse_summary_empty_sections_produce_no_items() {
    let summary = "## Summary\nJust a summary.\n\n## Key Decisions\n\n## Corrections\n";
    let flush = parse_summary_to_flush(summary, "2026-03-13T00:00:00Z");
    assert!(
        flush.decisions.is_empty(),
        "empty Key Decisions section should produce no decision items"
    );
    assert!(
        flush.corrections.is_empty(),
        "empty Corrections section should produce no correction items"
    );
    assert!(
        flush.task_state.is_none(),
        "missing Task Context section should leave task_state as None"
    );
}

#[test]
fn parse_summary_flush_source_is_extracted() {
    let summary = "## Key Decisions\n- Decision: Use snafu. Reason: Standard.\n";
    let flush = parse_summary_to_flush(summary, "2026-03-13T00:00:00Z");
    assert_eq!(
        flush.decisions.len(),
        1,
        "should extract exactly 1 decision from the summary"
    );
    assert!(
        matches!(flush.decisions[0].source, FlushSource::Extracted),
        "extracted decision source should be FlushSource::Extracted"
    );
}
