//! Tests for distillation threshold checks and prompt building.
#![expect(clippy::expect_used, reason = "test assertions")]
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{CompletionResponse, ContentBlock, StopReason, Usage};

use super::super::*;

fn distill_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        id: "msg_distill_1".to_owned(),
        model: "claude-sonnet-4-20250514".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 5000,
            output_tokens: 200,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
    }
}

fn success_provider(summary: &str) -> MockProvider {
    MockProvider::with_responses(vec![distill_response(summary)])
        .models(&["claude-sonnet-4-20250514"])
        .named("mock-distill")
}

fn empty_response_provider() -> MockProvider {
    MockProvider::with_responses(vec![CompletionResponse {
        id: "msg_empty".to_owned(),
        model: "claude-sonnet-4-20250514".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![],
        usage: Usage::default(),
    }])
    .models(&["claude-sonnet-4-20250514"])
    .named("mock-distill")
}

fn empty_text_provider() -> MockProvider {
    MockProvider::with_responses(vec![CompletionResponse {
        id: "msg_empty_text".to_owned(),
        model: "claude-sonnet-4-20250514".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![
            ContentBlock::Text {
                text: String::new(),
                citations: None,
            },
            ContentBlock::Text {
                text: "   ".to_owned(),
                citations: None,
            },
        ],
        usage: Usage::default(),
    }])
    .models(&["claude-sonnet-4-20250514"])
    .named("mock-distill")
}

fn failure_provider() -> MockProvider {
    MockProvider::error("network timeout")
        .models(&["claude-sonnet-4-20250514"])
        .named("mock-distill")
}

fn text_msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: Content::Text(text.to_owned()),
    }
}

fn sample_conversation() -> Vec<Message> {
    vec![
        text_msg(Role::User, "Help me fix the login bug"),
        text_msg(Role::Assistant, "I'll look at the auth module"),
        text_msg(Role::User, "It's in src/auth/login.rs"),
        text_msg(
            Role::Assistant,
            "Found the issue — missing null check on line 42",
        ),
        text_msg(Role::User, "Great, fix it please"),
        text_msg(Role::Assistant, "Done. Added the check and a test."),
    ]
}

fn default_engine() -> DistillEngine {
    DistillEngine::new(DistillConfig::default())
}

const MOCK_SUMMARY: &str = "\
## Summary
Fixed login bug in auth module.

## Task Context
Working on a null pointer crash in the login flow.

## Completed Work
- Fixed null check on line 42 of src/auth/login.rs
- Added regression test

## Key Decisions
- Decision: Add null check rather than restructure. Reason: Minimal change for the fix.

## Current State
Bug is fixed, test passes.

## Open Threads
- None

## Corrections
- Initially looked at wrong file before finding the issue in login.rs";

#[test]
fn should_distill_below_threshold_returns_false() {
    let engine = default_engine();
    assert!(
        !engine.should_distill(10, 50_000, 200_000, 0.8),
        "should not distill when token usage is well below the threshold"
    );
}

#[test]
fn should_distill_at_threshold_returns_true() {
    let engine = default_engine();
    // NOTE: 10 >= min_messages(6) + verbatim_tail(3) = 9, tokens at threshold
    assert!(
        engine.should_distill(10, 160_000, 200_000, 0.8),
        "should distill when token usage is exactly at the 0.8 threshold"
    );
}

#[test]
fn should_distill_above_threshold_returns_true() {
    let engine = default_engine();
    assert!(
        engine.should_distill(10, 190_000, 200_000, 0.8),
        "should distill when token usage is above the threshold"
    );
}

#[test]
fn should_distill_too_few_messages_returns_false() {
    let engine = default_engine();
    // NOTE: 5 < min_messages(6) + verbatim_tail(3) = 9
    assert!(
        !engine.should_distill(5, 190_000, 200_000, 0.8),
        "should not distill when message count is too low even if tokens are high"
    );
}

#[test]
fn should_distill_zero_context_window_returns_false() {
    let engine = default_engine();
    assert!(
        !engine.should_distill(10, 100, 0, 0.8),
        "should not distill when context window is zero"
    );
}

#[test]
fn should_distill_exact_min_plus_tail() {
    let engine = default_engine();
    // NOTE: exactly min_messages(6) + verbatim_tail(3) = 9
    assert!(
        engine.should_distill(9, 180_000, 200_000, 0.8),
        "should distill when message count equals exactly min_messages + verbatim_tail"
    );
}

#[test]
fn should_distill_below_min_plus_tail_returns_false() {
    let engine = default_engine();
    // NOTE: 8 < min_messages(6) + verbatim_tail(3) = 9
    assert!(
        !engine.should_distill(8, 190_000, 200_000, 0.8),
        "should not distill when message count is one below min_messages + verbatim_tail"
    );
}

#[test]
fn build_prompt_has_system_prompt() {
    let engine = default_engine();
    let messages = sample_conversation();
    let request = engine.build_prompt(&messages, "test-nous");

    assert!(
        request.system.is_some(),
        "build_prompt should produce a system prompt"
    );
    let system = request.system.expect("system prompt should be present");
    assert!(
        system.contains("## Summary"),
        "system prompt should contain ## Summary section"
    );
    assert!(
        system.contains("## Key Decisions"),
        "system prompt should contain ## Key Decisions section"
    );
    assert!(
        system.contains("## Corrections"),
        "system prompt should contain ## Corrections section"
    );
}

#[test]
fn build_prompt_includes_nous_id() {
    let engine = default_engine();
    let messages = sample_conversation();
    let request = engine.build_prompt(&messages, "my-agent");

    let user_text = request.messages[0].content.text();
    assert!(
        user_text.contains("my-agent"),
        "prompt user message should include the nous_id"
    );
}

#[test]
fn build_prompt_formats_messages_with_roles() {
    let engine = default_engine();
    let messages = sample_conversation();
    let request = engine.build_prompt(&messages, "test-nous");

    let user_text = request.messages[0].content.text();
    assert!(
        user_text.contains("[USER]"),
        "prompt should format user messages with [USER] role label"
    );
    assert!(
        user_text.contains("[ASSISTANT]"),
        "prompt should format assistant messages with [ASSISTANT] role label"
    );
}

#[test]
fn build_prompt_uses_config_model() {
    let config = DistillConfig {
        model: "claude-haiku-4-5-20251001".to_owned(),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert_eq!(
        request.model, "claude-haiku-4-5-20251001",
        "build_prompt should use the model from config"
    );
}

#[test]
fn build_prompt_uses_config_max_tokens() {
    let config = DistillConfig {
        max_output_tokens: 2048,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert_eq!(
        request.max_tokens, 2048,
        "build_prompt should use max_output_tokens from config"
    );
}

#[test]
fn build_prompt_no_tools() {
    let engine = default_engine();
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert!(
        request.tools.is_empty(),
        "build_prompt should not include any tools"
    );
}

#[tokio::test]
async fn distill_success_returns_result() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = success_provider(MOCK_SUMMARY);

    let result = engine.distill(&messages, "test-nous", &provider, 1).await;
    assert!(
        result.is_ok(),
        "distill should succeed with a valid provider and messages"
    );

    let result = result.expect("distill result should be Ok");
    assert!(
        result.summary.contains("Fixed login bug"),
        "distill result summary should contain the mock LLM output"
    );
    // NOTE: 6 messages - 3 verbatim_tail = 3 distilled
    assert_eq!(
        result.messages_distilled, 3,
        "should distill 6 messages minus 3 verbatim tail"
    );
    assert_eq!(
        result.verbatim_messages.len(),
        3,
        "should preserve 3 verbatim tail messages"
    );
    assert_eq!(
        result.distillation_number, 1,
        "distillation_number should match the value passed to distill"
    );
}

#[tokio::test]
async fn distill_token_estimates_populated() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = success_provider(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .expect("distill should succeed with a valid provider");

    assert!(
        result.tokens_before > 0,
        "tokens_before should be non-zero for a non-empty conversation"
    );
    assert_eq!(
        result.tokens_after, 200,
        "tokens_after should match the mock usage output_tokens"
    ); // from mock Usage
}

#[tokio::test]
async fn distill_distillation_number_passed_through() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = success_provider(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test-nous", &provider, 42)
        .await
        .expect("distill should succeed with a valid provider");
    assert_eq!(
        result.distillation_number, 42,
        "distillation_number should be passed through unchanged"
    );
}

#[tokio::test]
async fn distill_timestamp_is_valid() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = success_provider(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .expect("distill should succeed with a valid provider");

    // NOTE: jiff::Timestamp::to_string() produces RFC 3339 / ISO 8601
    assert!(
        result.timestamp.contains('T'),
        "timestamp should be ISO 8601: {}",
        result.timestamp
    );
}

#[tokio::test]
async fn distill_empty_messages_returns_no_messages_error() {
    let engine = default_engine();
    let provider = success_provider(MOCK_SUMMARY);

    let result = engine.distill(&[], "test-nous", &provider, 1).await;
    assert!(
        result.is_err(),
        "distill should return an error when called with no messages"
    );
    let err = result.expect_err("distill with empty messages should be an Err");
    assert!(
        err.to_string().contains("no messages"),
        "error message should indicate no messages were provided"
    );
}

#[tokio::test]
async fn distill_llm_failure_returns_llm_call_error() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = failure_provider();

    let result = engine.distill(&messages, "test-nous", &provider, 1).await;
    assert!(
        result.is_err(),
        "distill should return an error when the LLM provider fails"
    );
    let err = result.expect_err("distill with failing provider should be an Err");
    assert!(
        err.to_string().contains("LLM call failed"),
        "error message should indicate LLM call failure"
    );
}

#[tokio::test]
async fn distill_empty_response_returns_empty_summary_error() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = empty_response_provider();

    let result = engine.distill(&messages, "test-nous", &provider, 1).await;
    assert!(
        result.is_err(),
        "distill should return an error when the response has no content blocks"
    );
    let err = result.expect_err("distill with empty response should be an Err");
    assert!(
        err.to_string().contains("empty summary"),
        "error message should indicate an empty summary"
    );
}

#[tokio::test]
async fn distill_whitespace_only_response_returns_empty_summary_error() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = empty_text_provider();

    let result = engine.distill(&messages, "test-nous", &provider, 1).await;
    assert!(
        result.is_err(),
        "distill should return an error when all text blocks are whitespace-only"
    );
    let err = result.expect_err("distill with whitespace-only response should be an Err");
    assert!(
        err.to_string().contains("empty summary"),
        "error message should indicate an empty summary"
    );
}

#[test]
fn config_default_model() {
    let config = DistillConfig::default();
    assert_eq!(
        config.model, "claude-sonnet-4-20250514",
        "default model should be claude-sonnet-4-20250514"
    );
}

#[test]
fn config_default_values() {
    let config = DistillConfig::default();
    assert_eq!(
        config.max_output_tokens, 4096,
        "default max_output_tokens should be 4096"
    );
    assert_eq!(config.min_messages, 6, "default min_messages should be 6");
    assert!(
        config.include_tool_calls,
        "include_tool_calls should default to true"
    );
}

#[test]
fn estimate_tokens_chars_div_4() {
    let messages = vec![text_msg(Role::User, "abcdefgh")]; // 8 chars → 2 tokens
    assert_eq!(
        estimate_tokens(&messages),
        2,
        "8 chars should estimate to 2 tokens (chars / 4)"
    );
}

#[test]
fn estimate_tokens_rounds_up() {
    let messages = vec![text_msg(Role::User, "abcde")]; // 5 chars → ceil(5/4) = 2
    assert_eq!(
        estimate_tokens(&messages),
        2,
        "5 chars should round up to 2 tokens (ceil(5/4))"
    );
}

#[test]
fn estimate_tokens_empty_messages() {
    let messages: Vec<Message> = vec![];
    assert_eq!(
        estimate_tokens(&messages),
        0,
        "empty message list should estimate to 0 tokens"
    );
}

#[test]
fn extract_summary_from_text_blocks() {
    let blocks = vec![
        ContentBlock::Text {
            text: "Part 1".to_owned(),
            citations: None,
        },
        ContentBlock::Text {
            text: "Part 2".to_owned(),
            citations: None,
        },
    ];
    let text = extract_summary_text(&blocks);
    assert_eq!(
        text, "Part 1\nPart 2",
        "multiple text blocks should be joined with a newline"
    );
}
