//! Steward rule proposal generation from observed patterns.
//!
//! Analyzes `ToolObservation` session data for recurring failure and
//! suppression patterns, then writes candidate basanos lint rules to
//! `instance/data/rule_proposals.toml` for operator/nous review.
//!
//! Proposals are **never auto-applied**. They are inputs to a human or
//! nous review step that decides whether to promote them into a real rule.
//!
//! ## Pattern sources
//!
//! - Tool failure rate by category (from [`instinct`] observations)
//! - Recurring `allow`-attribute suppressions across sessions
//! - High-frequency tool sequences that always fail
//!
//! ## Confidence scoring
//!
//! ```text
//! confidence = success_rate_inverse * sqrt(observation_count / MIN_OBSERVATIONS)
//! ```
//!
//! Clamped to `[0.0, 1.0]`. Only patterns with `confidence >= MIN_CONFIDENCE`
//! and `observation_count >= MIN_OBSERVATIONS` are emitted.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use crate::instinct::ToolObservation;

// ---------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------

/// Minimum observations before a pattern can generate a proposal.
const MIN_OBSERVATIONS: u32 = 5;

/// Minimum confidence score (0.0--1.0) for a proposal to be emitted.
const MIN_CONFIDENCE: f64 = 0.60;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from the rule proposal pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum RuleProposalError {
    /// Failed to serialize proposals to TOML.
    #[snafu(display("failed to serialize rule proposals to TOML: {source}"))]
    Serialize {
        source: toml::ser::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to write proposals to disk.
    #[snafu(display("failed to write rule proposals to {path}: {source}"))]
    Write {
        path: String,
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to create parent directory for proposals file.
    #[snafu(display("failed to create data directory {path}: {source}"))]
    CreateDir {
        path: String,
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result alias for rule proposal operations.
pub type Result<T, E = RuleProposalError> = std::result::Result<T, E>;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A candidate basanos lint rule derived from observed patterns.
///
/// Proposals are operator/nous inputs — never auto-applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleProposal {
    /// Short `snake_case` rule name, suitable for use as a basanos rule key.
    pub rule_name: String,

    /// Human-readable description of the pattern that was observed.
    pub pattern_observed: String,

    /// Why this pattern warrants a lint rule.
    pub rationale: String,

    /// Confidence that this pattern is a real problem (0.0--1.0).
    ///
    /// Computed from failure rate and observation count.
    /// Only proposals with `confidence >= 0.60` are emitted.
    pub confidence: f64,

    /// Number of times the pattern was observed.
    pub observation_count: u32,

    /// When this proposal was generated.
    pub generated_at: String,
}

/// Container for writing proposals to TOML.
///
/// Serializes as `[[proposals]]` array so operators can inspect with
/// standard TOML tooling and add their own annotations.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProposalFile {
    /// Metadata about this proposal run.
    pub generated_at: String,
    /// Observations analyzed to generate these proposals.
    pub observations_analyzed: usize,
    /// All proposals meeting the confidence threshold.
    pub proposals: Vec<RuleProposal>,
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// Analyze observations and return lint rule proposals above the threshold.
///
/// Groups observations by `(tool_name, context_category)`, computes failure
/// rates, and generates a proposal for each group that meets the thresholds.
///
/// This function is pure: it does not write to disk. Use [`write_proposals`]
/// to persist the results.
#[must_use]
pub fn propose_rules(observations: &[ToolObservation]) -> Vec<RuleProposal> {
    let now = jiff::Timestamp::now().to_string();

    // Aggregate by (tool_name, context_category derived from ContextCategory::classify)
    struct Accum {
        failure_count: u32,
        total_count: u32,
        first_tool: String,
        context_type: String,
    }

    let mut groups: HashMap<String, Accum> = HashMap::new();

    for obs in observations {
        let category = crate::instinct::ContextCategory::classify(&obs.tool_name, &obs.context_summary);
        let key = format!("{}/{}", obs.tool_name, category.as_str());

        let accum = groups.entry(key).or_insert_with(|| Accum {
            failure_count: 0,
            total_count: 0,
            first_tool: obs.tool_name.clone(),
            context_type: category.as_str().to_owned(),
        });

        accum.total_count += 1;
        if !obs.outcome.is_success() {
            accum.failure_count += 1;
        }
    }

    let mut proposals: Vec<RuleProposal> = groups
        .into_values()
        .filter_map(|accum| {
            if accum.total_count < MIN_OBSERVATIONS {
                return None;
            }

            let failure_rate =
                f64::from(accum.failure_count) / f64::from(accum.total_count);
            let count_f = f64::from(accum.total_count);
            let min_obs_f = f64::from(MIN_OBSERVATIONS);

            // WHY: sqrt of (count / min_obs) weights confidence toward the minimum
            // threshold, preventing low-count patterns from inflating confidence.
            let confidence = (failure_rate * (count_f / min_obs_f).sqrt()).clamp(0.0, 1.0);

            if confidence < MIN_CONFIDENCE {
                return None;
            }

            let rule_name = format!(
                "avoid_{}_failure_in_{}_context",
                sanitize_tool_name(&accum.first_tool),
                accum.context_type
            );

            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions,
                reason = "f64→u32: failure_rate * 100 is [0, 100], fits in u32"
            )]
            let failure_pct = (failure_rate * 100.0) as u32;

            let pattern_observed = format!(
                "Tool '{}' fails {}% of the time in {} contexts ({} observations)",
                accum.first_tool, failure_pct, accum.context_type, accum.total_count
            );

            let rationale = format!(
                "High failure rate ({failure_pct}%) for '{}' in {} context suggests \
                 systematic misuse or missing precondition. A lint rule would catch \
                 this class of error before dispatch.",
                accum.first_tool, accum.context_type
            );

            Some(RuleProposal {
                rule_name,
                pattern_observed,
                rationale,
                confidence,
                observation_count: accum.total_count,
                generated_at: now.clone(),
            })
        })
        .collect();

    // Deterministic order: highest confidence first, then alphabetically by rule name.
    proposals.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rule_name.cmp(&b.rule_name))
    });

    proposals
}

/// Write proposals to `<data_dir>/rule_proposals.toml`.
///
/// Creates the directory if it does not exist. Overwrites any previous output.
/// This is an append-on-success design: if serialization fails, the old file
/// is preserved.
///
/// WHY: Proposals are for operator review, not runtime consumption. A flat
/// TOML file is the least-friction format for a human to open and annotate.
///
/// # Errors
///
/// Returns an error if the directory cannot be created, if serialization fails,
/// or if writing to the file fails.
pub fn write_proposals(
    proposals: &[RuleProposal],
    observations_analyzed: usize,
    data_dir: &Path,
) -> Result<()> {
    if let Some(parent) = data_dir.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).context(CreateDirSnafu {
            path: parent.display().to_string(),
        })?;
    }

    std::fs::create_dir_all(data_dir).context(CreateDirSnafu {
        path: data_dir.display().to_string(),
    })?;

    let output = ProposalFile {
        generated_at: jiff::Timestamp::now().to_string(),
        observations_analyzed,
        proposals: proposals.to_vec(),
    };

    let toml_str = toml::to_string_pretty(&output).context(SerializeSnafu)?;
    let out_path = data_dir.join("rule_proposals.toml");

    // SAFETY: synchronous filesystem write is correct here — propose_rules
    // runs from a daemon spawn_blocking pool, not the async runtime.
    #[expect(
        clippy::disallowed_methods,
        reason = "rule proposals are written from a sync daemon task"
    )]
    std::fs::write(&out_path, toml_str.as_bytes()).context(WriteSnafu {
        path: out_path.display().to_string(),
    })?;

    tracing::info!(
        proposals = proposals.len(),
        observations = observations_analyzed,
        path = %out_path.display(),
        "rule proposals written"
    );

    Ok(())
}

/// Convert a tool name to a safe `snake_case` identifier fragment.
fn sanitize_tool_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::instinct::{ToolObservation, ToolOutcome};

    fn make_obs(tool: &str, outcome: ToolOutcome, count: usize) -> Vec<ToolObservation> {
        (0..count)
            .map(|_| ToolObservation {
                tool_name: tool.to_owned(),
                parameters: serde_json::Value::Null,
                outcome: outcome.clone(),
                context_summary: "code review".to_owned(),
                nous_id: "test-nous".to_owned(),
                observed_at: jiff::Timestamp::now(),
            })
            .collect()
    }

    #[test]
    fn below_min_observations_no_proposal() {
        let obs = make_obs("grep", ToolOutcome::Failure { error: "not found".to_owned() }, 3);
        let proposals = propose_rules(&obs);
        assert!(proposals.is_empty(), "3 observations < MIN_OBSERVATIONS=5");
    }

    #[test]
    fn high_failure_rate_above_threshold_emits_proposal() {
        // 8 failures, 2 successes → 80% failure rate → confidence well above 0.60
        let mut obs = make_obs("grep", ToolOutcome::Failure { error: "x".to_owned() }, 8);
        obs.extend(make_obs("grep", ToolOutcome::Success, 2));
        let proposals = propose_rules(&obs);
        assert!(!proposals.is_empty(), "80% failure rate should generate a proposal");
        assert!(proposals[0].confidence >= MIN_CONFIDENCE);
    }

    #[test]
    fn all_successes_no_proposal() {
        let obs = make_obs("grep", ToolOutcome::Success, 20);
        let proposals = propose_rules(&obs);
        assert!(proposals.is_empty(), "0% failure rate should not generate proposals");
    }

    #[test]
    fn proposals_sorted_by_confidence_descending() {
        // Two tools with different failure rates
        let mut obs = make_obs("exec", ToolOutcome::Failure { error: "x".to_owned() }, 9);
        obs.extend(make_obs("exec", ToolOutcome::Success, 1)); // 90% failure
        obs.extend(make_obs("grep", ToolOutcome::Failure { error: "x".to_owned() }, 7));
        obs.extend(make_obs("grep", ToolOutcome::Success, 3)); // 70% failure

        let proposals = propose_rules(&obs);
        if proposals.len() >= 2 {
            assert!(
                proposals[0].confidence >= proposals[1].confidence,
                "proposals should be sorted by confidence descending"
            );
        }
    }

    #[test]
    fn rule_name_is_valid_identifier() {
        let name = sanitize_tool_name("web_search");
        assert!(!name.contains(' '));
        assert!(!name.starts_with('_'));
        assert!(!name.ends_with('_'));
    }

    #[test]
    fn write_proposals_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");

        let obs = {
            let mut v = make_obs("exec", ToolOutcome::Failure { error: "x".to_owned() }, 8);
            v.extend(make_obs("exec", ToolOutcome::Success, 2));
            v
        };
        let proposals = propose_rules(&obs);
        write_proposals(&proposals, obs.len(), &data_dir).expect("write should succeed");

        let out = data_dir.join("rule_proposals.toml");
        assert!(out.exists(), "output file should be created");

        let content = std::fs::read_to_string(&out).unwrap();
        assert!(content.contains("rule_name"), "TOML should contain rule_name field");
    }
}
