//! Context compaction: microcompaction and full compaction passes.
//!
//! Two tiers of compaction work together to extend useful session length:
//!
//! - **Microcompaction** runs every turn as a cheap in-place pass. It replaces
//!   expired tool results with cleared markers, freeing tokens without a model call.
//!
//! - **Full compaction** fires when token usage crosses a configurable threshold.
//!   It summarizes conversation history via a model call and re-injects critical
//!   files that may have been discarded.
//!
//! Microcompaction delays the need for full compaction, and full compaction
//! resets the context for continued productive work.

use std::collections::HashMap;

use jiff::SignedDuration;

use aletheia_hermeneus::types::ToolResultType;

pub(crate) mod full;
pub(crate) mod micro;

/// Per-tool-type TTL configuration for microcompaction.
///
/// Determines how long tool results remain before being replaced with
/// cleared markers. Different tool types have different staleness
/// characteristics.
#[derive(Debug, Clone)]
pub struct CompactConfig {
    /// Per-tool-type time-to-live durations.
    pub ttls: HashMap<ToolResultType, SignedDuration>,
    /// Number of most-recent results per tool type to preserve regardless of age.
    pub keep_last_n: usize,
    /// Token usage ratio (0.0--1.0) that triggers full compaction.
    pub full_compact_threshold: f64,
    /// Number of most-recent turns to preserve after full compaction.
    pub preserve_turns: usize,
    /// Maximum number of critical files to re-inject after full compaction.
    pub max_critical_files: usize,
    /// Number of recent turns to scan for critical file identification.
    pub critical_file_lookback: usize,
}

impl Default for CompactConfig {
    fn default() -> Self {
        let mut ttls = HashMap::new();
        // WHY: file content changes slowly relative to turn rate
        ttls.insert(ToolResultType::FileOperation, SignedDuration::from_mins(5));
        // WHY: shell results are more ephemeral
        ttls.insert(ToolResultType::ShellOutput, SignedDuration::from_mins(3));
        // WHY: search context becomes stale quickly as focus shifts
        ttls.insert(ToolResultType::SearchResult, SignedDuration::from_mins(2));
        // WHY: web results are similar to search in staleness
        ttls.insert(ToolResultType::WebResult, SignedDuration::from_mins(2));

        Self {
            ttls,
            keep_last_n: 2,
            full_compact_threshold: 0.80,
            preserve_turns: 3,
            max_critical_files: 5,
            critical_file_lookback: 3,
        }
    }
}

/// Identifies a critical file that should be re-injected after full compaction.
#[derive(Debug, Clone)]
pub(crate) struct CriticalFile {
    /// File path.
    pub path: String,
    /// Content to re-inject.
    pub content: String,
    /// Token estimate for the content.
    pub token_estimate: i64,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test assertions use .expect() for descriptive panic messages"
)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_ttls() {
        let config = CompactConfig::default();
        let file_ttl = config
            .ttls
            .get(&ToolResultType::FileOperation)
            .expect("FileOperation TTL should be present");
        assert_eq!(
            file_ttl.as_secs(),
            300,
            "FileOperation TTL should be 5 minutes (300s)"
        );

        let shell_ttl = config
            .ttls
            .get(&ToolResultType::ShellOutput)
            .expect("ShellOutput TTL should be present");
        assert_eq!(
            shell_ttl.as_secs(),
            180,
            "ShellOutput TTL should be 3 minutes (180s)"
        );

        let search_ttl = config
            .ttls
            .get(&ToolResultType::SearchResult)
            .expect("SearchResult TTL should be present");
        assert_eq!(
            search_ttl.as_secs(),
            120,
            "SearchResult TTL should be 2 minutes (120s)"
        );

        let web_ttl = config
            .ttls
            .get(&ToolResultType::WebResult)
            .expect("WebResult TTL should be present");
        assert_eq!(
            web_ttl.as_secs(),
            120,
            "WebResult TTL should be 2 minutes (120s)"
        );
    }

    #[test]
    fn default_config_has_expected_defaults() {
        let config = CompactConfig::default();
        assert_eq!(config.keep_last_n, 2, "keep_last_n should default to 2");
        assert!(
            (config.full_compact_threshold - 0.80).abs() < f64::EPSILON,
            "full_compact_threshold should default to 0.80"
        );
        assert_eq!(
            config.preserve_turns, 3,
            "preserve_turns should default to 3"
        );
        assert_eq!(
            config.max_critical_files, 5,
            "max_critical_files should default to 5"
        );
    }
}
