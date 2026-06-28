//! Full compaction: summarizes conversation history via model call.
//!
//! Triggers when token usage exceeds a configurable threshold of the context
//! window. Replaces history with a summary plus the last N turns. After
//! compaction, critical files (recently modified or referenced) are re-injected
//! so the agent retains context about files it's actively editing.

use tracing::debug;

use crate::budget::CompactionMetrics;
use crate::memory::step::Step;
use crate::pipeline::PipelineMessage;

use super::{CompactConfig, CompactionStrategy, CriticalFile};

/// Result of a full compaction pass.
#[derive(Debug, Clone)]
pub(crate) struct FullCompactionResult {
    /// Updated message list (summary + preserved tail + critical files).
    pub(crate) messages: Vec<PipelineMessage>,
    /// Compaction metrics.
    pub(crate) metrics: CompactionMetrics,
    /// Critical files that were re-injected.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read in tests and future pipeline telemetry")
    )]
    pub(crate) critical_files_restored: Vec<String>,
}

/// Check whether full compaction should trigger based on token usage.
///
/// Returns `true` when the ratio of consumed tokens to context window
/// exceeds the configured threshold.
#[must_use]
pub(crate) fn should_trigger(
    consumed_tokens: u64,
    context_window: u64,
    config: &CompactConfig,
) -> bool {
    if context_window == 0 {
        return false;
    }
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "u64→f64: context windows and consumed tokens are bounded by model context limits (current max ~2M tokens), well below f64 mantissa 2^53"
    )]
    let ratio = consumed_tokens as f64 / context_window as f64;
    ratio >= config.full_compact_threshold
}

/// Build the summarization request from conversation history.
///
/// Extracts the messages that will be summarized (everything except the
/// last `preserve_turns` turns) and formats them for the model call.
#[must_use]
pub(crate) fn build_summary_request(
    messages: &[PipelineMessage],
    config: &CompactConfig,
    prompt: &str,
) -> (String, Vec<PipelineMessage>) {
    // WHY: preserve the most recent turns because they contain active context
    let preserve_count = config.preserve_turns.min(messages.len());
    let split_point = messages.len().saturating_sub(preserve_count);

    let to_summarize = messages.get(..split_point).unwrap_or(&[]);
    let to_preserve = messages.get(split_point..).unwrap_or(&[]).to_vec();
    let preserved = match config.strategy {
        CompactionStrategy::UniformTail => to_preserve,
        CompactionStrategy::StepPositional => {
            let steps = crate::pipeline::assemble_steps(&to_preserve);
            let budget = steps
                .iter()
                .map(Step::token_estimate)
                .fold(0usize, usize::saturating_add);
            let compacted = config.strategy.apply(&steps, budget);
            render_steps(&compacted)
        }
    };

    let mut history_text = String::new();
    for msg in to_summarize {
        history_text.push_str(&msg.role);
        history_text.push_str(": ");
        history_text.push_str(&msg.content);
        history_text.push('\n');
    }

    let request = format!("{prompt}\n\n---\n\n{history_text}");
    (request, preserved)
}

#[must_use]
pub(crate) fn render_steps(steps: &[Step]) -> Vec<PipelineMessage> {
    steps
        .iter()
        .flat_map(|step| {
            let mut messages = Vec::with_capacity(1 + step.observations.len());
            messages.push(PipelineMessage::text(
                "assistant",
                step.self_note.clone(),
                i64::try_from(step.self_note.len().div_ceil(4)).unwrap_or(i64::MAX),
            ));
            messages.extend(step.observations.iter().map(|observation| {
                PipelineMessage::text(
                    "user",
                    observation.body.clone(),
                    i64::try_from(observation.token_estimate).unwrap_or(i64::MAX),
                )
            }));
            messages
        })
        .collect()
}

/// Apply full compaction: replace history with summary + preserved tail.
///
/// This is the second phase after the model returns a summary. It rebuilds
/// the message list FROM the summary, preserved messages, and critical files.
pub(crate) fn apply_compaction(
    summary: &str,
    preserved_messages: Vec<PipelineMessage>,
    critical_files: Vec<CriticalFile>,
    original_token_count: u64,
    config: &CompactConfig,
) -> FullCompactionResult {
    let mut messages = Vec::new();

    // NOTE: summary becomes a system-like context message
    #[expect(
        clippy::cast_possible_wrap,
        clippy::as_conversions,
        reason = "usize->i64: summary length fits in i64"
    )]
    let summary_tokens = ((summary.len() as i64) + 3) / 4; // kanon:ignore RUST/as-cast
    // WHY(#3781): mark the summary message with cache_breakpoint=true so
    // subsequent turns know to cache up to this point, achieving cache_read
    // pricing on the next turn after compaction.
    messages.push(
        PipelineMessage::text(
            "user",
            format!("[Conversation summary FROM compaction]\n{summary}"),
            summary_tokens,
        )
        .with_cache_breakpoint(true),
    );

    // NOTE: re-inject critical files before preserved messages
    let mut restored_files = Vec::new();
    let files_to_restore = critical_files.into_iter().take(config.max_critical_files);

    for file in files_to_restore {
        messages.push(PipelineMessage::text(
            "user",
            format!(
                "[Critical file restored after compaction: {}]\n{}",
                file.path, file.content
            ),
            file.token_estimate,
        ));
        restored_files.push(file.path);
    }

    // NOTE: preserved tail messages (most recent turns)
    messages.extend(preserved_messages);

    #[expect(
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "i64->u64: token estimates are non-negative in practice"
    )]
    let post_tokens: u64 = messages
        .iter()
        .map(|m| m.token_estimate.max(0) as u64) // kanon:ignore RUST/as-cast
        .sum();

    let metrics = CompactionMetrics {
        pre_compact_tokens: original_token_count,
        post_compact_tokens: post_tokens,
        results_cleared: 0,
        results_preserved: 0,
        full_compaction_triggered: true,
    };

    debug!(
        pre = original_token_count,
        post = post_tokens,
        reclaimed = metrics.tokens_reclaimed(),
        critical_files = restored_files.len(),
        "full compaction applied"
    );

    FullCompactionResult {
        messages,
        metrics,
        critical_files_restored: restored_files,
    }
}

/// Identify critical files FROM recent conversation history.
///
/// Scans the last `lookback` turns for file references. Critical files are:
/// 1. Files modified by the agent (indicated by write/edit tool results)
/// 2. Files referenced in the last assistant message
///
/// Returns up to `max_files` unique file entries.
pub(crate) fn identify_critical_files(
    messages: &[PipelineMessage],
    config: &CompactConfig,
) -> Vec<CriticalFile> {
    let mut files: Vec<CriticalFile> = Vec::new();
    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    let lookback_start = messages
        .len()
        .saturating_sub(config.critical_file_lookback * 2);
    let recent = messages.get(lookback_start..).unwrap_or(&[]);

    for msg in recent {
        if !msg.is_tool_result() {
            continue;
        }
        // WHY: tool results with file operations contain the file path and content
        if let Some(path) = extract_file_path(&msg.content) {
            if seen_paths.contains(&path) {
                continue;
            }
            if files.len() >= config.max_critical_files {
                break;
            }
            // NOTE: extract content after the metadata header
            let content = extract_file_content(&msg.content);
            seen_paths.insert(path.clone());
            files.push(CriticalFile {
                path,
                content,
                token_estimate: msg.token_estimate,
            });
        }
    }

    files
}

/// Extract a file path FROM a tool result message.
///
/// Looks for file operation tool results that contain path information.
fn extract_file_path(content: &str) -> Option<String> {
    // WHY: tool results formatted by format_tool_result have structure:
    // [tool:<name>@<timestamp>] <content>
    if !content.starts_with("[tool:") {
        return None;
    }
    let end_bracket = content.find(']')?;
    let metadata = content.get(6..end_bracket)?;
    let at_pos = metadata.find('@')?;
    let tool_name = metadata.get(..at_pos)?;

    // NOTE: only extract paths FROM file operations
    let tool_type = hermeneus::types::ToolResultType::classify(tool_name);
    if tool_type != hermeneus::types::ToolResultType::FileOperation {
        return None;
    }

    // WHY: the content after the bracket often starts with the file path
    let after_bracket = content.get(end_bracket + 1..)?.trim_start();
    // NOTE: heuristic -- first line of file operation output is often the path
    let first_line = after_bracket.lines().next()?;
    if first_line.contains('/') || first_line.contains('.') {
        Some(first_line.trim().to_owned())
    } else {
        None
    }
}

/// Extract file content FROM a tool result (everything after the header).
fn extract_file_content(content: &str) -> String {
    if let Some(bracket_end) = content.find("] ") {
        content.get(bracket_end + 2..).unwrap_or("").to_owned()
    } else {
        content.to_owned()
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting sufficient length"
)]
mod tests {
    use super::*;
    use crate::compact::micro::format_tool_result;
    use crate::memory::step::{Observation, Step};

    fn make_text_msg(role: &str, content: &str, tokens: i64) -> PipelineMessage {
        PipelineMessage::text(role, content, tokens)
    }

    fn make_step(index: usize, self_note: &str, observation_body: &str) -> Step {
        Step {
            self_note: self_note.to_owned(),
            observations: vec![Observation::new("shell", observation_body)],
            summary: None,
            index,
            started_at: jiff::Timestamp::UNIX_EPOCH,
        }
    }

    fn message_signature(messages: &[PipelineMessage]) -> Vec<(String, String, i64, bool)> {
        messages
            .iter()
            .map(|message| {
                (
                    message.role.clone(),
                    message.content.clone(),
                    message.token_estimate,
                    message.cache_breakpoint,
                )
            })
            .collect()
    }

    fn make_tool_msg(tool_name: &str, content: &str, tokens: i64) -> PipelineMessage {
        let ts = jiff::Timestamp::UNIX_EPOCH;
        PipelineMessage::text("user", format_tool_result(tool_name, ts, content), tokens)
    }

    #[test]
    fn should_trigger_at_threshold() {
        let config = CompactConfig::default();
        // 80% of 100,000 = 80,000
        assert!(
            should_trigger(80_000, 100_000, &config),
            "should trigger at exactly 80% threshold"
        );
        assert!(
            should_trigger(90_000, 100_000, &config),
            "should trigger above 80% threshold"
        );
        assert!(
            !should_trigger(79_999, 100_000, &config),
            "should not trigger below 80% threshold"
        );
    }

    #[test]
    fn should_trigger_zero_window() {
        let config = CompactConfig::default();
        assert!(
            !should_trigger(100, 0, &config),
            "should not trigger with zero context window"
        );
    }

    #[test]
    fn should_trigger_custom_threshold() {
        let config = CompactConfig {
            full_compact_threshold: 0.50,
            ..CompactConfig::default()
        };
        assert!(
            should_trigger(50_000, 100_000, &config),
            "should trigger at custom 50% threshold"
        );
        assert!(
            !should_trigger(49_999, 100_000, &config),
            "should not trigger below custom 50% threshold"
        );
    }

    #[test]
    fn build_summary_request_preserves_tail() {
        let config = CompactConfig {
            preserve_turns: 2,
            ..CompactConfig::default()
        };
        let messages = vec![
            make_text_msg("user", "old message 1", 50),
            make_text_msg("assistant", "old reply 1", 50),
            make_text_msg("user", "recent message", 50),
            make_text_msg("assistant", "recent reply", 50),
        ];

        let (request, preserved) = build_summary_request(&messages, &config, "test prompt");
        assert_eq!(preserved.len(), 2, "should preserve last 2 messages");
        assert_eq!(
            preserved[0].content, "recent message",
            "first preserved message should be 'recent message'"
        );
        assert!(
            request.contains("old message 1"),
            "summarization request should contain old messages"
        );
        assert!(
            !request.contains("recent message"),
            "summarization request should not contain preserved messages"
        );
    }

    #[test]
    fn render_steps_round_trips_self_notes_and_tool_prefixes() {
        let steps = vec![
            make_step(
                0,
                "note 0",
                &format_tool_result("shell", jiff::Timestamp::UNIX_EPOCH, "obs 0"),
            ),
            make_step(
                1,
                "note 1",
                &format_tool_result("shell", jiff::Timestamp::UNIX_EPOCH, "obs 1"),
            ),
        ];

        let rendered = render_steps(&steps);
        assert!(
            rendered
                .iter()
                .any(|message| message.role == "user" && message.content.starts_with("[tool:")),
            "rendered observations should keep the [tool:] prefix"
        );

        let round_tripped = crate::pipeline::assemble_steps(&rendered);
        assert_eq!(
            round_tripped.len(),
            steps.len(),
            "rendered steps should round-trip through assemble_steps"
        );
        for (expected, actual) in steps.iter().zip(round_tripped.iter()) {
            assert_eq!(
                actual.self_note, expected.self_note,
                "self_note should survive the render/assemble round-trip"
            );
        }
        assert_eq!(
            round_tripped[0].observations[0].body, steps[0].observations[0].body,
            "observation body should round-trip"
        );
    }

    #[test]
    fn build_summary_request_step_positional_changes_preserved_tail() {
        let messages: Vec<PipelineMessage> = (0..6)
            .flat_map(|step_index| {
                let note = format!("note {step_index}");
                let observation_body = format!("observation {step_index} {}", "x".repeat(256));
                let observation =
                    format_tool_result("shell", jiff::Timestamp::UNIX_EPOCH, &observation_body);
                vec![
                    make_text_msg("assistant", &note, 10),
                    make_text_msg("user", &observation, 90),
                ]
            })
            .collect();

        let uniform_config = CompactConfig {
            preserve_turns: 6,
            strategy: CompactionStrategy::UniformTail,
            ..CompactConfig::default()
        };
        let positional_config = CompactConfig {
            preserve_turns: 6,
            strategy: CompactionStrategy::StepPositional,
            ..CompactConfig::default()
        };

        let (_uniform_request, uniform_tail) =
            build_summary_request(&messages, &uniform_config, "test prompt");
        let (_positional_request, positional_tail) =
            build_summary_request(&messages, &positional_config, "test prompt");

        assert_eq!(
            message_signature(&uniform_tail),
            message_signature(&messages[6..]),
            "UniformTail should keep the exact current whole-message tail"
        );
        assert_ne!(
            message_signature(&positional_tail),
            message_signature(&uniform_tail),
            "StepPositional should change the preserved tail"
        );

        let positional_steps = crate::pipeline::assemble_steps(&positional_tail);
        assert_eq!(
            positional_steps.len(),
            3,
            "three preserved steps should remain visible after StepPositional compaction"
        );
        assert_eq!(positional_steps[0].self_note, "note 3");
        assert!(
            positional_steps[0].observations.is_empty(),
            "older preserved step should lose observations under StepPositional"
        );
        assert!(
            !positional_steps[1].observations.is_empty(),
            "the last two preserved steps should keep their observations"
        );
        assert!(
            !positional_steps[2].observations.is_empty(),
            "the last two preserved steps should keep their observations"
        );
        assert!(
            uniform_tail
                .iter()
                .any(|message| message.content.contains("observation 3")),
            "UniformTail should keep the older observation body byte-for-byte"
        );
        assert!(
            !positional_tail
                .iter()
                .any(|message| message.content.contains("observation 3")),
            "StepPositional should strip the older observation body from the preserved tail"
        );
    }

    #[test]
    fn apply_compaction_builds_correct_structure() {
        let config = CompactConfig::default();
        let preserved = vec![
            make_text_msg("user", "current question", 50),
            make_text_msg("assistant", "current answer", 50),
        ];
        let critical_files = vec![CriticalFile {
            path: "src/main.rs".to_owned(),
            content: "fn main() {}".to_owned(),
            token_estimate: 10,
        }];

        let result = apply_compaction(
            "Summary of previous conversation",
            preserved,
            critical_files,
            10_000,
            &config,
        );

        assert!(
            result.metrics.full_compaction_triggered,
            "should flag full compaction"
        );
        assert_eq!(
            result.critical_files_restored.len(),
            1,
            "should restore one critical file"
        );
        assert_eq!(
            result.critical_files_restored[0], "src/main.rs",
            "restored file should be src/main.rs"
        );

        // NOTE: structure is: summary + critical files + preserved tail
        assert!(
            result.messages[0].content.contains("Conversation summary"),
            "first message should be the summary"
        );
        assert!(
            result.messages[1].content.contains("src/main.rs"),
            "second message should be critical file"
        );
        assert_eq!(
            result.messages[2].content, "current question",
            "third message should be preserved user message"
        );
    }

    #[test]
    fn apply_compaction_tracks_token_metrics() {
        let config = CompactConfig::default();
        let result = apply_compaction(
            "short summary",
            vec![make_text_msg("user", "q", 10)],
            Vec::new(),
            50_000,
            &config,
        );

        assert_eq!(
            result.metrics.pre_compact_tokens, 50_000,
            "pre-compact tokens should match input"
        );
        assert!(
            result.metrics.post_compact_tokens < 50_000,
            "post-compact tokens should be less than pre-compact"
        );
        assert!(
            result.metrics.tokens_reclaimed() > 0,
            "should reclaim tokens"
        );
    }

    #[test]
    fn apply_compaction_limits_critical_files() {
        let config = CompactConfig {
            max_critical_files: 2,
            ..CompactConfig::default()
        };
        let files = (0..5)
            .map(|i| CriticalFile {
                path: format!("file_{i}.rs"),
                content: format!("content {i}"),
                token_estimate: 10,
            })
            .collect();

        let result = apply_compaction("summary", Vec::new(), files, 1000, &config);
        assert_eq!(
            result.critical_files_restored.len(),
            2,
            "should LIMIT to max_critical_files"
        );
    }

    #[test]
    fn identify_critical_files_finds_file_operations() {
        let config = CompactConfig {
            critical_file_lookback: 3,
            max_critical_files: 5,
            ..CompactConfig::default()
        };
        let messages = vec![
            make_tool_msg("file_read", "src/lib.rs\nfn main() {}", 100),
            make_text_msg("assistant", "I see the file", 50),
            make_tool_msg("bash", "ls output here", 50),
            make_text_msg("assistant", "shell done", 30),
        ];

        let files = identify_critical_files(&messages, &config);
        assert_eq!(files.len(), 1, "should identify one file operation");
        assert_eq!(
            files[0].path, "src/lib.rs",
            "should extract file path FROM first line"
        );
    }

    #[test]
    fn identify_critical_files_deduplicates() {
        let config = CompactConfig {
            critical_file_lookback: 5,
            max_critical_files: 5,
            ..CompactConfig::default()
        };
        let messages = vec![
            make_tool_msg("file_read", "src/lib.rs\ncontent v1", 100),
            make_text_msg("assistant", "editing", 20),
            make_tool_msg("file_read", "src/lib.rs\ncontent v2", 100),
            make_text_msg("assistant", "done", 20),
        ];

        let files = identify_critical_files(&messages, &config);
        assert_eq!(files.len(), 1, "should deduplicate same file path");
    }

    #[test]
    fn build_summary_request_handles_empty_messages() {
        let config = CompactConfig::default();
        let messages: Vec<PipelineMessage> = Vec::new();
        let (request, preserved) = build_summary_request(&messages, &config, "test prompt");
        assert!(preserved.is_empty(), "preserved should be empty");
        assert!(
            request.contains("test prompt"),
            "request should contain the prompt"
        );
    }

    #[test]
    fn build_summary_request_preserves_all_when_fewer_than_threshold() {
        let config = CompactConfig {
            preserve_turns: 10,
            ..CompactConfig::default()
        };
        let messages = vec![
            make_text_msg("user", "msg1", 10),
            make_text_msg("assistant", "msg2", 10),
        ];
        let (_request, preserved) = build_summary_request(&messages, &config, "test prompt");
        assert_eq!(
            preserved.len(),
            2,
            "should preserve all messages when fewer than threshold"
        );
    }

    #[test]
    fn cached_microcompact_marks_summary_with_cache_breakpoint() {
        // WHY(#3781): regression test to ensure that when full compaction
        // produces a distilled summary, it marks that summary with
        // cache_breakpoint=true so the next turn benefits from cached-read pricing.
        let config = CompactConfig::default();
        let preserved = vec![
            make_text_msg("user", "current question", 50),
            make_text_msg("assistant", "current answer", 50),
        ];
        let critical_files = vec![];

        let result = apply_compaction(
            "Summary of previous conversation",
            preserved,
            critical_files,
            10_000,
            &config,
        );

        // The first message should be the summary
        assert!(
            !result.messages.is_empty(),
            "result should contain at least the summary message"
        );
        let summary_msg = &result.messages[0];
        assert!(
            summary_msg
                .content
                .starts_with("[Conversation summary FROM compaction]"),
            "first message should be the distilled summary"
        );
        // WHY(#3781): the cache_breakpoint flag must be set so that when the
        // pipeline loads this message again, it knows to enable cache_turns.
        assert!(
            summary_msg.cache_breakpoint,
            "distilled summary should have cache_breakpoint=true for cached-read pricing"
        );
    }
}
