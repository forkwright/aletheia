//! Tests for summary extraction and integration scenarios.
#![expect(clippy::expect_used, reason = "test assertions")]
#[cfg(test)]
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{CompletionResponse, ContentBlock, StopReason, Usage};

use super::super::*;

mod context_parse;

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

pub(super) fn success_provider(summary: &str) -> MockProvider {
    MockProvider::with_responses(vec![distill_response(summary)])
        .models(&["claude-sonnet-4-20250514"])
        .named("mock-distill")
}

#[expect(
    dead_code,
    reason = "test helper available for future extraction tests"
)]
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

#[expect(
    dead_code,
    reason = "test helper available for future extraction tests"
)]
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

pub(super) fn text_msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: Content::Text(text.to_owned()),
    }
}

pub(super) fn sample_conversation() -> Vec<Message> {
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

pub(super) fn default_engine() -> DistillEngine {
    DistillEngine::new(DistillConfig::default())
}

pub(super) const MOCK_SUMMARY: &str = "\
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
    assert_eq!(
        text, "Summary text",
        "non-text blocks like Thinking should be skipped when extracting summary"
    );
}

#[test]
fn extract_summary_trims_whitespace() {
    let blocks = vec![ContentBlock::Text {
        text: "  summary  ".to_owned(),
        citations: None,
    }];
    let text = extract_summary_text(&blocks);
    assert_eq!(
        text, "summary",
        "leading and trailing whitespace should be trimmed from extracted summary"
    );
}

#[test]
fn config_default_sections() {
    let config = DistillConfig::default();
    assert_eq!(
        config.sections.len(),
        7,
        "default config should have exactly 7 sections"
    );
    let s = &config.sections;
    assert_eq!(
        *s.first().expect("idx 0"),
        DistillSection::Summary,
        "first section should be Summary"
    ); // WHY: test assertion
    assert_eq!(
        *s.get(1).expect("idx 1"),
        DistillSection::TaskContext,
        "second section should be TaskContext"
    ); // WHY: test assertion
    assert_eq!(
        *s.get(2).expect("idx 2"),
        DistillSection::CompletedWork,
        "third section should be CompletedWork"
    ); // WHY: test assertion
    assert_eq!(
        *s.get(3).expect("idx 3"),
        DistillSection::KeyDecisions,
        "fourth section should be KeyDecisions"
    ); // WHY: test assertion
    assert_eq!(
        *s.get(4).expect("idx 4"),
        DistillSection::CurrentState,
        "fifth section should be CurrentState"
    ); // WHY: test assertion
    assert_eq!(
        *s.get(5).expect("idx 5"),
        DistillSection::OpenThreads,
        "sixth section should be OpenThreads"
    ); // WHY: test assertion
    assert_eq!(
        *s.get(6).expect("idx 6"),
        DistillSection::Corrections,
        "seventh section should be Corrections"
    ); // WHY: test assertion
}

#[test]
fn config_default_verbatim_tail() {
    let config = DistillConfig::default();
    assert_eq!(config.verbatim_tail, 3, "default verbatim_tail should be 3");
}

#[test]
fn config_default_distillation_model() {
    let config = DistillConfig::default();
    assert!(
        config.distillation_model.is_none(),
        "distillation_model should default to None"
    );
}

#[test]
fn build_prompt_uses_distillation_model_when_set() {
    let config = DistillConfig {
        distillation_model: Some("claude-haiku-4-5-20251001".to_owned()),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    assert_eq!(
        request.model, "claude-haiku-4-5-20251001",
        "build_prompt should use distillation_model when set"
    );
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
    assert_eq!(
        request.model, "claude-sonnet-4-20250514",
        "should fall back to primary model"
    );
}

#[test]
fn build_prompt_uses_dynamic_system_prompt() {
    let config = DistillConfig {
        sections: vec![DistillSection::Summary, DistillSection::KeyDecisions],
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&sample_conversation(), "test");
    let system = request
        .system
        .expect("build_prompt should produce a system prompt"); // WHY: test assertion
    assert!(
        system.contains("## Summary"),
        "system prompt should include the Summary section when configured"
    );
    let check = system.contains("## Key Decisions");
    assert!(
        check,
        "system prompt should include the Key Decisions section when configured"
    );
    let check = !system.contains("## Open Threads");
    assert!(
        check,
        "system prompt should not include Open Threads when not in configured sections"
    );
}

#[tokio::test]
async fn distill_preserves_verbatim_messages() {
    let config = DistillConfig {
        verbatim_tail: 2,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = sample_conversation(); // 6 messages
    let provider = success_provider(MOCK_SUMMARY);

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .expect("distill should succeed with a valid provider"); // WHY: test assertion

    assert_eq!(
        result.messages_distilled, 4,
        "should distill 6 messages minus verbatim_tail of 2"
    ); // 6 - 2);
    assert_eq!(
        result.verbatim_messages.len(),
        2,
        "should preserve 2 verbatim tail messages"
    );
}

#[test]
fn distill_section_equality() {
    assert_eq!(
        DistillSection::Summary,
        DistillSection::Summary,
        "identical variants should be equal"
    );
    assert_ne!(
        DistillSection::Summary,
        DistillSection::TaskContext,
        "different variants should not be equal"
    );
    let left = DistillSection::Custom {
        name: "Test".to_owned(),
        description: "desc".to_owned(),
    };
    let right = DistillSection::Custom {
        name: "Test".to_owned(),
        description: "desc".to_owned(),
    };
    assert_eq!(
        left, right,
        "Custom variants with identical fields should be equal"
    );
    let left = DistillSection::Custom {
        name: "A".to_owned(),
        description: "desc".to_owned(),
    };
    let right = DistillSection::Custom {
        name: "B".to_owned(),
        description: "desc".to_owned(),
    };
    assert_ne!(
        left, right,
        "Custom variants with different names should not be equal"
    );
}

#[test]
fn tick_turn_returns_false_when_no_failures() {
    let engine = default_engine();
    assert!(
        !engine.tick_turn(),
        "tick_turn should return false when no failures have been recorded"
    );
}

#[test]
fn in_backoff_is_false_on_fresh_engine() {
    let engine = default_engine();
    assert!(
        !engine.in_backoff(),
        "fresh engine should not be in backoff"
    );
}

#[test]
fn backoff_activates_after_failure_and_expires_after_one_turn() {
    let engine = default_engine();
    engine
        .retry_state
        .lock()
        .expect("retry_state mutex should not be poisoned") // WHY: test assertion
        .record_failure();
    assert!(
        engine.in_backoff(),
        "engine should be in backoff after recording a failure"
    );
    // NOTE: still in backoff, counter decrements to 0
    assert!(
        engine.tick_turn(),
        "tick_turn should return true while in backoff"
    );
    assert!(
        !engine.in_backoff(),
        "engine should exit backoff after turns_to_skip reaches zero"
    );
    assert!(
        !engine.tick_turn(),
        "tick_turn should return false once backoff has expired"
    ); // no longer in backoff);
}

#[test]
fn backoff_resets_on_success() {
    let engine = default_engine();
    {
        let mut state = engine
            .retry_state
            .lock()
            .expect("retry_state mutex should not be poisoned"); // WHY: test assertion
        state.record_failure();
        state.record_failure();
    }
    assert!(
        engine.in_backoff(),
        "engine should be in backoff after two failures"
    );
    engine
        .retry_state
        .lock()
        .expect("retry_state mutex should not be poisoned") // WHY: test assertion
        .record_success();
    assert!(
        !engine.in_backoff(),
        "backoff should clear after recording a success"
    );
    assert!(
        !engine.tick_turn(),
        "tick_turn should return false after backoff is cleared"
    );
}

#[test]
fn backoff_schedule_is_exponential() {
    // NOTE: after N failures, turns_to_skip = min(2^(N-1), 8)
    let cases: &[(u32, u32)] = &[(1, 1), (2, 2), (3, 4), (4, 8), (5, 8), (10, 8)];
    for &(failures, expected_skip) in cases {
        let engine = default_engine();
        {
            let mut state = engine
                .retry_state
                .lock()
                .expect("retry_state mutex should not be poisoned"); // WHY: test assertion
            for _ in 0..failures {
                state.record_failure();
            }
        }
        let actual = engine
            .retry_state
            .lock()
            .expect("retry_state mutex should not be poisoned") // WHY: test assertion
            .turns_to_skip;
        assert_eq!(
            actual, expected_skip,
            "after {failures} failures: got {actual}"
        );
    }
}

#[tokio::test]
async fn distill_records_failure_on_llm_error() {
    let engine = default_engine();
    let messages = sample_conversation();
    let provider = failure_provider();

    let _ = engine.distill(&messages, "test", &provider, 1).await;

    assert!(
        engine.in_backoff(),
        "engine should be in backoff after LLM failure"
    );
}

#[tokio::test]
async fn distill_records_success_and_clears_backoff() {
    let engine = default_engine();
    engine
        .retry_state
        .lock()
        .expect("retry_state mutex should not be poisoned") // WHY: test assertion
        .record_failure();
    assert!(
        engine.in_backoff(),
        "engine should be in backoff after recording a failure"
    );

    let messages = sample_conversation();
    let provider = success_provider(MOCK_SUMMARY);
    engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed with a valid provider"); // WHY: test assertion

    assert!(
        !engine.in_backoff(),
        "backoff should clear after successful distillation"
    );
}
