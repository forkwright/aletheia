//! Tests for the distill module.

mod extraction_integration;
mod panic;
mod threshold_prompt;

use hermeneus::test_utils::MockProvider;
use hermeneus::types::{
    CompletionResponse, Content, ContentBlock, Message, Role, StopReason, Usage,
};

use super::{DistillConfig, DistillEngine};

pub(super) fn distill_response(text: &str) -> CompletionResponse {
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
        cost_usd: None,
        duration_ms: None,
    }
}

pub(super) fn success_provider(summary: &str) -> MockProvider {
    MockProvider::with_responses(vec![distill_response(summary)])
        .models(&["claude-sonnet-4-20250514"])
        .named("mock-distill")
}

pub(super) fn failure_provider() -> MockProvider {
    MockProvider::error("network timeout")
        .models(&["claude-sonnet-4-20250514"])
        .named("mock-distill")
}

pub(super) fn empty_response_provider() -> MockProvider {
    MockProvider::with_responses(vec![CompletionResponse {
        id: "msg_empty".to_owned(),
        model: "claude-sonnet-4-20250514".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![],
        usage: Usage::default(),
        cost_usd: None,
        duration_ms: None,
    }])
    .models(&["claude-sonnet-4-20250514"])
    .named("mock-distill")
}

pub(super) fn empty_text_provider() -> MockProvider {
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
        cost_usd: None,
        duration_ms: None,
    }])
    .models(&["claude-sonnet-4-20250514"])
    .named("mock-distill")
}

pub(super) fn text_msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: Content::Text(text.to_owned()),
        cache_breakpoint: false,
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
