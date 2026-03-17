//! Roundtrip and comprehensive tests for melete distillation pipeline.
#![expect(
    clippy::expect_used,
    reason = "test assertions use .expect() for descriptive panic messages"
)]

use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{Content, ContentBlock, Message, Role, ToolResultContent};

use crate::distill::{DistillConfig, DistillEngine, DistillSection};
use crate::flush::{FlushItem, FlushSource, MemoryFlush};

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
fn test_distill_section_summary_roundtrip() {
    let section = DistillSection::Summary;
    let json =
        serde_json::to_string(&section).expect("DistillSection::Summary should serialize to JSON");
    let back: DistillSection =
        serde_json::from_str(&json).expect("serialized DistillSection::Summary should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::Summary should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_section_task_context_roundtrip() {
    let section = DistillSection::TaskContext;
    let json = serde_json::to_string(&section)
        .expect("DistillSection::TaskContext should serialize to JSON");
    let back: DistillSection = serde_json::from_str(&json)
        .expect("serialized DistillSection::TaskContext should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::TaskContext should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_section_completed_work_roundtrip() {
    let section = DistillSection::CompletedWork;
    let json = serde_json::to_string(&section)
        .expect("DistillSection::CompletedWork should serialize to JSON");
    let back: DistillSection = serde_json::from_str(&json)
        .expect("serialized DistillSection::CompletedWork should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::CompletedWork should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_section_key_decisions_roundtrip() {
    let section = DistillSection::KeyDecisions;
    let json = serde_json::to_string(&section)
        .expect("DistillSection::KeyDecisions should serialize to JSON");
    let back: DistillSection = serde_json::from_str(&json)
        .expect("serialized DistillSection::KeyDecisions should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::KeyDecisions should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_section_current_state_roundtrip() {
    let section = DistillSection::CurrentState;
    let json = serde_json::to_string(&section)
        .expect("DistillSection::CurrentState should serialize to JSON");
    let back: DistillSection = serde_json::from_str(&json)
        .expect("serialized DistillSection::CurrentState should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::CurrentState should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_section_open_threads_roundtrip() {
    let section = DistillSection::OpenThreads;
    let json = serde_json::to_string(&section)
        .expect("DistillSection::OpenThreads should serialize to JSON");
    let back: DistillSection = serde_json::from_str(&json)
        .expect("serialized DistillSection::OpenThreads should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::OpenThreads should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_section_corrections_roundtrip() {
    let section = DistillSection::Corrections;
    let json = serde_json::to_string(&section)
        .expect("DistillSection::Corrections should serialize to JSON");
    let back: DistillSection = serde_json::from_str(&json)
        .expect("serialized DistillSection::Corrections should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::Corrections should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_section_custom_roundtrip() {
    let section = DistillSection::Custom {
        name: "Architecture Notes".to_owned(),
        description: "Record architectural decisions.".to_owned(),
    };
    let json =
        serde_json::to_string(&section).expect("DistillSection::Custom should serialize to JSON");
    let back: DistillSection =
        serde_json::from_str(&json).expect("serialized DistillSection::Custom should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::Custom should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_section_custom_with_special_chars_roundtrip() {
    let section = DistillSection::Custom {
        name: "Notes: \"important\" & <critical>".to_owned(),
        description: "Contains special chars: \\ / \n newline".to_owned(),
    };
    let json = serde_json::to_string(&section)
        .expect("DistillSection::Custom with special chars should serialize to JSON");
    let back: DistillSection = serde_json::from_str(&json)
        .expect("serialized DistillSection::Custom with special chars should deserialize");
    assert_eq!(
        section, back,
        "DistillSection::Custom with special chars should survive a JSON roundtrip"
    );
}

#[test]
fn test_distill_config_default_roundtrip() {
    let config = DistillConfig::default();
    let json =
        serde_json::to_string(&config).expect("DistillConfig::default() should serialize to JSON");
    let back: DistillConfig =
        serde_json::from_str(&json).expect("serialized DistillConfig should deserialize");
    assert_eq!(back.model, config.model, "model should survive roundtrip");
    assert_eq!(
        back.max_output_tokens, config.max_output_tokens,
        "max_output_tokens should survive roundtrip"
    );
    assert_eq!(
        back.min_messages, config.min_messages,
        "min_messages should survive roundtrip"
    );
    assert_eq!(
        back.include_tool_calls, config.include_tool_calls,
        "include_tool_calls should survive roundtrip"
    );
    assert_eq!(
        back.distillation_model, config.distillation_model,
        "distillation_model should survive roundtrip"
    );
    assert_eq!(
        back.verbatim_tail, config.verbatim_tail,
        "verbatim_tail should survive roundtrip"
    );
    assert_eq!(
        back.sections, config.sections,
        "sections should survive roundtrip"
    );
}

#[test]
fn test_distill_config_with_downshift_roundtrip() {
    let config = DistillConfig {
        distillation_model: Some("claude-haiku-4-5-20251001".to_owned()),
        ..DistillConfig::default()
    };
    let json = serde_json::to_string(&config)
        .expect("DistillConfig with distillation_model should serialize to JSON");
    let back: DistillConfig = serde_json::from_str(&json)
        .expect("serialized DistillConfig with distillation_model should deserialize");
    assert_eq!(
        back.distillation_model,
        Some("claude-haiku-4-5-20251001".to_owned()),
        "distillation_model should survive roundtrip"
    );
}

#[test]
fn test_distill_config_custom_sections_roundtrip() {
    let config = DistillConfig {
        sections: vec![
            DistillSection::Summary,
            DistillSection::Custom {
                name: "Perf".to_owned(),
                description: "Performance notes.".to_owned(),
            },
        ],
        ..DistillConfig::default()
    };
    let json = serde_json::to_string(&config)
        .expect("DistillConfig with custom sections should serialize to JSON");
    let back: DistillConfig = serde_json::from_str(&json)
        .expect("serialized DistillConfig with custom sections should deserialize");
    assert_eq!(
        back.sections.len(),
        2,
        "deserialized config should have 2 sections"
    );
    assert_eq!(
        back.sections[0],
        DistillSection::Summary,
        "first section should be Summary after roundtrip"
    );
}

#[test]
fn test_flush_source_extracted_roundtrip() {
    let source = FlushSource::Extracted;
    let json =
        serde_json::to_string(&source).expect("FlushSource::Extracted should serialize to JSON");
    let back: FlushSource =
        serde_json::from_str(&json).expect("serialized FlushSource::Extracted should deserialize");
    assert!(
        matches!(back, FlushSource::Extracted),
        "deserialized value should be FlushSource::Extracted"
    );
}

#[test]
fn test_flush_source_agent_note_roundtrip() {
    let source = FlushSource::AgentNote;
    let json =
        serde_json::to_string(&source).expect("FlushSource::AgentNote should serialize to JSON");
    let back: FlushSource =
        serde_json::from_str(&json).expect("serialized FlushSource::AgentNote should deserialize");
    assert!(
        matches!(back, FlushSource::AgentNote),
        "deserialized value should be FlushSource::AgentNote"
    );
}

#[test]
fn test_flush_source_tool_pattern_roundtrip() {
    let source = FlushSource::ToolPattern;
    let json =
        serde_json::to_string(&source).expect("FlushSource::ToolPattern should serialize to JSON");
    let back: FlushSource = serde_json::from_str(&json)
        .expect("serialized FlushSource::ToolPattern should deserialize");
    assert!(
        matches!(back, FlushSource::ToolPattern),
        "deserialized value should be FlushSource::ToolPattern"
    );
}

#[test]
fn test_flush_item_roundtrip() {
    let item = sample_flush_item("Use snafu for errors", FlushSource::Extracted);
    let json = serde_json::to_string(&item).expect("FlushItem should serialize to JSON");
    let back: FlushItem =
        serde_json::from_str(&json).expect("serialized FlushItem should deserialize");
    assert_eq!(
        back.content, item.content,
        "FlushItem content should survive roundtrip"
    );
    assert_eq!(
        back.timestamp, item.timestamp,
        "FlushItem timestamp should survive roundtrip"
    );
}

#[test]
fn test_memory_flush_empty_roundtrip() {
    let flush = MemoryFlush::empty();
    let json = serde_json::to_string(&flush).expect("empty MemoryFlush should serialize to JSON");
    let back: MemoryFlush =
        serde_json::from_str(&json).expect("serialized empty MemoryFlush should deserialize");
    assert!(
        back.is_empty(),
        "deserialized MemoryFlush should still be empty"
    );
}

#[test]
fn test_memory_flush_full_roundtrip() {
    let flush = MemoryFlush {
        decisions: vec![sample_flush_item("Use actor model", FlushSource::Extracted)],
        corrections: vec![sample_flush_item("Wrong path", FlushSource::AgentNote)],
        facts: vec![sample_flush_item(
            "Config in taxis",
            FlushSource::ToolPattern,
        )],
        task_state: Some("Implementing pipeline".to_owned()),
    };
    let json =
        serde_json::to_string(&flush).expect("populated MemoryFlush should serialize to JSON");
    let back: MemoryFlush =
        serde_json::from_str(&json).expect("serialized populated MemoryFlush should deserialize");
    assert_eq!(
        back.decisions.len(),
        1,
        "decisions should have 1 item after roundtrip"
    );
    assert_eq!(
        back.corrections.len(),
        1,
        "corrections should have 1 item after roundtrip"
    );
    assert_eq!(
        back.facts.len(),
        1,
        "facts should have 1 item after roundtrip"
    );
    assert_eq!(
        back.task_state,
        Some("Implementing pipeline".to_owned()),
        "task_state should survive roundtrip"
    );
    assert!(
        !back.is_empty(),
        "populated MemoryFlush should not be empty after roundtrip"
    );
}

#[test]
fn test_all_standard_sections_roundtrip() {
    let sections = DistillSection::all_standard();
    let json = serde_json::to_string(&sections)
        .expect("all standard DistillSections should serialize to JSON");
    let back: Vec<DistillSection> = serde_json::from_str(&json)
        .expect("serialized standard DistillSections should deserialize");
    assert_eq!(
        sections, back,
        "all standard sections should survive a JSON roundtrip"
    );
}

#[tokio::test]
async fn test_split_when_verbatim_tail_zero_summarizes_all() {
    let config = DistillConfig {
        verbatim_tail: 0,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = n_messages(6);
    let provider = summary_provider(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed with verbatim_tail=0");

    assert_eq!(
        result.messages_distilled, 6,
        "all 6 messages should be distilled when verbatim_tail=0"
    );
    assert!(
        result.verbatim_messages.is_empty(),
        "no verbatim messages should remain when verbatim_tail=0"
    );
}

#[tokio::test]
async fn test_split_when_verbatim_tail_equals_messages_distills_none() {
    let config = DistillConfig {
        verbatim_tail: 4,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = n_messages(4);
    let provider = summary_provider(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed when verbatim_tail equals message count");

    assert_eq!(
        result.messages_distilled, 0,
        "no messages should be distilled when verbatim_tail equals message count"
    );
    assert_eq!(
        result.verbatim_messages.len(),
        4,
        "all 4 messages should be kept verbatim"
    );
}

#[tokio::test]
async fn test_split_when_verbatim_tail_exceeds_messages_clamps() {
    let config = DistillConfig {
        verbatim_tail: 100,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = n_messages(3);
    let provider = summary_provider(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed when verbatim_tail exceeds message count");

    assert_eq!(
        result.messages_distilled, 0,
        "no messages should be distilled when verbatim_tail exceeds message count"
    );
    assert_eq!(
        result.verbatim_messages.len(),
        3,
        "all 3 messages should be kept verbatim when verbatim_tail is clamped"
    );
}

#[tokio::test]
async fn test_split_preserves_exact_tail_content() {
    let config = DistillConfig {
        verbatim_tail: 2,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = vec![
        text_msg(Role::User, "First"),
        text_msg(Role::Assistant, "Second"),
        text_msg(Role::User, "Third"),
        text_msg(Role::Assistant, "Fourth"),
    ];
    let provider = summary_provider(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .expect("distill should succeed when preserving exact tail content");

    assert_eq!(
        result.messages_distilled, 2,
        "first 2 messages should be distilled"
    );
    assert_eq!(
        result.verbatim_messages.len(),
        2,
        "last 2 messages should be kept verbatim"
    );
    assert_eq!(
        result.verbatim_messages[0].content.text(),
        "Third",
        "first verbatim message should be 'Third'"
    );
    assert_eq!(
        result.verbatim_messages[1].content.text(),
        "Fourth",
        "second verbatim message should be 'Fourth'"
    );
}

#[test]
fn test_should_distill_when_exactly_at_threshold_returns_true() {
    let engine = default_engine();
    // NOTE: ratio = 80000/100000 = 0.8, threshold = 0.8 → true
    assert!(
        engine.should_distill(10, 80_000, 100_000, 0.8),
        "should_distill should return true when ratio equals threshold"
    );
}

#[test]
fn test_should_distill_when_just_below_threshold_returns_false() {
    let engine = default_engine();
    // NOTE: ratio = 79999/100000 = 0.79999, threshold = 0.8 → false
    assert!(
        !engine.should_distill(10, 79_999, 100_000, 0.8),
        "should_distill should return false when ratio is just below threshold"
    );
}

#[test]
fn test_should_distill_when_threshold_zero_always_true_if_enough_messages() {
    let engine = default_engine();
    assert!(
        engine.should_distill(10, 1, 100_000, 0.0),
        "should_distill should return true for any token count when threshold is zero"
    );
}

#[test]
fn test_should_distill_when_threshold_one_needs_full_context() {
    let engine = default_engine();
    assert!(
        engine.should_distill(10, 100_000, 100_000, 1.0),
        "should_distill should return true when tokens fill entire context and threshold is 1.0"
    );
    assert!(
        !engine.should_distill(10, 99_999, 100_000, 1.0),
        "should_distill should return false when tokens are one short of full context and threshold is 1.0"
    );
}

#[test]
fn test_should_distill_when_large_token_count_returns_true() {
    let engine = default_engine();
    assert!(
        engine.should_distill(100, 900_000, 1_000_000, 0.8),
        "should_distill should return true with large token counts at threshold"
    );
}

#[test]
fn test_should_distill_with_custom_min_messages() {
    let config = DistillConfig {
        min_messages: 20,
        verbatim_tail: 5,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    // NOTE: need at least min_messages(20) + verbatim_tail(5) = 25 messages
    assert!(
        !engine.should_distill(24, 180_000, 200_000, 0.8),
        "should_distill should return false with 24 messages when minimum required is 25"
    );
    assert!(
        engine.should_distill(25, 180_000, 200_000, 0.8),
        "should_distill should return true with exactly 25 messages when minimum required is 25"
    );
}

#[test]
fn test_should_distill_with_zero_verbatim_tail() {
    let config = DistillConfig {
        min_messages: 6,
        verbatim_tail: 0,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    // NOTE: need only min_messages(6) + verbatim_tail(0) = 6 messages
    assert!(
        !engine.should_distill(5, 180_000, 200_000, 0.8),
        "should_distill should return false with 5 messages when minimum required is 6"
    );
    assert!(
        engine.should_distill(6, 180_000, 200_000, 0.8),
        "should_distill should return true with exactly 6 messages when minimum required is 6"
    );
}

#[tokio::test]
async fn test_verbatim_tail_preserves_roles() {
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
        .expect("distill should succeed when preserving verbatim tail roles");

    assert_eq!(
        result.verbatim_messages.len(),
        3,
        "last 3 messages should be kept verbatim"
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
async fn test_verbatim_tail_when_single_message_preserves_it() {
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
        .expect("distill should succeed with single message input");

    assert_eq!(
        result.verbatim_messages.len(),
        1,
        "single message should be kept verbatim"
    );
    assert_eq!(
        result.verbatim_messages[0].content.text(),
        "Only message",
        "verbatim message content should match input"
    );
    assert_eq!(
        result.messages_distilled, 0,
        "no messages should be distilled when only one message exists"
    );
}

#[tokio::test]
async fn test_verbatim_tail_preserves_block_content() {
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
        .expect("distill should succeed when last message has block content");

    assert_eq!(
        result.verbatim_messages.len(),
        1,
        "last message with block content should be kept verbatim"
    );
    assert!(
        result.verbatim_messages[0]
            .content
            .text()
            .contains("Block content preserved"),
        "verbatim block message should contain the original text block content"
    );
}

#[test]
fn test_build_prompt_when_distillation_model_set_uses_it() {
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
fn test_build_prompt_when_no_distillation_model_uses_primary() {
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
fn test_build_prompt_downshift_does_not_affect_max_tokens() {
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
fn test_build_prompt_downshift_sonnet_to_haiku() {
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
fn test_build_prompt_downshift_opus_to_sonnet() {
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
async fn test_full_pipeline_preserves_tool_results() {
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
async fn test_full_pipeline_preserves_decisions() {
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
async fn test_full_pipeline_preserves_corrections() {
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
async fn test_full_pipeline_reduces_token_count() {
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
async fn test_full_pipeline_summary_contains_all_sections() {
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
async fn test_full_pipeline_verbatim_tail_integrity() {
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
async fn test_distill_when_empty_messages_returns_error() {
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
async fn test_distill_when_single_message_all_verbatim() {
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
async fn test_distill_when_oversized_input_handles_gracefully() {
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
async fn test_distill_when_all_tool_call_messages() {
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
async fn test_distill_when_two_messages_with_tail_three() {
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
fn test_section_heading_for_each_standard_variant() {
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
fn test_section_heading_for_custom_uses_name() {
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
fn test_section_description_non_empty_for_all_standard() {
    for section in DistillSection::all_standard() {
        assert!(
            !section.description().is_empty(),
            "empty description for {section:?}",
        );
    }
}

#[test]
fn test_section_custom_description_returns_provided_text() {
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
fn test_all_standard_returns_seven_sections() {
    assert_eq!(
        DistillSection::all_standard().len(),
        7,
        "all_standard() should return exactly 7 sections"
    );
}

#[test]
fn test_build_prompt_includes_message_count() {
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
fn test_build_prompt_temperature_is_zero() {
    let engine = default_engine();
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(
        request.temperature,
        Some(0.0),
        "distillation prompt should use temperature 0.0 for deterministic output"
    );
}

#[test]
fn test_build_prompt_with_system_message() {
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

#[test]
fn test_memory_flush_is_empty_when_only_empty_vecs() {
    let flush = MemoryFlush {
        decisions: vec![],
        corrections: vec![],
        facts: vec![],
        task_state: None,
    };
    assert!(
        flush.is_empty(),
        "MemoryFlush with all empty fields should be considered empty"
    );
}

#[test]
fn test_memory_flush_not_empty_when_has_facts() {
    let flush = MemoryFlush {
        decisions: vec![],
        corrections: vec![],
        facts: vec![sample_flush_item("A fact", FlushSource::ToolPattern)],
        task_state: None,
    };
    assert!(
        !flush.is_empty(),
        "MemoryFlush with facts should not be considered empty"
    );
}

#[test]
fn test_memory_flush_not_empty_when_has_corrections() {
    let flush = MemoryFlush {
        decisions: vec![],
        corrections: vec![sample_flush_item("A correction", FlushSource::AgentNote)],
        facts: vec![],
        task_state: None,
    };
    assert!(
        !flush.is_empty(),
        "MemoryFlush with corrections should not be considered empty"
    );
}

#[test]
fn test_memory_flush_markdown_multiple_items_per_section() {
    let flush = MemoryFlush {
        decisions: vec![
            sample_flush_item("Decision A", FlushSource::Extracted),
            sample_flush_item("Decision B", FlushSource::AgentNote),
        ],
        corrections: vec![],
        facts: vec![],
        task_state: None,
    };
    let md = flush.to_markdown();
    assert!(
        md.contains("Decision A"),
        "markdown should include 'Decision A'"
    );
    assert!(
        md.contains("Decision B"),
        "markdown should include 'Decision B'"
    );
    assert!(
        md.contains("(source: extracted)"),
        "markdown should label extracted items with their source"
    );
    assert!(
        md.contains("(source: agent_note)"),
        "markdown should label agent_note items with their source"
    );
}

#[test]
fn test_flush_source_labels_via_markdown() {
    let flush = MemoryFlush {
        decisions: vec![sample_flush_item("d", FlushSource::Extracted)],
        corrections: vec![sample_flush_item("c", FlushSource::AgentNote)],
        facts: vec![sample_flush_item("f", FlushSource::ToolPattern)],
        task_state: None,
    };
    let md = flush.to_markdown();
    assert!(
        md.contains("(source: extracted)"),
        "markdown should label Extracted items with '(source: extracted)'"
    );
    assert!(
        md.contains("(source: agent_note)"),
        "markdown should label AgentNote items with '(source: agent_note)'"
    );
    assert!(
        md.contains("(source: tool_pattern)"),
        "markdown should label ToolPattern items with '(source: tool_pattern)'"
    );
}

#[test]
fn test_engine_config_returns_reference() {
    let config = DistillConfig {
        model: "custom-model".to_owned(),
        max_output_tokens: 2048,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    assert_eq!(
        engine.config().model,
        "custom-model",
        "engine config should reflect the provided model name"
    );
    assert_eq!(
        engine.config().max_output_tokens,
        2048,
        "engine config should reflect the provided max_output_tokens"
    );
}

#[test]
fn test_engine_config_sections_match_input() {
    let config = DistillConfig {
        sections: vec![DistillSection::Summary, DistillSection::Corrections],
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    assert_eq!(
        engine.config().sections.len(),
        2,
        "engine config should have 2 sections as provided"
    );
    assert_eq!(
        engine.config().sections[0],
        DistillSection::Summary,
        "first section should be Summary"
    );
    assert_eq!(
        engine.config().sections[1],
        DistillSection::Corrections,
        "second section should be Corrections"
    );
}
