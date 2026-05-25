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

/// Reward function that scores improvement over a LongMemEval baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LongMemEvalReward {
    /// Baseline exact-match rate to improve on.
    pub baseline_exact_match_rate: f64,
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

    Some(hits as f64 / questions.len() as f64)
}

#[cfg(test)]
mod tests {
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
        };
        let outcome = MemoryOutcome {
            exact_match_rate: 0.50,
            mean_f1: None,
        };

        assert!((reward.reward(&outcome) - 0.15).abs() < f64::EPSILON);
    }
}
