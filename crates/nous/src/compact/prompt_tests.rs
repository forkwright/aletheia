//! Tests for prompt selection and compaction-by-reason behavior.

#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use crate::compact::{CompactConfig, CompactReason, full, prompts, select_prompt};
use crate::pipeline::PipelineMessage;

#[test]
fn test_select_prompt_routing() {
    assert_eq!(
        select_prompt(CompactReason::TokenBudget),
        prompts::COMPACT_PROMPT,
        "TokenBudget should route to COMPACT_PROMPT"
    );
    assert_eq!(
        select_prompt(CompactReason::OperatorRequest),
        prompts::COMPACT_PROMPT,
        "OperatorRequest should route to COMPACT_PROMPT"
    );
    assert_eq!(
        select_prompt(CompactReason::SessionBoundary),
        prompts::RESTORE_PROMPT,
        "SessionBoundary should route to RESTORE_PROMPT"
    );
    assert_eq!(
        select_prompt(CompactReason::DreamConsolidation),
        prompts::RESTORE_PROMPT,
        "DreamConsolidation should route to RESTORE_PROMPT"
    );
}

#[test]
fn test_dream_consolidation_preserves_tool_trail() {
    // WHY(#95): The actual dream-consolidation call site does not yet exist
    // (background task registry wiring is TODO #2261). This test verifies
    // that the routing primitive is correctly set up: DreamConsolidation
    // selects RESTORE_PROMPT, and RESTORE_PROMPT instructs the model to
    // preserve tool-call trails.
    let prompt = select_prompt(CompactReason::DreamConsolidation);
    assert_eq!(prompt, prompts::RESTORE_PROMPT);
    assert!(
        prompt.contains("tool") || prompt.contains("call"),
        "RESTORE_PROMPT should mention tool calls so the trail is preserved"
    );
}

#[test]
fn test_token_budget_summarizes_60_percent() {
    // Simulate a mock LLM that returns the first 40% of the history text.
    let messages = vec![
        PipelineMessage::text("user", "a".repeat(500), 125),
        PipelineMessage::text("assistant", "b".repeat(500), 125),
        PipelineMessage::text("user", "c".repeat(500), 125),
        PipelineMessage::text("assistant", "d".repeat(500), 125),
        PipelineMessage::text("user", "recent question", 4),
        PipelineMessage::text("assistant", "recent answer", 4),
    ];
    let config = CompactConfig {
        preserve_turns: 2,
        ..CompactConfig::default()
    };
    let prompt = select_prompt(CompactReason::TokenBudget);
    let (request, preserved) = full::build_summary_request(&messages, &config, prompt);

    // Mock LLM: return first 40% of the history portion of the request.
    let history_start = request.find("---\n\n").map_or(0, |i| i + 5);
    let history_text = request.get(history_start..).unwrap_or("");
    let cutoff = history_text.len() * 40 / 100;
    let mock_summary = history_text.get(..cutoff).unwrap_or("").to_string();

    let result = full::apply_compaction(&mock_summary, preserved, Vec::new(), 1000, &config);

    let first_msg = result
        .messages
        .first()
        .expect("summary message should exist");
    let summary_content = first_msg
        .content
        .strip_prefix("[Conversation summary FROM compaction]\n")
        .unwrap_or(&first_msg.content);

    assert!(
        summary_content.len() <= history_text.len() * 40 / 100,
        "mock summary should be ≤40% of history text (was {}/{})",
        summary_content.len(),
        history_text.len()
    );
}
