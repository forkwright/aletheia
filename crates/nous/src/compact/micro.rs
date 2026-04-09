//! Microcompaction: in-place clearing of expired tool results.
//!
//! Runs synchronously on the pipeline thread every turn. Iterates tool results
//! in history, checks age against per-type TTLs, and replaces expired content
//! with cleared markers. The last N results per tool type are always preserved
//! regardless of age.

use std::collections::HashMap;

use tracing::debug;

use aletheia_hermeneus::types::ToolResultType;

use crate::budget::CompactionMetrics;
use crate::pipeline::PipelineMessage;

use super::CompactConfig;

/// Marker prefix for cleared tool results.
const CLEARED_MARKER_PREFIX: &str = "[Cleared: ";

/// Run microcompaction on pipeline messages, replacing expired tool results.
///
/// Returns updated messages and compaction metrics. Messages that are not
/// tool results pass through unchanged. The last `config.keep_last_n` results
/// per tool type are preserved regardless of age.
///
/// # Complexity
///
/// O(m × t) where m is the number of messages and t is the number of unique
/// tool types. Groups messages by tool type and processes each group separately.
///
/// # Arguments
///
/// - `messages`: pipeline messages (history + current). Modified in place.
/// - `config`: compaction configuration with per-type TTLs.
/// - `now`: current timestamp for age comparison.
pub(crate) fn run_microcompaction(
    messages: &mut [PipelineMessage],
    config: &CompactConfig,
    now: jiff::Timestamp,
) -> CompactionMetrics {
    let mut metrics = CompactionMetrics::default();

    // NOTE: collect all tool results with their indices, grouped by tool type
    let mut by_type: HashMap<ToolResultType, Vec<(usize, jiff::Timestamp, i64)>> = HashMap::new();

    for (idx, msg) in messages.iter().enumerate() {
        if let Some((tool_type, created_at)) = parse_tool_result_metadata(msg) {
            by_type
                .entry(tool_type)
                .or_default()
                .push((idx, created_at, msg.token_estimate));
        }
    }

    // NOTE: pre-count tokens for metrics
    #[expect(
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "i64→u64: token estimates are non-negative in practice"
    )]
    {
        metrics.pre_compact_tokens = messages
            .iter()
            .map(|m| m.token_estimate.max(0) as u64) // kanon:ignore RUST/as-cast
            .sum();
    }

    let mut indices_to_clear: Vec<usize> = Vec::new();

    for (tool_type, mut entries) in by_type {
        let Some(&ttl) = config.ttls.get(&tool_type) else {
            // WHY: tool types without a TTL (e.g., Other) are never auto-cleared
            continue;
        };

        // INVARIANT: entries are sorted by index (insertion ORDER FROM forward iteration)
        // Last N entries by position are the most recent.
        let preserve_count = config.keep_last_n.min(entries.len());
        let clearable_end = entries.len().saturating_sub(preserve_count);
        // WHY: sort by index to ensure the last N entries by position are preserved
        entries.sort_by_key(|(idx, _, _)| *idx);

        for &(idx, created_at, _) in entries.get(..clearable_end).unwrap_or(&[]) {
            let age = now.duration_since(created_at);
            // WHY: SignedDuration comparison  -  if age exceeds TTL, the result is stale
            if age >= ttl {
                indices_to_clear.push(idx);
            }
        }
    }

    // NOTE: apply clearing in reverse ORDER to preserve indices
    indices_to_clear.sort_unstable();
    indices_to_clear.dedup();

    for &idx in indices_to_clear.iter().rev() {
        if let Some(msg) = messages.get_mut(idx) {
            let Some((tool_type, created_at)) = parse_tool_result_metadata(msg) else {
                continue;
            };
            let age = now.duration_since(created_at);
            let age_display = format!("{}s", age.as_secs());
            let marker = format!("{CLEARED_MARKER_PREFIX}{tool_type:?}, age {age_display}]");

            #[expect(
                clippy::as_conversions,
                reason = "usize→u64: marker length is small, always fits in u64"
            )]
            let marker_tokens = (marker.len() as u64).div_ceil(4); // kanon:ignore RUST/as-cast
            #[expect(
                clippy::cast_possible_wrap,
                clippy::as_conversions,
                reason = "u64→i64: marker token count is small, fits in i64"
            )]
            {
                msg.token_estimate = i64::try_from(marker_tokens).unwrap_or_default(); // kanon:ignore RUST/as-cast
            }
            msg.content = marker;
            metrics.results_cleared += 1;
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "usize→u32: message count fits in u32 for practical conversation lengths"
    )]
    {
        metrics.results_preserved = messages
            .iter()
            .filter(|m| {
                parse_tool_result_metadata(m).is_some()
                    && !m.content.starts_with(CLEARED_MARKER_PREFIX)
            })
            .count() as u32; // kanon:ignore RUST/as-cast
    }

    #[expect(
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "i64→u64: token estimates are non-negative in practice"
    )]
    {
        metrics.post_compact_tokens = messages
            .iter()
            .map(|m| m.token_estimate.max(0) as u64) // kanon:ignore RUST/as-cast
            .sum();
    }

    if metrics.results_cleared > 0 {
        debug!(
            cleared = metrics.results_cleared,
            preserved = metrics.results_preserved,
            reclaimed = metrics.tokens_reclaimed(),
            "microcompaction complete"
        );
    }

    metrics
}

/// Extract tool result metadata FROM a pipeline message.
///
/// Uses a convention: tool result messages have role "user" and content
/// that starts with a tool result marker or contains tool metadata encoded
/// in the message. Since `PipelineMessage` uses simplified string content,
/// we look for patterns indicating tool output.
///
/// Returns `(tool_type, created_at)` if the message looks like a tool result.
#[expect(
    clippy::string_slice,
    reason = "metadata prefix is ASCII-only ([tool:<name>@<timestamp>]), byte indices are safe"
)]
fn parse_tool_result_metadata(msg: &PipelineMessage) -> Option<(ToolResultType, jiff::Timestamp)> {
    // WHY: tool result messages carry metadata in a structured prefix
    // Format: "[tool:<name>@<timestamp>] <content>"
    if msg.role != "user" {
        return None;
    }
    let content = &msg.content;
    if !content.starts_with("[tool:") {
        return None;
    }
    let end_bracket = content.find(']')?;
    let metadata = &content[6..end_bracket];
    let at_pos = metadata.find('@')?;
    let tool_name = &metadata[..at_pos];
    let timestamp_str = &metadata[at_pos + 1..];
    let created_at: jiff::Timestamp = timestamp_str.parse().ok()?;
    let tool_type = ToolResultType::classify(tool_name);
    Some((tool_type, created_at))
}

/// Format a tool result with compaction metadata prefix.
///
/// Prepends a parseable metadata header to tool result content so
/// microcompaction can identify tool results and their age.
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired when finalize stage tags tool results with metadata"
    )
)]
pub(crate) fn format_tool_result(
    tool_name: &str,
    created_at: jiff::Timestamp,
    content: &str,
) -> String {
    format!("[tool:{tool_name}@{created_at}] {content}")
}

/// Check whether a message has already been cleared by microcompaction.
#[must_use]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired when execute stage checks cleared state")
)]
pub(crate) fn is_cleared(msg: &PipelineMessage) -> bool {
    msg.content.starts_with(CLEARED_MARKER_PREFIX)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting sufficient length"
)]
mod tests {
    use super::*;

    fn make_tool_msg(
        tool_name: &str,
        created_at: jiff::Timestamp,
        content: &str,
        token_estimate: i64,
    ) -> PipelineMessage {
        PipelineMessage {
            role: "user".to_owned(),
            content: format_tool_result(tool_name, created_at, content),
            token_estimate,
        }
    }

    fn make_text_msg(role: &str, content: &str, tokens: i64) -> PipelineMessage {
        PipelineMessage {
            role: role.to_owned(),
            content: content.to_owned(),
            token_estimate: tokens,
        }
    }

    #[test]
    fn microcompaction_clears_expired_file_result() {
        // WHY: keep_last_n=0 so a single expired result gets cleared
        let config = CompactConfig {
            keep_last_n: 0,
            ..CompactConfig::default()
        };
        let old = jiff::Timestamp::UNIX_EPOCH;
        let now = old
            .checked_add(jiff::SignedDuration::from_mins(10))
            .unwrap();

        let mut messages = vec![
            make_tool_msg("file_read", old, "contents of main.rs", 500),
            make_text_msg("assistant", "I see the file", 50),
            make_text_msg("user", "next question", 20),
        ];

        let metrics = run_microcompaction(&mut messages, &config, now);
        assert_eq!(
            metrics.results_cleared, 1,
            "one expired result should be cleared"
        );
        assert!(
            messages[0].content.starts_with(CLEARED_MARKER_PREFIX),
            "cleared message should start with cleared marker"
        );
        assert!(
            metrics.tokens_reclaimed() > 0,
            "should reclaim tokens FROM cleared result"
        );
    }

    #[test]
    fn microcompaction_preserves_fresh_results() {
        let config = CompactConfig::default();
        let now = jiff::Timestamp::now();
        let recent = now
            .checked_add(jiff::SignedDuration::from_secs(-30))
            .unwrap();

        let mut messages = vec![
            make_tool_msg("file_read", recent, "fresh contents", 200),
            make_text_msg("assistant", "here is the result", 50),
        ];

        let metrics = run_microcompaction(&mut messages, &config, now);
        assert_eq!(
            metrics.results_cleared, 0,
            "fresh results should not be cleared"
        );
        assert!(
            messages[0].content.contains("fresh contents"),
            "fresh result content should be preserved"
        );
    }

    #[test]
    fn microcompaction_preserves_last_n_regardless_of_age() {
        let config = CompactConfig {
            keep_last_n: 2,
            ..CompactConfig::default()
        };
        let old = jiff::Timestamp::UNIX_EPOCH;
        let now = old
            .checked_add(jiff::SignedDuration::from_mins(60))
            .unwrap();

        let mut messages = vec![
            make_tool_msg("file_read", old, "old file 1", 100),
            make_tool_msg("file_read", old, "old file 2", 100),
            make_tool_msg("file_read", old, "old file 3", 100),
            make_tool_msg("file_read", old, "old file 4", 100),
        ];

        let metrics = run_microcompaction(&mut messages, &config, now);
        // WHY: 4 file_read results, keep_last_n=2, so 2 should be cleared
        assert_eq!(
            metrics.results_cleared, 2,
            "should clear all but last 2 results"
        );
        assert!(
            messages[0].content.starts_with(CLEARED_MARKER_PREFIX),
            "first (oldest positionally) should be cleared"
        );
        assert!(
            messages[1].content.starts_with(CLEARED_MARKER_PREFIX),
            "second should be cleared"
        );
        assert!(
            messages[2].content.contains("old file 3"),
            "third (kept as last-N) should be preserved"
        );
        assert!(
            messages[3].content.contains("old file 4"),
            "fourth (kept as last-N) should be preserved"
        );
    }

    #[test]
    fn microcompaction_different_ttls_per_type() {
        let config = CompactConfig::default();
        // NOTE: shell TTL is 3min, file TTL is 5min, keep_last_n=2
        let base = jiff::Timestamp::UNIX_EPOCH;
        // 4 minutes old: expired for shell (3min) but not file (5min)
        let four_min_ago = base
            .checked_add(jiff::SignedDuration::from_mins(4))
            .unwrap();
        let now = base
            .checked_add(jiff::SignedDuration::from_mins(8))
            .unwrap();

        let mut messages = vec![
            make_tool_msg("file_read", four_min_ago, "file content", 200),
            make_tool_msg("bash", four_min_ago, "shell output 1", 200),
            make_tool_msg("bash", four_min_ago, "shell output 2", 200),
            make_tool_msg("bash", four_min_ago, "shell output 3", 200),
            make_tool_msg("bash", four_min_ago, "shell output 4", 200),
        ];

        let metrics = run_microcompaction(&mut messages, &config, now);
        // file_read: 4 min old < 5 min TTL -> preserved
        assert!(
            messages[0].content.contains("file content"),
            "file read within TTL should be preserved"
        );
        // bash: 4 expired, keep_last_n=2 -> 2 cleared, 2 preserved
        assert_eq!(
            metrics.results_cleared, 2,
            "2 expired shell results should be cleared (keep_last_n=2 preserves 2)"
        );
        // WHY: last 2 bash results (indices 3, 4) should be preserved
        assert!(
            messages[3].content.contains("shell output 3"),
            "third-to-last bash result should be preserved"
        );
        assert!(
            messages[4].content.contains("shell output 4"),
            "last bash result should be preserved"
        );
    }

    #[test]
    fn microcompaction_ignores_non_tool_messages() {
        let config = CompactConfig::default();
        let now = jiff::Timestamp::now();

        let mut messages = vec![
            make_text_msg("user", "hello", 10),
            make_text_msg("assistant", "hi there", 20),
            make_text_msg("user", "question", 15),
        ];

        let metrics = run_microcompaction(&mut messages, &config, now);
        assert_eq!(
            metrics.results_cleared, 0,
            "non-tool messages should not be cleared"
        );
        assert_eq!(
            messages[0].content, "hello",
            "non-tool message content should be unchanged"
        );
    }

    #[test]
    fn microcompaction_no_ttl_for_other_type() {
        let config = CompactConfig::default();
        let old = jiff::Timestamp::UNIX_EPOCH;
        let now = old
            .checked_add(jiff::SignedDuration::from_mins(60))
            .unwrap();

        let mut messages = vec![make_tool_msg("calculator", old, "result: 42", 50)];

        let metrics = run_microcompaction(&mut messages, &config, now);
        assert_eq!(
            metrics.results_cleared, 0,
            "Other-type tool results should never be auto-cleared"
        );
    }

    #[test]
    fn microcompaction_token_tracking() {
        let config = CompactConfig {
            keep_last_n: 0,
            ..CompactConfig::default()
        };
        let old = jiff::Timestamp::UNIX_EPOCH;
        let now = old
            .checked_add(jiff::SignedDuration::from_mins(60))
            .unwrap();

        let mut messages = vec![
            make_tool_msg("file_read", old, "large file content here", 500),
            make_text_msg("assistant", "noted", 10),
        ];

        let metrics = run_microcompaction(&mut messages, &config, now);
        assert_eq!(
            metrics.pre_compact_tokens, 510,
            "pre-compact tokens should be sum of all message tokens"
        );
        assert!(
            metrics.post_compact_tokens < 510,
            "post-compact tokens should be less after clearing"
        );
        assert!(
            metrics.tokens_reclaimed() > 0,
            "tokens reclaimed should be positive"
        );
    }

    #[test]
    fn is_cleared_detects_cleared_messages() {
        let cleared = PipelineMessage {
            role: "user".to_owned(),
            content: format!("{CLEARED_MARKER_PREFIX}FileOperation, age 300s]"),
            token_estimate: 10,
        };
        assert!(is_cleared(&cleared), "should detect cleared message");

        let normal = PipelineMessage {
            role: "user".to_owned(),
            content: "normal content".to_owned(),
            token_estimate: 10,
        };
        assert!(!is_cleared(&normal), "should not detect normal message");
    }

    #[test]
    fn format_tool_result_roundtrips_through_parse() {
        let ts = jiff::Timestamp::UNIX_EPOCH;
        let formatted = format_tool_result("file_read", ts, "hello world");
        let msg = PipelineMessage {
            role: "user".to_owned(),
            content: formatted,
            token_estimate: 100,
        };
        let parsed = parse_tool_result_metadata(&msg);
        assert!(
            parsed.is_some(),
            "formatted tool result should be parseable"
        );
        let (tool_type, created_at) = parsed.unwrap();
        assert_eq!(
            tool_type,
            ToolResultType::FileOperation,
            "parsed tool type should match"
        );
        assert_eq!(created_at, ts, "parsed timestamp should match");
    }
}
