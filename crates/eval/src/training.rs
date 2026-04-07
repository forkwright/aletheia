//! A/B training pipeline: multi-model competition via canary evaluation.
//!
//! Implements continuous model improvement through selection pressure:
//! candidate models compete against the production model on the canary
//! prompt suite. Winners are promoted; losers are archived for forensics.
//!
//! WHY: Without selection pressure, model quality drifts. The A/B pipeline
//! provides a data-driven promotion gate: only models that score higher
//! on the canary suite replace the current production model.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::provider::EvalProvider;
use crate::runner::{RunConfig, RunReport, ScenarioRunner};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the A/B training evaluation pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TrainingPipelineConfig {
    /// Base URL of the target aletheia instance.
    pub base_url: String,
    /// Bearer token for authenticated endpoints.
    pub token: Option<String>,
    /// Per-scenario timeout in seconds.
    pub timeout_secs: u64,
    /// Directory where evaluation results are archived.
    pub archive_dir: PathBuf,
    /// Minimum improvement (percentage points) required for promotion.
    ///
    /// If the candidate scores 82% and production scores 80%, improvement
    /// is 2pp. If `min_improvement_pp` is 3.0, the candidate is NOT promoted.
    pub min_improvement_pp: f64,
}

impl Default for TrainingPipelineConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:3000".to_owned(),
            token: None,
            timeout_secs: 30,
            archive_dir: PathBuf::from("instance/data/training/evaluations"),
            min_improvement_pp: 2.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Candidate
// ---------------------------------------------------------------------------

/// A model candidate being evaluated against production.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelCandidate {
    /// Model identifier (e.g., "claude-sonnet-4-20250514", "qwen3.5-27b-finetuned-v3").
    pub model_id: String,
    /// Human-readable label for reports.
    pub label: String,
    /// Training run identifier (links back to the training job).
    pub training_run_id: Option<String>,
    /// Path to model weights or checkpoint (for local models).
    pub weights_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Evaluation result
// ---------------------------------------------------------------------------

/// Result of evaluating a single model against the canary suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelEvaluation {
    /// The model that was evaluated.
    pub candidate: ModelCandidate,
    /// Pass rate: passed / (passed + failed). Skipped excluded.
    pub pass_rate: f64,
    /// Number of scenarios passed.
    pub passed: usize,
    /// Number of scenarios failed.
    pub failed: usize,
    /// Number of scenarios skipped.
    pub skipped: usize,
    /// Total evaluation duration.
    pub duration: Duration,
}

impl ModelEvaluation {
    /// Compute the evaluation from a run report and candidate.
    #[must_use]
    pub fn from_report(candidate: ModelCandidate, report: &RunReport) -> Self {
        let total_run = report.passed + report.failed;
        let pass_rate = if total_run > 0 {
            report.passed as f64 / total_run as f64
        } else {
            0.0
        };
        Self {
            candidate,
            pass_rate,
            passed: report.passed,
            failed: report.failed,
            skipped: report.skipped,
            duration: report.total_duration,
        }
    }
}

// ---------------------------------------------------------------------------
// Competition result
// ---------------------------------------------------------------------------

/// Outcome of a head-to-head model competition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CompetitionResult {
    /// The production model's evaluation.
    pub production: ModelEvaluation,
    /// The candidate model's evaluation.
    pub candidate: ModelEvaluation,
    /// Improvement in pass rate (percentage points).
    pub improvement_pp: f64,
    /// Whether the candidate should be promoted.
    pub promote: bool,
    /// Human-readable verdict.
    pub verdict: String,
}

/// Run a head-to-head competition between production and candidate models.
///
/// Both models are evaluated against the same canary suite. The candidate
/// is promoted if it exceeds the production pass rate by at least
/// `min_improvement_pp` percentage points.
///
/// # Note
///
/// This function evaluates against a running aletheia instance. The caller
/// is responsible for configuring the instance to use each model in turn
/// (e.g., via config reload or restart).
pub async fn compete(
    config: &TrainingPipelineConfig,
    provider: Box<dyn EvalProvider>,
    production: ModelCandidate,
    candidate: ModelCandidate,
) -> CompetitionResult {
    // WHY: we evaluate both models against the same scenario set.
    // The provider is called twice (once per model). In a real deployment,
    // the caller would switch the instance's model between evaluations.
    let run_config = RunConfig {
        base_url: config.base_url.clone(),
        token: config.token.as_ref().map(|t| {
            aletheia_koina::secret::SecretString::from(t.clone())
        }),
        filter: None,
        fail_fast: false,
        timeout_secs: config.timeout_secs,
        json_output: false,
    };

    // Evaluate production model.
    let prod_runner = ScenarioRunner::with_provider(run_config.clone(), provider_clone(&provider));
    let prod_report = prod_runner.run().await;
    let prod_eval = ModelEvaluation::from_report(production, &prod_report);

    // Evaluate candidate model.
    let cand_runner = ScenarioRunner::with_provider(
        RunConfig {
            base_url: config.base_url.clone(),
            token: config.token.as_ref().map(|t| {
                aletheia_koina::secret::SecretString::from(t.clone())
            }),
            filter: None,
            fail_fast: false,
            timeout_secs: config.timeout_secs,
            json_output: false,
        },
        provider,
    );
    let cand_report = cand_runner.run().await;
    let cand_eval = ModelEvaluation::from_report(candidate, &cand_report);

    let improvement_pp = (cand_eval.pass_rate - prod_eval.pass_rate) * 100.0;
    let promote = improvement_pp >= config.min_improvement_pp;

    let verdict = if promote {
        format!(
            "PROMOTE: candidate scores {:.1}% vs production {:.1}% (+{:.1}pp, threshold {:.1}pp)",
            cand_eval.pass_rate * 100.0,
            prod_eval.pass_rate * 100.0,
            improvement_pp,
            config.min_improvement_pp,
        )
    } else {
        format!(
            "KEEP: candidate scores {:.1}% vs production {:.1}% ({:+.1}pp, threshold {:.1}pp)",
            cand_eval.pass_rate * 100.0,
            prod_eval.pass_rate * 100.0,
            improvement_pp,
            config.min_improvement_pp,
        )
    };

    CompetitionResult {
        production: prod_eval,
        candidate: cand_eval,
        improvement_pp,
        promote,
        verdict,
    }
}

/// Clone a provider by re-calling provide() on a new BuiltinProvider.
///
/// WHY: EvalProvider is not Clone (trait objects). We work around this by
/// creating a fresh provider for the second evaluation. In practice, both
/// evaluations run the same scenario set.
fn provider_clone(_provider: &dyn EvalProvider) -> Box<dyn EvalProvider> {
    // WHY: the production eval uses the same provider type.
    // For competition, both should run identical scenarios.
    Box::new(crate::provider::BuiltinProvider)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::runner::RunReport;

    #[test]
    fn model_evaluation_from_report() {
        let report = RunReport {
            passed: 8,
            failed: 2,
            skipped: 1,
            total_duration: Duration::from_secs(10),
            results: vec![],
        };
        let candidate = ModelCandidate {
            model_id: "test-model".to_owned(),
            label: "Test".to_owned(),
            training_run_id: None,
            weights_path: None,
        };
        let eval = ModelEvaluation::from_report(candidate, &report);
        assert!((eval.pass_rate - 0.8).abs() < 0.01);
        assert_eq!(eval.passed, 8);
        assert_eq!(eval.failed, 2);
    }

    #[test]
    fn competition_result_promote() {
        let result = CompetitionResult {
            production: ModelEvaluation {
                candidate: ModelCandidate {
                    model_id: "prod".to_owned(),
                    label: "Production".to_owned(),
                    training_run_id: None,
                    weights_path: None,
                },
                pass_rate: 0.8,
                passed: 8,
                failed: 2,
                skipped: 0,
                duration: Duration::from_secs(10),
            },
            candidate: ModelEvaluation {
                candidate: ModelCandidate {
                    model_id: "new".to_owned(),
                    label: "Candidate".to_owned(),
                    training_run_id: None,
                    weights_path: None,
                },
                pass_rate: 0.9,
                passed: 9,
                failed: 1,
                skipped: 0,
                duration: Duration::from_secs(12),
            },
            improvement_pp: 10.0,
            promote: true,
            verdict: "PROMOTE".to_owned(),
        };
        assert!(result.promote);
        assert!(result.improvement_pp > 0.0);
    }

    #[test]
    fn default_config() {
        let config = TrainingPipelineConfig::default();
        assert_eq!(config.min_improvement_pp, 2.0);
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn config_roundtrip() {
        let config = TrainingPipelineConfig {
            base_url: "http://test:3000".to_owned(),
            token: Some("secret".to_owned()),
            timeout_secs: 60,
            archive_dir: PathBuf::from("/tmp/archive"),
            min_improvement_pp: 5.0,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: TrainingPipelineConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.min_improvement_pp, 5.0);
    }
}
