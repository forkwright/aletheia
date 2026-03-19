//! Tests for `DistillEngine` prompt building and section headings.
//! Tests for `DistillEngine` behavior.
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions use .expect() for descriptive panic messages; test vec indices are valid"
)]
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{Content, ContentBlock, Message, Role, ToolResultContent};

use crate::distill::{DistillConfig, DistillEngine, DistillSection};
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

#[expect(dead_code, reason = "test helper available for future roundtrip tests")]
fn sample_flush_item(content: &str, source: FlushSource) -> FlushItem {
    FlushItem {
        content: content.to_owned(),
        timestamp: "2026-03-09T12:00:00Z".to_owned(),
        source,
    }
}

const FULL_SUMMARY: &str = "\
## Summary
Fixed login bug and added tool-based database migration.

## Task Context
Working on auth module bug fix for nous agent \"syn\".

## Completed Work
- Fixed null check on line 42 of src/auth/login.rs
- Ran database migration tool: migrate_db({\"version\": \"v2\"})
- Added regression test for login flow

## Key Decisions
- Decision: Add null check rather than restructure auth flow. Reason: Minimal invasive fix.
- Decision: Use v2 schema for migration. Reason: Backwards compatible.

## Current State
Bug is fixed, migration applied, all tests passing.

## Open Threads
- Performance audit of login endpoint deferred to next sprint

## Corrections
- CORRECTION: Initially looked at wrong file (session.rs), actually the bug was in login.rs";

#[test]
fn build_prompt_when_distillation_model_set_uses_it() {
    let config = DistillConfig {
        model: "claude-opus-4-20250514".to_owned(),
        distillation_model: Some("claude-haiku-4-5-20251001".to_owned()),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(
        request.model, "claude-haiku-4-5-20251001",
        "prompt should use distillation_model when set"
    );
}

#[test]
fn build_prompt_when_no_distillation_model_uses_primary() {
    let config = DistillConfig {
        model: "claude-opus-4-20250514".to_owned(),
        distillation_model: None,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(
        request.model, "claude-opus-4-20250514",
        "prompt should use primary model when distillation_model is None"
    );
}

#[test]
fn build_prompt_downshift_does_not_affect_max_tokens() {
    let config = DistillConfig {
        max_output_tokens: 8192,
        distillation_model: Some("claude-haiku-4-5-20251001".to_owned()),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(
        request.max_tokens, 8192,
        "max_tokens should be taken from config even when using a downshift model"
    );
}

#[test]
fn build_prompt_downshift_sonnet_to_haiku() {
    let config = DistillConfig {
        model: "claude-sonnet-4-20250514".to_owned(),
        distillation_model: Some("claude-haiku-4-5-20251001".to_owned()),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(
        request.model, "claude-haiku-4-5-20251001",
        "prompt should downshift from sonnet to haiku when distillation_model is set"
    );
}

#[test]
fn build_prompt_downshift_opus_to_sonnet() {
    let config = DistillConfig {
        model: "claude-opus-4-20250514".to_owned(),
        distillation_model: Some("claude-sonnet-4-20250514".to_owned()),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(
        request.model, "claude-sonnet-4-20250514",
        "prompt should downshift from opus to sonnet when distillation_model is set"
    );
}

#[tokio::test]
async fn full_pipeline_preserves_tool_results() {
    let messages = vec![
        text_msg(Role::User, "Run the database migration tool"),
        text_msg(Role::Assistant, "Running migrate_db({\"version\": \"v2\"})"),
        text_msg(Role::User, "What was the result?"),
        text_msg(Role::Assistant, "Migration completed. 3 tables updated."),
        text_msg(Role::User, "Verify"),
        text_msg(Role::Assistant, "Verification passed."),
        text_msg(Role::User, "Ship it"),
        text_msg(Role::Assistant, "Done."),
        text_msg(Role::User, "Thanks"),
        text_msg(Role::Assistant, "Welcome."),
    ];
    let provider = summary_provider(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill should succeed for tool-result pipeline");

    assert!(
        result.summary.contains("migrate_db"),
        "summary should contain the tool call name 'migrate_db'"
    );
    assert!(
        result.summary.contains("database migration"),
        "summary should mention database migration"
    );
}

#[tokio::test]
async fn full_pipeline_preserves_decisions() {
    let messages = vec![
        text_msg(Role::User, "Patch or restructure?"),
        text_msg(Role::Assistant, "Decision: Patch. Reason: Minimal fix."),
        text_msg(Role::User, "Schema version?"),
        text_msg(
            Role::Assistant,
            "Decision: v2. Reason: Backwards compatible.",
        ),
        text_msg(Role::User, "Apply"),
        text_msg(Role::Assistant, "Applied."),
        text_msg(Role::User, "Test"),
        text_msg(Role::Assistant, "Tests pass."),
        text_msg(Role::User, "Done?"),
        text_msg(Role::Assistant, "All done."),
    ];
    let provider = summary_provider(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill should succeed for decisions pipeline");

    assert!(
        result.summary.contains("Decision: Add null check"),
        "summary should contain the null check decision"
    );
    assert!(
        result.summary.contains("Decision: Use v2 schema"),
        "summary should contain the v2 schema decision"
    );
}

#[tokio::test]
async fn full_pipeline_preserves_corrections() {
    let messages = vec![
        text_msg(Role::User, "Check session.rs"),
        text_msg(
            Role::Assistant,
            "CORRECTION: wrong file. Bug is in login.rs.",
        ),
        text_msg(Role::User, "Fix it"),
        text_msg(Role::Assistant, "Fixed."),
        text_msg(Role::User, "Verify"),
        text_msg(Role::Assistant, "Verified."),
        text_msg(Role::User, "Test"),
        text_msg(Role::Assistant, "Passes."),
        text_msg(Role::User, "Ship"),
        text_msg(Role::Assistant, "Shipped."),
    ];
    let provider = summary_provider(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill should succeed for corrections pipeline");

    assert!(
        result.summary.contains("CORRECTION"),
        "summary should contain the CORRECTION marker"
    );
    assert!(
        result.summary.contains("login.rs"),
        "summary should contain the corrected file path login.rs"
    );
}

#[tokio::test]
async fn full_pipeline_reduces_token_count() {
    let messages = n_messages(20);
    let provider = summary_provider(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill should succeed for token reduction pipeline");

    assert!(
        result.tokens_after < result.tokens_before,
        "tokens_after ({}) should be less than tokens_before ({})",
        result.tokens_after,
        result.tokens_before
    );
}

#[tokio::test]
async fn full_pipeline_summary_contains_all_sections() {
    let messages = n_messages(10);
    let provider = summary_provider(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill should succeed for all-sections pipeline");

    for section in DistillSection::all_standard() {
        let heading = section.heading();
        assert!(
            result.summary.contains(&heading),
            "summary missing section: {heading}"
        );
    }
}

#[tokio::test]
async fn full_pipeline_verbatim_tail_integrity() {
    let messages = vec![
        text_msg(Role::User, "Alpha"),
        text_msg(Role::Assistant, "Bravo"),
        text_msg(Role::User, "Charlie"),
        text_msg(Role::Assistant, "Delta"),
        text_msg(Role::User, "Echo"),
        text_msg(Role::Assistant, "Foxtrot"),
        text_msg(Role::User, "Golf — preserved"),
        text_msg(Role::Assistant, "Hotel — preserved"),
        text_msg(Role::User, "India — preserved"),
    ];
    let provider = summary_provider(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill should succeed for verbatim tail integrity check");

    assert_eq!(
        result.verbatim_messages.len(),
        3,
        "last 3 messages should be kept verbatim"
    );
    assert_eq!(
        result.verbatim_messages[0].content.text(),
        "Golf — preserved",
        "first verbatim message should be 'Golf — preserved'"
    );
    assert_eq!(
        result.verbatim_messages[1].content.text(),
        "Hotel — preserved",
        "second verbatim message should be 'Hotel — preserved'"
    );
    assert_eq!(
        result.verbatim_messages[2].content.text(),
        "India — preserved",
        "third verbatim message should be 'India — preserved'"
    );
    assert_eq!(
        result.verbatim_messages[0].role,
        Role::User,
        "first verbatim message should have User role"
    );
    assert_eq!(
        result.verbatim_messages[1].role,
        Role::Assistant,
        "second verbatim message should have Assistant role"
    );
    assert_eq!(
        result.verbatim_messages[2].role,
        Role::User,
        "third verbatim message should have User role"
    );
}

#[tokio::test]
async fn distill_when_empty_messages_returns_error() {
    let provider = summary_provider(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine.distill(&[], "syn", &provider, 1).await;
    assert!(
        result.is_err(),
        "distill should return an error when given no messages"
    );
    assert!(
        result
            .expect_err("distill with empty input should have returned an error")
            .to_string()
            .contains("no messages"),
        "error message should mention 'no messages'"
    );
}

#[tokio::test]
async fn distill_when_single_message_all_verbatim() {
    let config = DistillConfig {
        min_messages: 1,
        verbatim_tail: 3,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = vec![text_msg(Role::User, "Solo message")];
    let provider = summary_provider("## Summary\nSolo.");

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed with a single message");

    assert_eq!(
        result.verbatim_messages.len(),
        1,
        "single message should be kept verbatim"
    );
    assert_eq!(
        result.messages_distilled, 0,
        "no messages should be distilled when input is a single message"
    );
}

#[tokio::test]
async fn distill_when_oversized_input_handles_gracefully() {
    let mut messages = Vec::new();
    for i in 0..100 {
        let long_content = format!("Message {i}: {}", "x".repeat(500));
        messages.push(text_msg(
            if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            &long_content,
        ));
    }

    let provider = summary_provider(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed with oversized input");

    assert_eq!(
        result.messages_distilled, 97,
        "97 messages should be distilled (100 - 3 verbatim)"
    ); // 100 - 3 verbatim
    assert_eq!(
        result.verbatim_messages.len(),
        3,
        "last 3 messages should be kept verbatim"
    );
    assert!(
        result.tokens_before > 10_000,
        "token count for 100 large messages should exceed 10,000"
    );
}

#[tokio::test]
async fn distill_when_all_tool_call_messages() {
    let messages = vec![
        Message {
            role: Role::Assistant,
            content: Content::Blocks(vec![
                ContentBlock::Text {
                    text: "Let me check.".to_owned(),
                    citations: None,
                },
                ContentBlock::ToolUse {
                    id: "t1".to_owned(),
                    name: "read_file".to_owned(),
                    input: serde_json::json!({"path": "/tmp/test.rs"}),
                },
            ]),
        },
        Message {
            role: Role::User,
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_owned(),
                content: ToolResultContent::text("fn main() {}"),
                is_error: Some(false),
            }]),
        },
        Message {
            role: Role::Assistant,
            content: Content::Blocks(vec![ContentBlock::Text {
                text: "Found the file.".to_owned(),
                citations: None,
            }]),
        },
        text_msg(Role::User, "Fix it"),
        text_msg(Role::Assistant, "Done."),
    ];

    let config = DistillConfig {
        verbatim_tail: 2,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let provider = summary_provider(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed with mixed tool-call messages");

    assert_eq!(
        result.messages_distilled, 3,
        "first 3 messages (including tool call/result) should be distilled"
    );
    assert_eq!(
        result.verbatim_messages.len(),
        2,
        "last 2 messages should be kept verbatim"
    );
}

#[tokio::test]
async fn distill_when_two_messages_with_tail_three() {
    let messages = vec![
        text_msg(Role::User, "Hello"),
        text_msg(Role::Assistant, "Hi"),
    ];
    let config = DistillConfig {
        verbatim_tail: 3,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let provider = summary_provider("## Summary\nGreeting.");

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed when message count is less than verbatim_tail");

    assert_eq!(
        result.verbatim_messages.len(),
        2,
        "both messages should be kept verbatim when count is less than verbatim_tail"
    );
    assert_eq!(
        result.messages_distilled, 0,
        "no messages should be distilled when all fit within verbatim_tail"
    );
}

#[test]
fn section_heading_for_each_standard_variant() {
    let expected = [
        (DistillSection::Summary, "## Summary"),
        (DistillSection::TaskContext, "## Task Context"),
        (DistillSection::CompletedWork, "## Completed Work"),
        (DistillSection::KeyDecisions, "## Key Decisions"),
        (DistillSection::CurrentState, "## Current State"),
        (DistillSection::OpenThreads, "## Open Threads"),
        (DistillSection::Corrections, "## Corrections"),
    ];
    for (section, heading) in expected {
        assert_eq!(
            section.heading(),
            heading,
            "heading for {section:?} should be {heading:?}"
        );
    }
}

#[test]
fn section_heading_for_custom_uses_name() {
    let section = DistillSection::Custom {
        name: "My Section".to_owned(),
        description: "ignored here".to_owned(),
    };
    assert_eq!(
        section.heading(),
        "## My Section",
        "custom section heading should use the provided name"
    );
}

#[test]
fn section_description_non_empty_for_all_standard() {
    for section in DistillSection::all_standard() {
        assert!(
            !section.description().is_empty(),
            "empty description for {section:?}",
        );
    }
}

#[test]
fn section_custom_description_returns_provided_text() {
    let section = DistillSection::Custom {
        name: "X".to_owned(),
        description: "My custom description".to_owned(),
    };
    assert_eq!(
        section.description(),
        "My custom description",
        "custom section should return the provided description text"
    );
}

#[test]
fn all_standard_returns_seven_sections() {
    assert_eq!(
        DistillSection::all_standard().len(),
        7,
        "all_standard() should return exactly 7 sections"
    );
}

#[test]
fn build_prompt_includes_message_count() {
    let engine = default_engine();
    let messages = n_messages(8);
    let request = engine.build_prompt(&messages, "test");
    let text = request.messages[0].content.text();
    assert!(
        text.contains("8 messages"),
        "prompt should include the message count '8 messages'"
    );
}

#[test]
fn build_prompt_temperature_is_zero() {
    let engine = default_engine();
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(
        request.temperature,
        Some(0.0),
        "distillation prompt should use temperature 0.0 for deterministic output"
    );
}

#[test]
fn build_prompt_with_system_message() {
    let engine = default_engine();
    let messages = vec![
        Message {
            role: Role::System,
            content: Content::Text("You are helpful.".to_owned()),
        },
        text_msg(Role::User, "Hello"),
        text_msg(Role::Assistant, "Hi"),
    ];
    let request = engine.build_prompt(&messages, "test");
    let text = request.messages[0].content.text();
    assert!(
        text.contains("[SYSTEM]"),
        "prompt should include [SYSTEM] marker when a system message is present"
    );
}
