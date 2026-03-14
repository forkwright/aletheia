#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
use aletheia_hermeneus::types::{
    CompletionResponse, ContentBlock, StopReason, ToolResultContent, Usage,
};

use super::*;

struct MockProvider {
    response: std::sync::Mutex<Option<aletheia_hermeneus::error::Result<CompletionResponse>>>,
}

impl MockProvider {
    fn success(summary: &str) -> Self {
        Self {
            response: std::sync::Mutex::new(Some(Ok(CompletionResponse {
                id: "msg_distill_1".to_owned(),
                model: "claude-sonnet-4-20250514".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: summary.to_owned(),
                    citations: None,
                }],
                usage: Usage {
                    input_tokens: 5000,
                    output_tokens: 200,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            }))),
        }
    }

    fn empty_response() -> Self {
        Self {
            response: std::sync::Mutex::new(Some(Ok(CompletionResponse {
                id: "msg_empty".to_owned(),
                model: "claude-sonnet-4-20250514".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![],
                usage: Usage::default(),
            }))),
        }
    }

    fn empty_text_blocks() -> Self {
        Self {
            response: std::sync::Mutex::new(Some(Ok(CompletionResponse {
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
            }))),
        }
    }

    fn failure() -> Self {
        Self {
            response: std::sync::Mutex::new(Some(Err(
                aletheia_hermeneus::error::ApiRequestSnafu {
                    message: "network timeout".to_owned(),
                }
                .build(),
            ))),
        }
    }
}

impl LlmProvider for MockProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = aletheia_hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            self.response
                .lock()
                .expect("lock poisoned") // INVARIANT: test mock, panic = test bug
                .take()
                .expect("mock provider called more than once")
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["claude-sonnet-4-20250514"]
    }

    #[expect(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "mock-distill"
    }
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
    assert!(!engine.should_distill(10, 50_000, 200_000, 0.8));
}

#[test]
fn should_distill_at_threshold_returns_true() {
    let engine = default_engine();
    // NOTE: 10 >= min_messages(6) + verbatim_tail(3) = 9, tokens at threshold
    assert!(engine.should_distill(10, 160_000, 200_000, 0.8));
}

#[test]
fn should_distill_above_threshold_returns_true() {
    let engine = default_engine();
    assert!(engine.should_distill(10, 190_000, 200_000, 0.8));
}

#[test]
fn should_distill_too_few_messages_returns_false() {
    let engine = default_engine();
    // NOTE: 5 < min_messages(6) + verbatim_tail(3) = 9
    assert!(!engine.should_distill(5, 190_000, 200_000, 0.8));
}

#[test]
fn should_distill_zero_context_window_returns_false() {
    let engine = default_engine();
    assert!(!engine.should_distill(10, 100, 0, 0.8));
}

#[test]
fn should_distill_exact_min_plus_tail() {
    let engine = default_engine();
    // NOTE: exactly min_messages(6) + verbatim_tail(3) = 9
    assert!(engine.should_distill(9, 180_000, 200_000, 0.8));
}

#[test]
fn should_distill_below_min_plus_tail_returns_false() {
    let engine = default_engine();
    // NOTE: 8 < min_messages(6) + verbatim_tail(3) = 9
    assert!(!engine.should_distill(8, 190_000, 200_000, 0.8));
}

#[test]
fn build_prompt_has_system_prompt() {
    let engine = default_engine();
    let messages = sample_conversation();
    let request = engine.build_prompt(&messages, "test-nous");

    assert!(request.system.is_some());
    let system = request.system.unwrap();
    assert!(system.contains("## Summary"));
    assert!(system.contains("## Key Decisions"));
    assert!(system.contains("## Corrections"));
}

#[test]
fn build_prompt_includes_nous_id() {
    let engine = default_engine();
    let messages = sample_conversation();
    let request = engine.build_prompt(&messages, "my-agent");

    let user_text = request.messages[0].content.text();
    assert!(user_text.contains("my-agent"));
}

#[test]
fn build_prompt_formats_messages_with_roles() {
    let engine = default_engine();
    let messages = sample_conversation();
    let request = engine.build_prompt(&messages, "test-nous");

    let user_text = request.messages[0].content.text();
    assert!(user_text.contains("[USER]"));
    assert!(user_text.contains("[ASSISTANT]"));
}

#[test]
fn build_prompt_uses_config_model() {
    let config = DistillConfig {
        model: "claude-haiku-4-5-20251001".to_owned(),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert_eq!(request.model, "claude-haiku-4-5-20251001");
}

#[test]
fn build_prompt_uses_config_max_tokens() {
    let config = DistillConfig {
        max_output_tokens: 2048,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert_eq!(request.max_tokens, 2048);
}

#[test]
fn build_prompt_no_tools() {
    let engine = default_engine();
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert!(request.tools.is_empty());
}

#[tokio::test]
async fn distill_success_returns_result() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = MockProvider::success(MOCK_SUMMARY);

    let result = engine.distill(&messages, "test-nous", &provider, 1).await;
    assert!(result.is_ok());

    let result = result.unwrap();
    assert!(result.summary.contains("Fixed login bug"));
    // NOTE: 6 messages - 3 verbatim_tail = 3 distilled
    assert_eq!(result.messages_distilled, 3);
    assert_eq!(result.verbatim_messages.len(), 3);
    assert_eq!(result.distillation_number, 1);
}

#[tokio::test]
async fn distill_token_estimates_populated() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = MockProvider::success(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .unwrap();

    assert!(result.tokens_before > 0);
    assert_eq!(result.tokens_after, 200); // from mock Usage
}

#[tokio::test]
async fn distill_distillation_number_passed_through() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = MockProvider::success(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test-nous", &provider, 42)
        .await
        .unwrap();
    assert_eq!(result.distillation_number, 42);
}

#[tokio::test]
async fn distill_timestamp_is_valid() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = MockProvider::success(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .unwrap();

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
    let provider = MockProvider::success(MOCK_SUMMARY);

    let result = engine.distill(&[], "test-nous", &provider, 1).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("no messages"));
}

#[tokio::test]
async fn distill_llm_failure_returns_llm_call_error() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = MockProvider::failure();

    let result = engine.distill(&messages, "test-nous", &provider, 1).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("LLM call failed"));
}

#[tokio::test]
async fn distill_empty_response_returns_empty_summary_error() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = MockProvider::empty_response();

    let result = engine.distill(&messages, "test-nous", &provider, 1).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("empty summary"));
}

#[tokio::test]
async fn distill_whitespace_only_response_returns_empty_summary_error() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = MockProvider::empty_text_blocks();

    let result = engine.distill(&messages, "test-nous", &provider, 1).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("empty summary"));
}

#[test]
fn config_default_model() {
    let config = DistillConfig::default();
    assert_eq!(config.model, "claude-sonnet-4-20250514");
}

#[test]
fn config_default_values() {
    let config = DistillConfig::default();
    assert_eq!(config.max_output_tokens, 4096);
    assert_eq!(config.min_messages, 6);
    assert!(config.include_tool_calls);
}

#[test]
fn estimate_tokens_chars_div_4() {
    let messages = vec![text_msg(Role::User, "abcdefgh")]; // 8 chars → 2 tokens
    assert_eq!(estimate_tokens(&messages), 2);
}

#[test]
fn estimate_tokens_rounds_up() {
    let messages = vec![text_msg(Role::User, "abcde")]; // 5 chars → ceil(5/4) = 2
    assert_eq!(estimate_tokens(&messages), 2);
}

#[test]
fn estimate_tokens_empty_messages() {
    let messages: Vec<Message> = vec![];
    assert_eq!(estimate_tokens(&messages), 0);
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
    assert_eq!(text, "Part 1\nPart 2");
}

#[test]
fn extract_summary_skips_non_text_blocks() {
    let blocks = vec![
        ContentBlock::Text {
            text: "Summary text".to_owned(),
            citations: None,
        },
        ContentBlock::Thinking {
            thinking: "internal thought".to_owned(),
            signature: None,
        },
    ];
    let text = extract_summary_text(&blocks);
    assert_eq!(text, "Summary text");
}

#[test]
fn extract_summary_trims_whitespace() {
    let blocks = vec![ContentBlock::Text {
        text: "  summary  ".to_owned(),
        citations: None,
    }];
    let text = extract_summary_text(&blocks);
    assert_eq!(text, "summary");
}

#[test]
fn config_default_sections() {
    let config = DistillConfig::default();
    assert_eq!(config.sections.len(), 7);
    assert_eq!(config.sections[0], DistillSection::Summary);
    assert_eq!(config.sections[1], DistillSection::TaskContext);
    assert_eq!(config.sections[2], DistillSection::CompletedWork);
    assert_eq!(config.sections[3], DistillSection::KeyDecisions);
    assert_eq!(config.sections[4], DistillSection::CurrentState);
    assert_eq!(config.sections[5], DistillSection::OpenThreads);
    assert_eq!(config.sections[6], DistillSection::Corrections);
}

#[test]
fn config_default_verbatim_tail() {
    let config = DistillConfig::default();
    assert_eq!(config.verbatim_tail, 3);
}

#[test]
fn config_default_distillation_model() {
    let config = DistillConfig::default();
    assert!(config.distillation_model.is_none());
}

#[test]
fn build_prompt_uses_distillation_model_when_set() {
    let config = DistillConfig {
        distillation_model: Some("claude-haiku-4-5-20251001".to_owned()),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert_eq!(request.model, "claude-haiku-4-5-20251001");
}

#[test]
fn build_prompt_falls_back_to_primary_model() {
    let config = DistillConfig {
        distillation_model: None,
        model: "claude-sonnet-4-20250514".to_owned(),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert_eq!(request.model, "claude-sonnet-4-20250514");
}

#[test]
fn build_prompt_uses_dynamic_system_prompt() {
    let config = DistillConfig {
        sections: vec![DistillSection::Summary, DistillSection::KeyDecisions],
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    let system = request.system.unwrap();
    assert!(system.contains("## Summary"));
    assert!(system.contains("## Key Decisions"));
    assert!(!system.contains("## Open Threads"));
}

#[tokio::test]
async fn distill_preserves_verbatim_messages() {
    let config = DistillConfig {
        verbatim_tail: 2,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = sample_conversation(); // 6 messages
    let provider = MockProvider::success(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.messages_distilled, 4); // 6 - 2
    assert_eq!(result.verbatim_messages.len(), 2);
}

#[test]
fn distill_section_equality() {
    assert_eq!(DistillSection::Summary, DistillSection::Summary);
    assert_ne!(DistillSection::Summary, DistillSection::TaskContext);
    assert_eq!(
        DistillSection::Custom {
            name: "Test".to_owned(),
            description: "desc".to_owned()
        },
        DistillSection::Custom {
            name: "Test".to_owned(),
            description: "desc".to_owned()
        }
    );
    assert_ne!(
        DistillSection::Custom {
            name: "A".to_owned(),
            description: "desc".to_owned()
        },
        DistillSection::Custom {
            name: "B".to_owned(),
            description: "desc".to_owned()
        }
    );
}

#[test]
fn tick_turn_returns_false_when_no_failures() {
    let engine = default_engine();
    assert!(!engine.tick_turn());
}

#[test]
fn in_backoff_is_false_on_fresh_engine() {
    let engine = default_engine();
    assert!(!engine.in_backoff());
}

#[test]
fn backoff_activates_after_failure_and_expires_after_one_turn() {
    let engine = default_engine();
    engine.retry_state.lock().unwrap().record_failure();
    assert!(engine.in_backoff());
    assert!(engine.tick_turn()); // still in backoff, counter decrements to 0
    assert!(!engine.in_backoff());
    assert!(!engine.tick_turn()); // no longer in backoff
}

#[test]
fn backoff_resets_on_success() {
    let engine = default_engine();
    {
        let mut state = engine.retry_state.lock().unwrap();
        state.record_failure();
        state.record_failure();
    }
    assert!(engine.in_backoff());
    engine.retry_state.lock().unwrap().record_success();
    assert!(!engine.in_backoff());
    assert!(!engine.tick_turn());
}

#[test]
fn backoff_schedule_is_exponential() {
    // NOTE: after N failures, turns_to_skip = min(2^(N-1), 8)
    let cases: &[(u32, u32)] = &[(1, 1), (2, 2), (3, 4), (4, 8), (5, 8), (10, 8)];
    for &(failures, expected_skip) in cases {
        let engine = default_engine();
        {
            let mut state = engine.retry_state.lock().unwrap();
            for _ in 0..failures {
                state.record_failure();
            }
        }
        let actual = engine.retry_state.lock().unwrap().turns_to_skip;
        assert_eq!(
            actual, expected_skip,
            "after {failures} failures expected turns_to_skip={expected_skip}, got {actual}"
        );
    }
}

#[tokio::test]
async fn distill_records_failure_on_llm_error() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = MockProvider::failure();

    let _ = engine.distill(&messages, "test", &provider, 1).await;

    assert!(
        engine.in_backoff(),
        "engine should be in backoff after LLM failure"
    );
}

#[tokio::test]
async fn distill_records_success_and_clears_backoff() {
    let engine = default_engine();
    engine.retry_state.lock().unwrap().record_failure();
    assert!(engine.in_backoff());

    let messages = sample_conversation();
    let provider = MockProvider::success(MOCK_SUMMARY);
    engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert!(
        !engine.in_backoff(),
        "backoff should clear after successful distillation"
    );
}

#[test]
fn enforce_context_limit_returns_zero_when_within_window() {
    let mut messages = sample_conversation();
    let count_before = messages.len();
    let dropped = enforce_context_limit(&mut messages, 1_000_000);
    assert_eq!(dropped, 0);
    assert_eq!(messages.len(), count_before);
}

#[test]
fn enforce_context_limit_drops_oldest_messages_when_over() {
    let mut messages: Vec<Message> = (0..10)
        .map(|i| text_msg(Role::User, &"x".repeat(100 + i)))
        .collect();
    let initial_count = messages.len();
    let dropped = enforce_context_limit(&mut messages, 4);
    assert!(dropped > 0, "should have dropped some messages");
    assert_eq!(messages.len(), initial_count - dropped);
}

#[test]
fn enforce_context_limit_keeps_at_least_one_message() {
    let mut messages = vec![text_msg(Role::User, "x".repeat(1000).as_str())];
    // NOTE: window of 1 token — impossible to satisfy, but we must keep the last message
    let dropped = enforce_context_limit(&mut messages, 1);
    assert_eq!(dropped, 0, "single message must not be dropped");
    assert_eq!(messages.len(), 1);
}

#[test]
fn enforce_context_limit_drops_from_front() {
    let mut messages = vec![
        text_msg(Role::User, &"a".repeat(400)), // oldest — should be dropped
        text_msg(Role::User, &"b".repeat(400)),
        text_msg(Role::User, &"c".repeat(4)), // newest — should be kept
    ];
    // NOTE: 201 total tokens, window of 2 keeps last ~8 chars = 2 tokens
    let dropped = enforce_context_limit(&mut messages, 2);
    assert!(dropped > 0);
    assert!(messages.last().unwrap().content.text().starts_with('c'));
}

#[test]
fn sanitize_nous_id_clean_string_unchanged() {
    assert_eq!(sanitize_nous_id("my-agent-01"), "my-agent-01");
}

#[test]
fn sanitize_nous_id_removes_backtick() {
    assert_eq!(sanitize_nous_id("agent`hack"), "agenthack");
}

#[test]
fn sanitize_nous_id_removes_newline() {
    assert_eq!(sanitize_nous_id("agent\nhack"), "agenthack");
}

#[test]
fn sanitize_nous_id_removes_carriage_return() {
    assert_eq!(sanitize_nous_id("agent\rhack"), "agenthack");
}

#[test]
fn sanitize_nous_id_removes_control_chars() {
    assert_eq!(sanitize_nous_id("agent\x00\x1bhack"), "agenthack");
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
    assert!(user_text.contains("idinjection"));
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
    let provider = MockProvider::success(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();
    // NOTE: assert the field exists — mock summary has no decisions to assert content on
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
    assert_eq!(flush.decisions.len(), 2);
    assert!(
        flush.decisions[0]
            .content
            .contains("Decision: Use null check")
    );
    assert!(
        flush.decisions[1]
            .content
            .contains("Decision: Keep auth module")
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
    assert_eq!(flush.corrections.len(), 2);
    assert!(flush.corrections[0].content.contains("Wrong file at first"));
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
    assert!(flush.task_state.is_some());
    let state = flush.task_state.unwrap();
    assert!(state.contains("login flow"));
}

#[test]
fn parse_summary_empty_sections_produce_no_items() {
    let summary = "## Summary\nJust a summary.\n\n## Key Decisions\n\n## Corrections\n";
    let flush = parse_summary_to_flush(summary, "2026-03-13T00:00:00Z");
    assert!(flush.decisions.is_empty());
    assert!(flush.corrections.is_empty());
    assert!(flush.task_state.is_none());
}

#[test]
fn parse_summary_flush_source_is_extracted() {
    let summary = "## Key Decisions\n- Decision: Use snafu. Reason: Standard.\n";
    let flush = parse_summary_to_flush(summary, "2026-03-13T00:00:00Z");
    assert_eq!(flush.decisions.len(), 1);
    assert!(matches!(flush.decisions[0].source, FlushSource::Extracted));
}
