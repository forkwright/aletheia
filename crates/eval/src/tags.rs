//! Typed-tag namespace over RunReport for SFT/distillation pipeline (Phase 10).
//!
//! Pure function: `tag_eval_result(report) -> Vec<TagId>`. Set-membership filtering
//! is then a `HashSet<TagId>` intersection — no JSON re-parsing per query.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::runner::RunReport;
use crate::scenario::ScenarioOutcome;

/// A typed tag identifier derived from a [`RunReport`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TagId {
    /// A scenario category present in the run.
    Category {
        /// The category name (e.g. "health", "session").
        name: String,
    },
    /// An outcome class present among the results.
    Outcome(OutcomeTag),
    /// The run contains at least one scenario requiring authentication.
    RequiresAuth,
    /// The run contains at least one scenario requiring a nous agent.
    RequiresNous,
    /// The run contains at least one scenario with validation criteria.
    HasCriteria,
    /// Wall-clock duration band for the entire run.
    DurationBand(DurationBand),
    /// Number-of-scenarios band for the run.
    SizeBand(SizeBand),
}

/// Outcome classes that can appear in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OutcomeTag {
    /// At least one scenario passed.
    Passed,
    /// At least one scenario failed.
    Failed,
    /// At least one scenario was skipped.
    Skipped,
}

/// Duration bands for run wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DurationBand {
    /// Under 1 second.
    Low,
    /// 1 second to 1 minute.
    Medium,
    /// Over 1 minute.
    High,
}

/// Size bands for the number of scenarios in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SizeBand {
    /// No scenarios in the run.
    Empty,
    /// A single scenario.
    Single,
    /// 2–5 scenarios.
    Small,
    /// 6–20 scenarios.
    Medium,
    /// More than 20 scenarios.
    Large,
}

/// Derive a sorted, deduplicated set of typed tags from an evaluation run report.
///
/// The returned vector is deterministic: equivalent reports always produce the
/// same tag ordering so that serializations are bitwise-identical.
#[must_use]
pub fn tag_eval_result(report: &RunReport) -> Vec<TagId> {
    let mut tags = HashSet::new();

    let total = report.results.len();
    let size_band = match total {
        0 => SizeBand::Empty,
        1 => SizeBand::Single,
        2..=5 => SizeBand::Small,
        6..=20 => SizeBand::Medium,
        _ => SizeBand::Large,
    };
    tags.insert(TagId::SizeBand(size_band));

    tags.insert(TagId::DurationBand(duration_band(&report.total_duration)));

    if total == 0 {
        return into_sorted_vec(tags);
    }

    let mut has_passed = false;
    let mut has_failed = false;
    let mut has_skipped = false;
    let mut categories = HashSet::new();

    for result in &report.results {
        categories.insert(result.meta.category.to_owned());

        match &result.outcome {
            ScenarioOutcome::Passed { .. } => has_passed = true,
            ScenarioOutcome::Failed { .. } => has_failed = true,
            ScenarioOutcome::Skipped { .. } => has_skipped = true,
        }

        if result.meta.requires_auth {
            tags.insert(TagId::RequiresAuth);
        }
        if result.meta.requires_nous {
            tags.insert(TagId::RequiresNous);
        }
        if result.meta.expected_contains.is_some() || result.meta.expected_pattern.is_some() {
            tags.insert(TagId::HasCriteria);
        }
    }

    if has_passed {
        tags.insert(TagId::Outcome(OutcomeTag::Passed));
    }
    if has_failed {
        tags.insert(TagId::Outcome(OutcomeTag::Failed));
    }
    if has_skipped {
        tags.insert(TagId::Outcome(OutcomeTag::Skipped));
    }

    for category in categories {
        tags.insert(TagId::Category { name: category });
    }

    into_sorted_vec(tags)
}

fn duration_band(duration: &std::time::Duration) -> DurationBand {
    let secs = duration.as_secs();
    if secs < 1 {
        DurationBand::Low
    } else if secs < 60 {
        DurationBand::Medium
    } else {
        DurationBand::High
    }
}

fn into_sorted_vec(tags: HashSet<TagId>) -> Vec<TagId> {
    let mut vec: Vec<TagId> = tags.into_iter().collect();
    vec.sort_unstable();
    vec
}
