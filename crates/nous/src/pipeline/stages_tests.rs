use super::*;
use crate::compact::CompactConfig;
use crate::pipeline::PipelineMessage;

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
