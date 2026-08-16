//! Reward helpers for memory-policy training experiments.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Benchmark outcome consumed by reward functions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MemoryOutcome {
    /// Exact-match rate in the inclusive range 0.0..=1.0.
    pub exact_match_rate: f64,
    /// Mean F1 score in the inclusive range 0.0..=1.0 when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_f1: Option<f64>,
}

/// Computes scalar reward from a benchmark outcome.
pub trait RewardFn {
    /// Return the scalar reward for an observed benchmark outcome.
    fn reward(&self, outcome: &MemoryOutcome) -> f64;
}

/// Reward function that scores improvement over a `LongMemEval` baseline.
///
/// Closes #4863 (reward-baseline slice): a scalar `baseline_exact_match_rate`
/// alone cannot explain *which* baseline artifact produced it or how much
/// data backed it, so a learned reward signal was not traceable to source
/// evidence. The provenance fields below are all populated from the same
/// JSON blob `from_json_file` already parses -- no new I/O.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LongMemEvalReward {
    /// Baseline exact-match rate to improve on.
    pub baseline_exact_match_rate: f64,
    /// Dataset identity, when the baseline file names one (`dataset_id` or
    /// falling back to `benchmark`, e.g. `"LongMemEval"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    /// Dataset content hash, when the baseline file carries one directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_hash: Option<String>,
    /// SHA-256 hex digest of the exact baseline report bytes this reward
    /// was built from -- computed locally, not read from the file, so it
    /// is always present and cannot drift from the source it describes.
    pub report_hash: String,
    /// Baseline benchmark run identifier, when the file names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Number of scored questions the baseline rate was computed over,
    /// when the file carries a `questions` array or an explicit
    /// `sample_size`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_size: Option<u64>,
    /// Confidence interval on `baseline_exact_match_rate`, when the file
    /// reports one (`ci: [lower, upper]` or `ci: {"lower":..,"upper":..}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci: Option<(f64, f64)>,
}

impl LongMemEvalReward {
    /// Build a reward from a compact baseline summary or full benchmark report.
    pub fn from_json_file(path: impl AsRef<Path>) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
        let exact_match_rate = extract_exact_match_rate(&value).ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidData,
                "baseline JSON must contain exact_match_rate or scored questions",
            )
        })?;
        Ok(Self {
            baseline_exact_match_rate: exact_match_rate,
            dataset_id: extract_dataset_id(&value),
            dataset_hash: extract_str_field(&value, "dataset_hash"),
            report_hash: sha256_hex(&text),
            run_id: extract_str_field(&value, "run_id"),
            sample_size: extract_sample_size(&value),
            ci: extract_ci(&value),
        })
    }
}

impl RewardFn for LongMemEvalReward {
    fn reward(&self, outcome: &MemoryOutcome) -> f64 {
        outcome.exact_match_rate - self.baseline_exact_match_rate
    }
}

fn extract_exact_match_rate(value: &serde_json::Value) -> Option<f64> {
    value
        .get("exact_match_rate")
        .and_then(serde_json::Value::as_f64)
        .or_else(|| exact_match_rate_from_questions(value))
}

fn extract_str_field(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

/// `dataset_id` if present, else the `benchmark` name (baseline files in
/// this codebase use `"benchmark": "LongMemEval"` rather than a distinct
/// `dataset_id` key -- see the module tests).
fn extract_dataset_id(value: &serde_json::Value) -> Option<String> {
    extract_str_field(value, "dataset_id").or_else(|| extract_str_field(value, "benchmark"))
}

#[expect(
    clippy::as_conversions,
    reason = "usize→u64: question counts fit u64 for any realistic benchmark file"
)]
fn extract_sample_size(value: &serde_json::Value) -> Option<u64> {
    value
        .get("questions")
        .and_then(serde_json::Value::as_array)
        .map(|q| q.len() as u64) // kanon:ignore RUST/as-cast
        .or_else(|| value.get("sample_size").and_then(serde_json::Value::as_u64))
}

fn extract_ci(value: &serde_json::Value) -> Option<(f64, f64)> {
    let ci = value.get("ci")?;
    if let Some(arr) = ci.as_array() {
        let lo = arr.first()?.as_f64()?;
        let hi = arr.get(1)?.as_f64()?;
        return Some((lo, hi));
    }
    let lo = ci.get("lower")?.as_f64()?;
    let hi = ci.get("upper")?.as_f64()?;
    Some((lo, hi))
}

fn sha256_hex(content: impl AsRef<[u8]>) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;

    let digest = sha2::Sha256::digest(content.as_ref());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // WHY discard the Result: writing hex digits into a String never
        // fails, and `expect_used` is denied crate-wide.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[expect(
    clippy::cast_precision_loss,
    reason = "benchmark question counts are small enough for exact f64 conversion"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 for bounded benchmark counts"
)]
fn exact_match_rate_from_questions(value: &serde_json::Value) -> Option<f64> {
    let questions = value.get("questions")?.as_array()?;
    if questions.is_empty() {
        return Some(0.0);
    }

    let hits = questions
        .iter()
        .filter(|question| {
            question
                .get("score")
                .and_then(|score| score.get("exact_match"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .count();

    // kanon:ignore RUST/as-cast — hits/questions.len() are usize; f64 conversion is fine for the [0.0,1.0] rate output (loss-of-precision irrelevant well under 2^53).
    // kanon:ignore RUST/as-cast — see above (second cast on the same expression).
    Some(hits as f64 / questions.len() as f64)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::fs;
    use std::io::Write as _;

    use tempfile::tempdir;

    use super::{LongMemEvalReward, MemoryOutcome, RewardFn, extract_exact_match_rate};

    #[test]
    fn loads_compact_baseline_summary() {
        let value = serde_json::json!({
            "benchmark": "LongMemEval",
            "exact_match_rate": 0.42
        });

        assert_eq!(extract_exact_match_rate(&value), Some(0.42));
    }

    #[test]
    fn computes_exact_match_from_full_report() {
        let value = serde_json::json!({
            "questions": [
                { "score": { "exact_match": true } },
                { "score": { "exact_match": false } },
                { "score": { "exact_match": true } }
            ]
        });

        assert_eq!(extract_exact_match_rate(&value), Some(2.0 / 3.0));
    }

    #[test]
    fn reward_is_delta_over_baseline() {
        let reward = LongMemEvalReward {
            baseline_exact_match_rate: 0.35,
            dataset_id: None,
            dataset_hash: None,
            report_hash: "test-hash".to_owned(),
            run_id: None,
            sample_size: None,
            ci: None,
        };
        let outcome = MemoryOutcome {
            exact_match_rate: 0.50,
            mean_f1: None,
        };

        assert!((reward.reward(&outcome) - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn loads_reward_from_file_and_scores_real_outcome() {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("baseline.json");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(
            serde_json::json!({
                "benchmark": "LongMemEval",
                "exact_match_rate": 0.35,
                "mean_f1": 0.42
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        let reward = LongMemEvalReward::from_json_file(&path).unwrap();
        let outcome = MemoryOutcome {
            exact_match_rate: 0.50,
            mean_f1: Some(0.40),
        };

        assert!((reward.reward(&outcome) - 0.15).abs() < f64::EPSILON);
        assert_eq!(reward.dataset_id.as_deref(), Some("LongMemEval"));
        assert!(!reward.report_hash.is_empty());
    }

    #[test]
    fn provenance_fields_populate_from_full_report() {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("baseline.json");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(
            serde_json::json!({
                "dataset_id": "longmemeval-s",
                "dataset_hash": "sha256:deadbeef",
                "run_id": "run-2026-08-14",
                "ci": [0.30, 0.40],
                "questions": [
                    { "score": { "exact_match": true } },
                    { "score": { "exact_match": false } }
                ]
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        let reward = LongMemEvalReward::from_json_file(&path).unwrap();
        assert_eq!(reward.dataset_id.as_deref(), Some("longmemeval-s"));
        assert_eq!(reward.dataset_hash.as_deref(), Some("sha256:deadbeef"));
        assert_eq!(reward.run_id.as_deref(), Some("run-2026-08-14"));
        assert_eq!(reward.sample_size, Some(2));
        assert_eq!(reward.ci, Some((0.30, 0.40)));
    }
}
