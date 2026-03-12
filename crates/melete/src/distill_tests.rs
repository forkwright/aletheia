use aletheia_hermeneus::types::{CompletionResponse, ContentBlock, StopReason, Usage};

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
            dyn std::future::Future<
                    Output = aletheia_hermeneus::error::Result<CompletionResponse>,
                > + Send
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
    // 10 >= min_messages(6) + verbatim_tail(3) = 9, tokens at threshold
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
    // 5 < min_messages(6) + verbatim_tail(3) = 9
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
    // Exactly min_messages(6) + verbatim_tail(3) = 9
    assert!(engine.should_distill(9, 180_000, 200_000, 0.8));
}

#[test]
fn should_distill_below_min_plus_tail_returns_false() {
    let engine = default_engine();
    // 8 < min_messages(6) + verbatim_tail(3) = 9
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
    // 6 messages - 3 verbatim_tail = 3 distilled
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

    // jiff::Timestamp::to_string() produces RFC 3339 / ISO 8601
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
