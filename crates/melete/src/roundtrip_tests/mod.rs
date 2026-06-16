//! Roundtrip and end-to-end tests for melete distillation pipeline.
use hermeneus::test_utils::MockProvider;
use hermeneus::types::{Content, Message, Role};

use crate::distill::{DistillConfig, DistillEngine};
use crate::flush::{FlushItem, FlushSource};

mod build_prompt;
mod distill_threshold;
mod flush_prompt;
mod section_config;

pub(super) fn summary_provider(text: &str) -> MockProvider {
    MockProvider::new(text)
        .models(&["claude-sonnet-4-20250514"])
        .named("mock-roundtrip")
}

pub(super) fn text_msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: Content::Text(text.to_owned()),
        cache_breakpoint: false,
    }
}

pub(super) fn default_engine() -> DistillEngine {
    DistillEngine::new(DistillConfig::default())
}

pub(super) fn n_messages(n: usize) -> Vec<Message> {
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

pub(super) fn sample_flush_item(content: &str, source: FlushSource) -> FlushItem {
    FlushItem {
        content: content.to_owned(),
        timestamp: "2026-03-09T12:00:00Z".to_owned(),
        source,
    }
}

pub(super) const FULL_SUMMARY: &str = "\
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
fn shared_roundtrip_helpers_are_defined_once() {
    let root = include_str!("mod.rs");
    let submodules = [
        include_str!("build_prompt.rs"),
        include_str!("distill_threshold.rs"),
        include_str!("flush_prompt.rs"),
        include_str!("section_config.rs"),
    ];
    for helper in [
        "summary_provider",
        "text_msg",
        "default_engine",
        "n_messages",
        "sample_flush_item",
    ] {
        let root_pattern = format!("pub(super) fn {helper}(");
        assert_eq!(
            root.matches(root_pattern.as_str()).count(),
            1,
            "{helper} should be defined in roundtrip_tests/mod.rs"
        );
        let submodule_pattern = format!("fn {helper}(");
        for source in &submodules {
            assert!(
                !source.contains(submodule_pattern.as_str()),
                "{helper} should not be defined in roundtrip submodules"
            );
        }
    }
    let root_summary = format!("pub(super) const {}", "FULL_SUMMARY");
    assert_eq!(
        root.matches(root_summary.as_str()).count(),
        1,
        "FULL_SUMMARY should be defined in roundtrip_tests/mod.rs"
    );
    let submodule_summary = format!("const {}", "FULL_SUMMARY");
    for source in &submodules {
        assert!(
            !source.contains(submodule_summary.as_str()),
            "FULL_SUMMARY should not be defined in roundtrip submodules"
        );
        let dead_code_expect = format!("expect({}", "dead_code");
        assert!(
            !source.contains(dead_code_expect.as_str()),
            "roundtrip submodules should not suppress duplicated dead helpers"
        );
    }
}
