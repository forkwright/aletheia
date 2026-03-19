//! Tests for `MemoryFlush`, `FlushSource`, and prompt building.
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{Content, Message, Role};

use crate::distill::{DistillConfig, DistillEngine, DistillSection};
use crate::flush::{FlushItem, FlushSource, MemoryFlush};

#[expect(dead_code, reason = "test helper available for future flush tests")]
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

#[expect(dead_code, reason = "test helper available for future flush tests")]
fn default_engine() -> DistillEngine {
    DistillEngine::new(DistillConfig::default())
}

#[expect(dead_code, reason = "test helper available for future flush tests")]
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

#[expect(dead_code, reason = "test helper available for future flush tests")]
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
fn memory_flush_is_empty_when_only_empty_vecs() {
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
fn memory_flush_not_empty_when_has_facts() {
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
fn memory_flush_not_empty_when_has_corrections() {
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
fn memory_flush_markdown_multiple_items_per_section() {
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
fn flush_source_labels_via_markdown() {
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
fn engine_config_returns_reference() {
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
fn engine_config_sections_match_input() {
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
