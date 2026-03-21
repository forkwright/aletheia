//! Tests for `DistillEngine` threshold and trigger logic.
//! Tests for `DistillEngine` behavior.
#![expect(clippy::expect_used, reason = "test assertions")]
#[cfg(test)]
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{Content, ContentBlock, Message, Role};

use crate::distill::{DistillConfig, DistillEngine};
use crate::flush::{FlushItem, FlushSource};

fn summary_provider(text: &str) -> MockProvider {
    MockProvider::new(text)
        .models(&["claude-sonnet-4-20250514"])
        .named("mock-roundtrip")
}

fn text_msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: Content::Text(text.to_owned()),
    }
}

fn default_engine() -> DistillEngine {
    DistillEngine::new(DistillConfig::default())
}

#[expect(dead_code, reason = "test helper available for future threshold tests")]
fn n_messages(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| {
            text_msg(
                if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                &format!("Message {i} with content for token estimation."),
            )
        })
        .collect()
}

#[expect(dead_code, reason = "test helper available for future threshold tests")]
fn sample_flush_item(content: &str, source: FlushSource) -> FlushItem {
    FlushItem {
        content: content.to_owned(),
        timestamp: "2026-03-09T12:00:00Z".to_owned(),
        source,
    }
}

const FULL_SUMMARY: &str = "\
## Summary
Fixed login bug and added tool-based database schema update.

## Task Context
Working on auth module bug fix for nous agent \"syn\".

## Completed Work
- Fixed null check on line 42 of src/auth/login.rs
- Ran database schema update tool: migrate_db({\"version\": \"v2\"})
- Added regression test for login flow

## Key Decisions
- Decision: Add null check rather than restructure auth flow. Reason: Minimal invasive fix.
- Decision: Use v2 schema for schema update. Reason: Backwards compatible.

## Current State
Bug is fixed, schema applied, all tests passing.

## Open Threads
- Performance audit of login endpoint deferred to next sprint

## Corrections
- CORRECTION: Initially looked at wrong file (session.rs), actually the bug was in login.rs";

#[test]
fn should_distill_when_exactly_at_threshold_returns_true() {
    let engine = default_engine();
    // NOTE: ratio = 80000/100000 = 0.8, threshold = 0.8 → true
    let check = engine.should_distill(10, 80_000, 100_000, 0.8);
    assert!(
        check,
        "should_distill should return true when ratio equals threshold"
    );
}

#[test]
fn should_distill_when_just_below_threshold_returns_false() {
    let engine = default_engine();
    // NOTE: ratio = 79999/100000 = 0.79999, threshold = 0.8 → false
    let check = !engine.should_distill(10, 79_999, 100_000, 0.8);
    assert!(
        check,
        "should_distill should return false when ratio is just below threshold"
    );
}

#[test]
fn should_distill_when_threshold_zero_always_true_if_enough_messages() {
    let engine = default_engine();
    let check = engine.should_distill(10, 1, 100_000, 0.0);
    assert!(
        check,
        "should_distill should return true for any token count when threshold is zero"
    );
}

#[test]
fn should_distill_when_threshold_one_needs_full_context() {
    let engine = default_engine();
    let check = engine.should_distill(10, 100_000, 100_000, 1.0);
    assert!(
        check,
        "should_distill should return true when tokens fill entire context and threshold is 1.0"
    );
    let check = !engine.should_distill(10, 99_999, 100_000, 1.0);
    assert!(
        check,
        "should_distill should return false when tokens are one short of full context and threshold is 1.0"
    );
}

#[test]
fn should_distill_when_large_token_count_returns_true() {
    let engine = default_engine();
    let check = engine.should_distill(100, 900_000, 1_000_000, 0.8);
    assert!(
        check,
        "should_distill should return true with large token counts at threshold"
    );
}

#[test]
fn should_distill_with_custom_min_messages() {
    let config = DistillConfig {
        min_messages: 20,
        verbatim_tail: 5,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    // NOTE: need at least min_messages(20) + verbatim_tail(5) = 25 messages
    let check = !engine.should_distill(24, 180_000, 200_000, 0.8);
    assert!(
        check,
        "should_distill should return false with 24 messages when minimum required is 25"
    );
    let check = engine.should_distill(25, 180_000, 200_000, 0.8);
    assert!(
        check,
        "should_distill should return true with exactly 25 messages when minimum required is 25"
    );
}

#[test]
fn should_distill_with_zero_verbatim_tail() {
    let config = DistillConfig {
        min_messages: 6,
        verbatim_tail: 0,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    // NOTE: need only min_messages(6) + verbatim_tail(0) = 6 messages
    let check = !engine.should_distill(5, 180_000, 200_000, 0.8);
    assert!(
        check,
        "should_distill should return false with 5 messages when minimum required is 6"
    );
    let check = engine.should_distill(6, 180_000, 200_000, 0.8);
    assert!(
        check,
        "should_distill should return true with exactly 6 messages when minimum required is 6"
    );
}

#[tokio::test]
async fn verbatim_tail_preserves_roles() {
    let config = DistillConfig {
        verbatim_tail: 3,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = vec![
        text_msg(Role::User, "Old 1"),
        text_msg(Role::Assistant, "Old 2"),
        text_msg(Role::User, "Old 3"),
        text_msg(Role::Assistant, "Old 4"),
        text_msg(Role::User, "Recent user"),
        text_msg(Role::Assistant, "Recent assistant"),
        text_msg(Role::User, "Last user"),
    ];
    let provider = summary_provider(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed when preserving verbatim tail roles"); // WHY: test assertion

    let vm = &result.verbatim_messages;
    assert_eq!(vm.len(), 3, "last 3 messages should be kept verbatim");
    assert_eq!(
        vm.first().expect("msg 0").role,
        Role::User,
        "first verbatim message should have User role"
    ); // WHY: test assertion
    assert_eq!(
        vm.get(1).expect("msg 1").role,
        Role::Assistant,
        "second verbatim message should have Assistant role"
    ); // WHY: test assertion
    assert_eq!(
        vm.get(2).expect("msg 2").role,
        Role::User,
        "third verbatim message should have User role"
    ); // WHY: test assertion
}

#[tokio::test]
async fn verbatim_tail_when_single_message_preserves_it() {
    let config = DistillConfig {
        verbatim_tail: 5,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = vec![text_msg(Role::User, "Only message")];
    let provider = summary_provider(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed with single message input"); // WHY: test assertion

    assert_eq!(
        result.verbatim_messages.len(),
        1,
        "single message should be kept verbatim"
    );
    let left = result
        .verbatim_messages
        .first()
        .expect("verbatim msg 0")
        .content
        .text(); // WHY: test assertion
    assert_eq!(
        left, "Only message",
        "verbatim message content should match input"
    );
    assert_eq!(
        result.messages_distilled, 0,
        "no messages should be distilled when only one message exists"
    );
}

#[tokio::test]
async fn verbatim_tail_preserves_block_content() {
    let config = DistillConfig {
        verbatim_tail: 1,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);

    let block_msg = Message {
        role: Role::Assistant,
        content: Content::Blocks(vec![
            ContentBlock::Text {
                text: "Block content preserved".to_owned(),
                citations: None,
            },
            ContentBlock::Thinking {
                thinking: "internal thought".to_owned(),
                signature: None,
            },
        ]),
    };
    let messages = vec![
        text_msg(Role::User, "First"),
        text_msg(Role::Assistant, "Second"),
        block_msg.clone(),
    ];
    let provider = summary_provider(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed when last message has block content"); // WHY: test assertion

    assert_eq!(
        result.verbatim_messages.len(),
        1,
        "last message with block content should be kept verbatim"
    );
    let first = result.verbatim_messages.first().expect("verbatim msg 0"); // WHY: test assertion
    let check = first.content.text().contains("Block content preserved");
    assert!(
        check,
        "verbatim block message should contain the original text block content"
    );
}
