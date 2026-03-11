//! Roundtrip and comprehensive tests for melete distillation pipeline.

use std::sync::Mutex;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{
    CompletionRequest, CompletionResponse, Content, ContentBlock, Message, Role, StopReason,
    ToolResultContent, Usage,
};

use crate::distill::{DistillConfig, DistillEngine, DistillSection};
use crate::flush::{FlushItem, FlushSource, MemoryFlush};

// ═══════════════════════════════════════════════════════════════════════
// Mock provider
// ═══════════════════════════════════════════════════════════════════════

struct MockProvider {
    response: Mutex<Option<aletheia_hermeneus::error::Result<CompletionResponse>>>,
}

impl MockProvider {
    fn with_summary(summary: &str) -> Self {
        Self {
            response: Mutex::new(Some(Ok(CompletionResponse {
                id: "msg_roundtrip".to_owned(),
                model: "claude-sonnet-4-20250514".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: summary.to_owned(),
                    citations: None,
                }],
                usage: Usage {
                    input_tokens: 5000,
                    output_tokens: 50,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            }))),
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
                .expect("lock") // INVARIANT: test mock, panic = test bug
                .take()
                .expect("mock provider called more than once")
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["claude-sonnet-4-20250514"]
    }

    #[expect(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "mock-roundtrip"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Serde roundtrip tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_distill_section_summary_roundtrip() {
    let section = DistillSection::Summary;
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_section_task_context_roundtrip() {
    let section = DistillSection::TaskContext;
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_section_completed_work_roundtrip() {
    let section = DistillSection::CompletedWork;
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_section_key_decisions_roundtrip() {
    let section = DistillSection::KeyDecisions;
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_section_current_state_roundtrip() {
    let section = DistillSection::CurrentState;
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_section_open_threads_roundtrip() {
    let section = DistillSection::OpenThreads;
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_section_corrections_roundtrip() {
    let section = DistillSection::Corrections;
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_section_custom_roundtrip() {
    let section = DistillSection::Custom {
        name: "Architecture Notes".to_owned(),
        description: "Record architectural decisions.".to_owned(),
    };
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_section_custom_with_special_chars_roundtrip() {
    let section = DistillSection::Custom {
        name: "Notes: \"important\" & <critical>".to_owned(),
        description: "Contains special chars: \\ / \n newline".to_owned(),
    };
    let json = serde_json::to_string(&section).unwrap();
    let back: DistillSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, back);
}

#[test]
fn test_distill_config_default_roundtrip() {
    let config = DistillConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let back: DistillConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, config.model);
    assert_eq!(back.max_output_tokens, config.max_output_tokens);
    assert_eq!(back.min_messages, config.min_messages);
    assert_eq!(back.include_tool_calls, config.include_tool_calls);
    assert_eq!(back.distillation_model, config.distillation_model);
    assert_eq!(back.verbatim_tail, config.verbatim_tail);
    assert_eq!(back.sections, config.sections);
}

#[test]
fn test_distill_config_with_downshift_roundtrip() {
    let config = DistillConfig {
        distillation_model: Some("claude-haiku-4-5-20251001".to_owned()),
        ..DistillConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: DistillConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.distillation_model,
        Some("claude-haiku-4-5-20251001".to_owned())
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
    let json = serde_json::to_string(&config).unwrap();
    let back: DistillConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.sections.len(), 2);
    assert_eq!(back.sections[0], DistillSection::Summary);
}

#[test]
fn test_flush_source_extracted_roundtrip() {
    let source = FlushSource::Extracted;
    let json = serde_json::to_string(&source).unwrap();
    let back: FlushSource = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, FlushSource::Extracted));
}

#[test]
fn test_flush_source_agent_note_roundtrip() {
    let source = FlushSource::AgentNote;
    let json = serde_json::to_string(&source).unwrap();
    let back: FlushSource = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, FlushSource::AgentNote));
}

#[test]
fn test_flush_source_tool_pattern_roundtrip() {
    let source = FlushSource::ToolPattern;
    let json = serde_json::to_string(&source).unwrap();
    let back: FlushSource = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, FlushSource::ToolPattern));
}

#[test]
fn test_flush_item_roundtrip() {
    let item = sample_flush_item("Use snafu for errors", FlushSource::Extracted);
    let json = serde_json::to_string(&item).unwrap();
    let back: FlushItem = serde_json::from_str(&json).unwrap();
    assert_eq!(back.content, item.content);
    assert_eq!(back.timestamp, item.timestamp);
}

#[test]
fn test_memory_flush_empty_roundtrip() {
    let flush = MemoryFlush::empty();
    let json = serde_json::to_string(&flush).unwrap();
    let back: MemoryFlush = serde_json::from_str(&json).unwrap();
    assert!(back.is_empty());
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
    let json = serde_json::to_string(&flush).unwrap();
    let back: MemoryFlush = serde_json::from_str(&json).unwrap();
    assert_eq!(back.decisions.len(), 1);
    assert_eq!(back.corrections.len(), 1);
    assert_eq!(back.facts.len(), 1);
    assert_eq!(back.task_state, Some("Implementing pipeline".to_owned()));
    assert!(!back.is_empty());
}

#[test]
fn test_all_standard_sections_roundtrip() {
    let sections = DistillSection::all_standard();
    let json = serde_json::to_string(&sections).unwrap();
    let back: Vec<DistillSection> = serde_json::from_str(&json).unwrap();
    assert_eq!(sections, back);
}

// ═══════════════════════════════════════════════════════════════════════
// Section splitting logic
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_split_when_verbatim_tail_zero_summarizes_all() {
    let config = DistillConfig {
        verbatim_tail: 0,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let messages = n_messages(6);
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.messages_distilled, 6);
    assert!(result.verbatim_messages.is_empty());
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
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.messages_distilled, 0);
    assert_eq!(result.verbatim_messages.len(), 4);
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
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.messages_distilled, 0);
    assert_eq!(result.verbatim_messages.len(), 3);
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
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.messages_distilled, 2);
    assert_eq!(result.verbatim_messages.len(), 2);
    assert_eq!(result.verbatim_messages[0].content.text(), "Third");
    assert_eq!(result.verbatim_messages[1].content.text(), "Fourth");
}

// ═══════════════════════════════════════════════════════════════════════
// Token budget calculation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_should_distill_when_exactly_at_threshold_returns_true() {
    let engine = default_engine();
    // ratio = 80000/100000 = 0.8, threshold = 0.8 → true
    assert!(engine.should_distill(10, 80_000, 100_000, 0.8));
}

#[test]
fn test_should_distill_when_just_below_threshold_returns_false() {
    let engine = default_engine();
    // ratio = 79999/100000 = 0.79999, threshold = 0.8 → false
    assert!(!engine.should_distill(10, 79_999, 100_000, 0.8));
}

#[test]
fn test_should_distill_when_threshold_zero_always_true_if_enough_messages() {
    let engine = default_engine();
    assert!(engine.should_distill(10, 1, 100_000, 0.0));
}

#[test]
fn test_should_distill_when_threshold_one_needs_full_context() {
    let engine = default_engine();
    assert!(engine.should_distill(10, 100_000, 100_000, 1.0));
    assert!(!engine.should_distill(10, 99_999, 100_000, 1.0));
}

#[test]
fn test_should_distill_when_large_token_count_returns_true() {
    let engine = default_engine();
    assert!(engine.should_distill(100, 900_000, 1_000_000, 0.8));
}

#[test]
fn test_should_distill_with_custom_min_messages() {
    let config = DistillConfig {
        min_messages: 20,
        verbatim_tail: 5,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    // Need at least 25 messages (20 + 5)
    assert!(!engine.should_distill(24, 180_000, 200_000, 0.8));
    assert!(engine.should_distill(25, 180_000, 200_000, 0.8));
}

#[test]
fn test_should_distill_with_zero_verbatim_tail() {
    let config = DistillConfig {
        min_messages: 6,
        verbatim_tail: 0,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    // Need only 6 messages (6 + 0)
    assert!(!engine.should_distill(5, 180_000, 200_000, 0.8));
    assert!(engine.should_distill(6, 180_000, 200_000, 0.8));
}

// ═══════════════════════════════════════════════════════════════════════
// Verbatim tail preservation
// ═══════════════════════════════════════════════════════════════════════

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
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.verbatim_messages.len(), 3);
    assert_eq!(result.verbatim_messages[0].role, Role::User);
    assert_eq!(result.verbatim_messages[1].role, Role::Assistant);
    assert_eq!(result.verbatim_messages[2].role, Role::User);
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
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.verbatim_messages.len(), 1);
    assert_eq!(result.verbatim_messages[0].content.text(), "Only message");
    assert_eq!(result.messages_distilled, 0);
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
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.verbatim_messages.len(), 1);
    assert!(
        result.verbatim_messages[0]
            .content
            .text()
            .contains("Block content preserved")
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Model downshift selection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_build_prompt_when_distillation_model_set_uses_it() {
    let config = DistillConfig {
        model: "claude-opus-4-20250514".to_owned(),
        distillation_model: Some("claude-haiku-4-5-20251001".to_owned()),
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(request.model, "claude-haiku-4-5-20251001");
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
    assert_eq!(request.model, "claude-opus-4-20250514");
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
    assert_eq!(request.max_tokens, 8192);
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
    assert_eq!(request.model, "claude-haiku-4-5-20251001");
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
    assert_eq!(request.model, "claude-sonnet-4-20250514");
}

// ═══════════════════════════════════════════════════════════════════════
// Integration: full distillation pipeline
// ═══════════════════════════════════════════════════════════════════════

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
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .unwrap();

    assert!(result.summary.contains("migrate_db"));
    assert!(result.summary.contains("database migration"));
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
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .unwrap();

    assert!(result.summary.contains("Decision: Add null check"));
    assert!(result.summary.contains("Decision: Use v2 schema"));
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
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .unwrap();

    assert!(result.summary.contains("CORRECTION"));
    assert!(result.summary.contains("login.rs"));
}

#[tokio::test]
async fn test_full_pipeline_reduces_token_count() {
    let messages = n_messages(20);
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .unwrap();

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
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .unwrap();

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
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.verbatim_messages.len(), 3);
    assert_eq!(
        result.verbatim_messages[0].content.text(),
        "Golf — preserved"
    );
    assert_eq!(
        result.verbatim_messages[1].content.text(),
        "Hotel — preserved"
    );
    assert_eq!(
        result.verbatim_messages[2].content.text(),
        "India — preserved"
    );
    assert_eq!(result.verbatim_messages[0].role, Role::User);
    assert_eq!(result.verbatim_messages[1].role, Role::Assistant);
    assert_eq!(result.verbatim_messages[2].role, Role::User);
}

// ═══════════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_distill_when_empty_messages_returns_error() {
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine.distill(&[], "syn", &provider, 1).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no messages"));
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
    let provider = MockProvider::with_summary("## Summary\nSolo.");

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.verbatim_messages.len(), 1);
    assert_eq!(result.messages_distilled, 0);
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

    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.messages_distilled, 97); // 100 - 3 verbatim
    assert_eq!(result.verbatim_messages.len(), 3);
    assert!(result.tokens_before > 10_000);
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
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.messages_distilled, 3);
    assert_eq!(result.verbatim_messages.len(), 2);
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
    let provider = MockProvider::with_summary("## Summary\nGreeting.");

    let result = engine
        .distill(&messages, "test", &provider, 1)
        .await
        .unwrap();

    assert_eq!(result.verbatim_messages.len(), 2);
    assert_eq!(result.messages_distilled, 0);
}

// ═══════════════════════════════════════════════════════════════════════
// DistillSection heading/description coverage
// ═══════════════════════════════════════════════════════════════════════

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
        assert_eq!(section.heading(), heading);
    }
}

#[test]
fn test_section_heading_for_custom_uses_name() {
    let section = DistillSection::Custom {
        name: "My Section".to_owned(),
        description: "ignored here".to_owned(),
    };
    assert_eq!(section.heading(), "## My Section");
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
    assert_eq!(section.description(), "My custom description");
}

#[test]
fn test_all_standard_returns_seven_sections() {
    assert_eq!(DistillSection::all_standard().len(), 7);
}

// ═══════════════════════════════════════════════════════════════════════
// Prompt formatting
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_build_prompt_includes_message_count() {
    let engine = default_engine();
    let messages = n_messages(8);
    let request = engine.build_prompt(&messages, "test");
    let text = request.messages[0].content.text();
    assert!(text.contains("8 messages"));
}

#[test]
fn test_build_prompt_temperature_is_zero() {
    let engine = default_engine();
    let request = engine.build_prompt(&n_messages(4), "test");
    assert_eq!(request.temperature, Some(0.0));
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
    assert!(text.contains("[SYSTEM]"));
}

// ═══════════════════════════════════════════════════════════════════════
// MemoryFlush additional tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_memory_flush_is_empty_when_only_empty_vecs() {
    let flush = MemoryFlush {
        decisions: vec![],
        corrections: vec![],
        facts: vec![],
        task_state: None,
    };
    assert!(flush.is_empty());
}

#[test]
fn test_memory_flush_not_empty_when_has_facts() {
    let flush = MemoryFlush {
        decisions: vec![],
        corrections: vec![],
        facts: vec![sample_flush_item("A fact", FlushSource::ToolPattern)],
        task_state: None,
    };
    assert!(!flush.is_empty());
}

#[test]
fn test_memory_flush_not_empty_when_has_corrections() {
    let flush = MemoryFlush {
        decisions: vec![],
        corrections: vec![sample_flush_item("A correction", FlushSource::AgentNote)],
        facts: vec![],
        task_state: None,
    };
    assert!(!flush.is_empty());
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
    assert!(md.contains("Decision A"));
    assert!(md.contains("Decision B"));
    assert!(md.contains("(source: extracted)"));
    assert!(md.contains("(source: agent_note)"));
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
    assert!(md.contains("(source: extracted)"));
    assert!(md.contains("(source: agent_note)"));
    assert!(md.contains("(source: tool_pattern)"));
}

// ═══════════════════════════════════════════════════════════════════════
// Config accessor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_engine_config_returns_reference() {
    let config = DistillConfig {
        model: "custom-model".to_owned(),
        max_output_tokens: 2048,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    assert_eq!(engine.config().model, "custom-model");
    assert_eq!(engine.config().max_output_tokens, 2048);
}

#[test]
fn test_engine_config_sections_match_input() {
    let config = DistillConfig {
        sections: vec![DistillSection::Summary, DistillSection::Corrections],
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    assert_eq!(engine.config().sections.len(), 2);
    assert_eq!(engine.config().sections[0], DistillSection::Summary);
    assert_eq!(engine.config().sections[1], DistillSection::Corrections);
}
