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

use hermeneus::types::ToolResultType;

pub(crate) mod full;
pub(crate) mod micro;
pub(crate) mod prompts;

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
    pub(crate) path: String,
    /// Content to re-inject.
    pub(crate) content: String,
    /// Token estimate for the content.
    pub(crate) token_estimate: i64,
}

/// Reason a compaction pass was triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompactReason {
    /// Mid-session token cap hit.
    TokenBudget,
    /// Session-end checkpoint.
    SessionBoundary,
    /// Operator-issued compact (defaults to terse).
    OperatorRequest,
    /// Background consolidation pass (#95).
    DreamConsolidation,
}

#[expect(
    dead_code,
    reason = "keeps all CompactReason variants alive for exhaustive-match maintenance"
)]
fn touch_all_compact_reasons() {
    let _ = CompactReason::TokenBudget;
    let _ = CompactReason::SessionBoundary;
    let _ = CompactReason::OperatorRequest;
    let _ = CompactReason::DreamConsolidation;
}

/// Select the appropriate prompt for a given compaction reason.
#[must_use]
pub fn select_prompt(reason: CompactReason) -> &'static str {
    match reason {
        CompactReason::TokenBudget | CompactReason::OperatorRequest => prompts::COMPACT_PROMPT,
        CompactReason::SessionBoundary | CompactReason::DreamConsolidation => {
            prompts::RESTORE_PROMPT
        }
    }
}

use crate::memory::step::Step;

/// Strategy for applying context compaction to a sequence of steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "#193 will wire CompactionStrategy into the pipeline"
    )
)]
#[non_exhaustive]
pub enum CompactionStrategy {
    /// Uniform tail truncation (current default).
    UniformTail,
    /// Step-positional: last 2 steps full, earlier i < n-2 notes-only.
    StepPositional,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "#193 will wire CompactionStrategy into the pipeline"
    )
)]
impl CompactionStrategy {
    /// Apply the strategy to a step sequence under a token budget.
    ///
    /// Returns a new sequence where steps may be compacted or dropped
    /// depending on the strategy and budget.
    ///
    /// # Note
    ///
    /// [`CompactionStrategy::StepPositional`] is currently a stub: it
    /// delegates to [`CompactionStrategy::UniformTail`] until #193
    /// implements the actual `i < n-2 → notes-only` rule.
    #[must_use]
    pub fn apply(self, steps: &[Step], budget: usize) -> Vec<Step> {
        match self {
            Self::UniformTail => Self::apply_uniform_tail(steps, budget),
            Self::StepPositional => {
                // TODO(#193)[deliberate-prudent]: implement step-positional degradation:
                //   i < n-2 → notes only (Step::compact)
                //   last 2 steps → full content
                // For now, delegate to UniformTail to avoid behavioural change.
                Self::apply_uniform_tail(steps, budget)
            }
        }
    }

    fn apply_uniform_tail(steps: &[Step], budget: usize) -> Vec<Step> {
        let mut result = Vec::new();
        let mut used: usize = 0;

        for step in steps.iter().rev() {
            let cost = step.token_estimate();
            let new_used = used.saturating_add(cost);
            if new_used > budget && !result.is_empty() {
                break;
            }
            result.push(step.clone());
            used = new_used;
        }

        result.reverse();
        result
    }
}

#[cfg(test)]
mod prompt_tests;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test assertions use .expect() for descriptive panic messages"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting sufficient length"
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

    #[test]
    fn uniform_tail_preserves_recent_steps() {
        let steps: Vec<Step> = (0..5)
            .map(|i| Step {
                self_note: format!("step {i}"),
                observations: Vec::new(),
                summary: None,
                index: i,
                started_at: jiff::Timestamp::now(),
            })
            .collect();

        // Each step is ~3 tokens ("step N".len() / 4 = 2 or 3).
        // Budget of 6 tokens should keep the last 2-3 steps.
        let result = CompactionStrategy::UniformTail.apply(&steps, 6);
        assert!(
            !result.is_empty(),
            "UniformTail should preserve at least the most recent step"
        );
        assert_eq!(
            result.last().expect("result should not be empty").index,
            4,
            "most recent step (index 4) should always be preserved"
        );
    }

    #[test]
    fn uniform_tail_passthrough_when_under_budget() {
        let steps: Vec<Step> = (0..3)
            .map(|i| Step {
                self_note: "x".to_owned(),
                observations: Vec::new(),
                summary: None,
                index: i,
                started_at: jiff::Timestamp::now(),
            })
            .collect();

        let result = CompactionStrategy::UniformTail.apply(&steps, 10_000);
        assert_eq!(
            result.len(),
            steps.len(),
            "under-budget should return all steps unchanged"
        );
        assert_eq!(result[0].index, 0);
        assert_eq!(result[2].index, 2);
    }

    #[test]
    fn step_positional_stub_delegates_to_uniform_tail() {
        let steps: Vec<Step> = (0..5)
            .map(|i| Step {
                self_note: format!("step {i}"),
                observations: Vec::new(),
                summary: None,
                index: i,
                started_at: jiff::Timestamp::now(),
            })
            .collect();

        let uniform_result = CompactionStrategy::UniformTail.apply(&steps, 6);
        let positional_result = CompactionStrategy::StepPositional.apply(&steps, 6);
        assert_eq!(
            uniform_result.len(),
            positional_result.len(),
            "StepPositional stub should produce same length as UniformTail"
        );
        assert_eq!(
            uniform_result
                .last()
                .expect("uniform_result not empty")
                .index,
            positional_result
                .last()
                .expect("positional_result not empty")
                .index,
            "StepPositional stub should preserve same tail as UniformTail"
        );
    }
}
