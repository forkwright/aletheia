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
    expect(dead_code, reason = "#193 will wire CompactionStrategy into the pipeline")
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
    expect(dead_code, reason = "#193 will wire CompactionStrategy into the pipeline")
)]
impl CompactionStrategy {
    /// Apply the strategy to a step sequence under a token budget.
    ///
    /// Returns a new sequence where steps may be compacted or dropped
    /// depending on the strategy and budget.
    #[must_use]
    pub fn apply(self, steps: &[Step], budget: usize) -> Vec<Step> {
        match self {
            Self::UniformTail => Self::apply_uniform_tail(steps, budget),
            Self::StepPositional => Self::apply_step_positional(steps, budget),
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

    /// Step-positional degradation: last 2 steps are kept full;
    /// all earlier steps are compacted to notes-only (`Step::compact`).
    ///
    /// WHY: tool results are the first thing to drop under budget pressure,
    /// but the assistant's reasoning (`self_note`) and any summary are retained
    /// so that decision history survives compaction. The last 2 steps keep
    /// full detail because they are the immediate context the model needs.
    fn apply_step_positional(steps: &[Step], budget: usize) -> Vec<Step> {
        let n = steps.len();
        let threshold = n.saturating_sub(2);
        let mut result = Vec::new();
        let mut used: usize = 0;

        for (idx, step) in steps.iter().enumerate().rev() {
            let compacted = if idx >= threshold {
                step.clone()
            } else {
                Step {
                    self_note: step.compact(),
                    observations: Vec::new(),
                    summary: None,
                    index: step.index,
                    started_at: step.started_at,
                }
            };

            let cost = compacted.token_estimate();
            let new_used = used.saturating_add(cost);
            if new_used > budget && !result.is_empty() {
                break;
            }
            result.push(compacted);
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
    fn step_positional_compacts_older_steps_keeps_last_two_full() {
        let steps: Vec<Step> = (0..5)
            .map(|i| Step {
                self_note: format!("note {i}"),
                observations: vec![crate::memory::step::Observation::new(
                    "tool",
                    "x".repeat(100),
                )],
                summary: Some(format!("summary {i}")),
                index: i,
                started_at: jiff::Timestamp::now(),
            })
            .collect();

        // Budget large enough to keep all compacted steps.
        let result = CompactionStrategy::StepPositional.apply(&steps, 200);
        assert_eq!(result.len(), 5, "should keep all steps under generous budget");

        // Last 2 steps retain observations.
        assert!(
            !result[3].observations.is_empty(),
            "step 3 (second-to-last) should keep observations"
        );
        assert!(
            !result[4].observations.is_empty(),
            "step 4 (last) should keep observations"
        );

        // Earlier steps are compacted (observations dropped).
        assert!(
            result[0].observations.is_empty(),
            "step 0 should be compacted (no observations)"
        );
        assert!(
            result[1].observations.is_empty(),
            "step 1 should be compacted (no observations)"
        );
        assert!(
            result[2].observations.is_empty(),
            "step 2 should be compacted (no observations)"
        );
    }

    #[test]
    fn step_positional_differs_from_uniform_tail() {
        let steps: Vec<Step> = (0..5)
            .map(|i| Step {
                self_note: format!("note {i}"),
                observations: vec![crate::memory::step::Observation::new(
                    "tool",
                    "x".repeat(100),
                )],
                summary: Some(format!("summary {i}")),
                index: i,
                started_at: jiff::Timestamp::now(),
            })
            .collect();

        // Budget = 80: UniformTail keeps 2 full steps (~60 tokens);
        // StepPositional keeps 2 full + 3 compacted (~75 tokens).
        let budget = 80;
        let uniform = CompactionStrategy::UniformTail.apply(&steps, budget);
        let positional = CompactionStrategy::StepPositional.apply(&steps, budget);

        assert_ne!(
            uniform.len(),
            positional.len(),
            "StepPositional should keep more steps than UniformTail for this budget"
        );
        assert_eq!(uniform.len(), 2, "UniformTail should keep last 2 full steps");
        assert_eq!(
            positional.len(),
            5,
            "StepPositional should keep all 5 steps (2 full + 3 compacted)"
        );
    }

    #[test]
    fn step_positional_respects_token_budget() {
        let steps: Vec<Step> = (0..5)
            .map(|i| Step {
                self_note: format!("note {i}"),
                observations: vec![crate::memory::step::Observation::new(
                    "tool",
                    "x".repeat(100),
                )],
                summary: None,
                index: i,
                started_at: jiff::Timestamp::now(),
            })
            .collect();

        let budget = 40;
        let result = CompactionStrategy::StepPositional.apply(&steps, budget);

        let total_tokens: usize = result.iter().map(Step::token_estimate).sum();
        assert!(
            total_tokens <= budget,
            "total tokens {total_tokens} should not exceed budget {budget}"
        );

        // Most recent step should always be present.
        assert_eq!(
            result.last().expect("result not empty").index,
            4,
            "most recent step should be preserved"
        );
    }
}
