//! Tests for `DistillSection` and `DistillConfig` roundtrip serialization.
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions use .expect() for descriptive panic messages; test vec indices are valid"
)]
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{Content, Message, Role};

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

#[expect(
    dead_code,
    reason = "test helper available for future section config tests"
)]
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
