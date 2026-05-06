#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use koina::event::EventEmitter;

use super::*;
use crate::compact::CompactConfig;
use crate::config::{NousConfig, PipelineConfig, StageBudget};
use crate::pipeline::{PipelineMessage, ReflectionStatus};

fn make_msg(role: &str, content: &str) -> PipelineMessage {
    PipelineMessage {
        role: role.to_owned(),
        content: content.to_owned(),
        token_estimate: 0,
        cache_breakpoint: false,
    }
}

fn config_with_preserve(preserve: usize) -> CompactConfig {
    CompactConfig {
        preserve_turns: preserve,
        ..CompactConfig::default()
    }
}

#[test]
fn structural_summary_header_present() {
    let msgs = vec![make_msg("user", "hello")];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.starts_with("Previous conversation context:"));
}

#[test]
fn structural_summary_preserves_recent_turns() {
    // preserve_turns=2: only the first 3 messages get summarized
    let msgs = vec![
        make_msg("user", "msg1"),
        make_msg("assistant", "msg2"),
        make_msg("user", "msg3"),
        make_msg("assistant", "msg4"),
        make_msg("user", "msg5"),
    ];
    let config = config_with_preserve(2);
    let summary = build_structural_summary(&msgs, &config);

    assert!(summary.contains("msg1"), "msg1 should be summarized");
    assert!(summary.contains("msg2"), "msg2 should be summarized");
    assert!(summary.contains("msg3"), "msg3 should be summarized");
    assert!(
        !summary.contains("msg4"),
        "msg4 should be preserved (not summarized)"
    );
    assert!(
        !summary.contains("msg5"),
        "msg5 should be preserved (not summarized)"
    );
    assert!(summary.contains("3 messages summarized"));
}

#[test]
fn structural_summary_truncates_long_content() {
    let long_content = "x".repeat(500);
    let msgs = vec![make_msg("user", &long_content)];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);

    // Content should be truncated to 200 chars + "..."
    assert!(summary.contains("..."), "should have ellipsis marker");
    // Summary shouldn't contain the full 500-char content
    assert!(
        !summary.contains(&"x".repeat(201)),
        "should not contain 201+ consecutive x's"
    );
}

#[test]
fn structural_summary_no_truncation_for_short_content() {
    let msgs = vec![make_msg("user", "short")];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("short"));
}

#[test]
fn structural_summary_empty_messages_zero_count() {
    let msgs: Vec<PipelineMessage> = Vec::new();
    let config = config_with_preserve(3);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("0 messages summarized"));
}

#[test]
fn structural_summary_preserve_exceeds_len() {
    // If preserve_turns > messages.len(), everything is preserved and nothing summarized
    let msgs = vec![make_msg("user", "only one")];
    let config = config_with_preserve(10);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("0 messages summarized"));
    assert!(!summary.contains("only one"));
}

#[test]
fn structural_summary_includes_role_prefix() {
    let msgs = vec![
        make_msg("user", "question"),
        make_msg("assistant", "answer"),
        make_msg("tool_result", "output"),
    ];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("[user]"));
    assert!(summary.contains("[assistant]"));
    assert!(summary.contains("[tool_result]"));
}

#[test]
fn structural_summary_handles_multibyte_content() {
    // Ensure char-based truncation doesn't panic on multibyte characters
    let multibyte = "héllo wörld 🌍 ".repeat(50); // well over 200 chars
    let msgs = vec![make_msg("user", &multibyte)];
    let config = config_with_preserve(0);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("héllo"));
    assert!(summary.contains("..."));
}

#[test]
fn structural_summary_preserve_exactly_equals_len() {
    // preserve_turns == len: everything is preserved, nothing summarized
    let msgs = vec![make_msg("user", "one"), make_msg("assistant", "two")];
    let config = config_with_preserve(2);
    let summary = build_structural_summary(&msgs, &config);
    assert!(summary.contains("0 messages summarized"));
}

// --- Reflection stage tests ---

#[tokio::test]
async fn reflection_stage_disabled_skips() {
    let config = NousConfig::default();
    let pipeline_config = PipelineConfig::default();
    let mut ctx = PipelineContext::default();
    let emitter = EventEmitter::new();

    run_reflection_stage(&config, &pipeline_config, &mut ctx, &emitter).await;

    let result = ctx
        .reflection_result
        .expect("reflection_result should be set");
    assert_eq!(result.status, ReflectionStatus::Skipped);
    assert_eq!(result.facts_emitted, 0);
}

#[tokio::test]
async fn reflection_stage_enabled_no_store() {
    let config = NousConfig::default();
    let pipeline_config = PipelineConfig {
        reflection_enabled: true,
        ..PipelineConfig::default()
    };
    let mut ctx = PipelineContext::default();
    let emitter = EventEmitter::new();

    run_reflection_stage(&config, &pipeline_config, &mut ctx, &emitter).await;

    let result = ctx
        .reflection_result
        .expect("reflection_result should be set");
    assert_eq!(result.status, ReflectionStatus::NoStore);
    assert_eq!(result.facts_emitted, 0);
}

#[tokio::test]
async fn reflection_stage_timeout_path() {
    let config = NousConfig::default();
    let pipeline_config = PipelineConfig {
        reflection_enabled: true,
        stage_budget: StageBudget {
            reflection_secs: 1,
            ..StageBudget::default()
        },
        ..PipelineConfig::default()
    };
    let mut ctx = PipelineContext::default();
    let emitter = EventEmitter::new();

    // With the current no-op implementation the timeout never fires,
    // but the timeout wrapper is exercised and the stage completes.
    run_reflection_stage(&config, &pipeline_config, &mut ctx, &emitter).await;

    let result = ctx
        .reflection_result
        .expect("reflection_result should be set");
    assert_eq!(result.status, ReflectionStatus::NoStore);
}

#[test]
fn apply_recall_result_injects_into_system_prompt_by_default() {
    let mut ctx = PipelineContext {
        system_prompt: Some("base prompt".to_owned()),
        messages: vec![make_msg("user", "hello")],
        remaining_tokens: 100,
        ..PipelineContext::default()
    };
    let recall = crate::recall::RecallStageResult {
        candidates_found: 1,
        results_injected: 1,
        tokens_consumed: 10,
        recall_section: Some("## Recalled Knowledge\n- fact".to_owned()),
        fact_ids: vec!["f1".to_owned()],
    };
    let span = tracing::info_span!("test");
    super::apply_recall_result(Ok(recall), &mut ctx, &span, false);
    assert!(
        ctx.system_prompt
            .as_ref()
            .is_some_and(|p| p.contains("Recalled Knowledge")),
        "recall should be appended to system prompt"
    );
    assert_eq!(ctx.messages.len(), 1, "messages should not grow");
    assert_eq!(ctx.remaining_tokens, 90, "tokens should be deducted");
}

#[test]
fn apply_recall_result_late_inject_appends_system_message() {
    let mut ctx = PipelineContext {
        system_prompt: Some("base prompt".to_owned()),
        messages: vec![make_msg("user", "hello")],
        remaining_tokens: 100,
        ..PipelineContext::default()
    };
    let recall = crate::recall::RecallStageResult {
        candidates_found: 1,
        results_injected: 1,
        tokens_consumed: 10,
        recall_section: Some("## Recalled Knowledge\n- fact".to_owned()),
        fact_ids: vec!["f1".to_owned()],
    };
    let span = tracing::info_span!("test");
    super::apply_recall_result(Ok(recall), &mut ctx, &span, true);
    assert!(
        !ctx.system_prompt
            .as_ref()
            .is_some_and(|p| p.contains("Recalled Knowledge")),
        "recall should NOT be appended to system prompt"
    );
    assert_eq!(ctx.messages.len(), 2, "messages should grow by 1");
    assert!(
        ctx.messages
            .get(1)
            .is_some_and(|m| m.role == "system" && m.content.contains("Recalled Knowledge"))
    );
    assert_eq!(ctx.remaining_tokens, 90, "tokens should be deducted");
}
