//! Scenario runner: orchestrates evaluation execution.

use std::time::{Duration, Instant};

use tracing::{info, warn};

use aletheia_koina::secret::SecretString;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioOutcome, ScenarioResult};
use crate::scenarios;

/// Configuration for a scenario run.
pub struct RunConfig {
    /// Base URL of the target instance.
    pub base_url: String,
    /// Bearer token for authenticated endpoints.
    pub token: Option<SecretString>,
    /// Substring filter on scenario IDs.
    pub filter: Option<String>,
    /// Stop on first failure.
    pub fail_fast: bool,
    /// Per-scenario timeout in seconds.
    pub timeout_secs: u64,
    /// Emit JSON instead of formatted output.
    pub json_output: bool,
}

/// Aggregated results from a full eval run.
pub struct RunReport {
    /// Number of scenarios that passed.
    pub passed: usize,
    /// Number of scenarios that failed.
    pub failed: usize,
    /// Number of scenarios that were skipped.
    pub skipped: usize,
    /// Total wall-clock duration of the run.
    pub total_duration: Duration,
    /// Per-scenario results in run order.
    pub results: Vec<ScenarioResult>,
}

/// Runs behavioral scenarios against a live Aletheia instance.
pub struct ScenarioRunner {
    config: RunConfig,
    client: EvalClient,
}

impl ScenarioRunner {
    /// Create a new runner from the given configuration.
    #[must_use]
    pub fn new(config: RunConfig) -> Self {
        let client = EvalClient::new(
            &config.base_url,
            config.token.as_ref().map(|t| t.expose_secret().to_owned()),
        );
        Self { config, client }
    }

    /// Run all scenarios matching the configured filter.
    #[expect(clippy::too_many_lines, reason = "sequential scenario orchestration")]
    #[tracing::instrument(skip(self))]
    pub async fn run(&self) -> RunReport {
        let start = Instant::now();
        let all_scenarios = scenarios::all_scenarios();

        let scenarios: Vec<Box<dyn Scenario>> = match &self.config.filter {
            Some(filter) => all_scenarios
                .into_iter()
                .filter(|s| s.meta().id.contains(filter.as_str()))
                .collect(),
            None => all_scenarios,
        };

        let health = self.client.health().await.ok(); // WHY: best-effort; scenarios self-skip when server is unreachable
        let has_token = self.client.has_token();

        let has_nous = if has_token {
            self.client
                .list_nous()
                .await
                .ok()
                .is_some_and(|list| !list.is_empty())
        } else {
            false
        };

        info!(
            url = self.client.base_url(),
            reachable = health.is_some(),
            has_token,
            has_nous,
            scenario_count = scenarios.len(),
            "eval pre-flight complete"
        );

        let mut results = Vec::with_capacity(scenarios.len());
        let mut passed = 0_usize;
        let mut failed = 0_usize;
        let mut skipped = 0_usize;
        let mut fail_fast_idx: Option<usize> = None;

        for (i, scenario) in scenarios.iter().enumerate() {
            let meta = scenario.meta();

            if health.is_none() {
                results.push(ScenarioResult {
                    meta: meta.clone(),
                    outcome: ScenarioOutcome::Skipped {
                        reason: "instance unreachable".to_owned(),
                    },
                });
                skipped += 1;
                continue;
            }

            if meta.requires_auth && !has_token {
                results.push(ScenarioResult {
                    meta: meta.clone(),
                    outcome: ScenarioOutcome::Skipped {
                        reason: "no auth token provided".to_owned(),
                    },
                });
                skipped += 1;
                continue;
            }

            if meta.requires_nous && !has_nous {
                results.push(ScenarioResult {
                    meta: meta.clone(),
                    outcome: ScenarioOutcome::Skipped {
                        reason: "no nous agents configured".to_owned(),
                    },
                });
                skipped += 1;
                continue;
            }

            let scenario_start = Instant::now();
            let timeout = Duration::from_secs(self.config.timeout_secs);

            // WHY: tokio::select! drops the losing branch's future, cancelling
            // in-flight work (HTTP requests, retries) immediately on timeout
            // rather than letting them run to completion in the background.
            let scenario_fut = scenario.run(&self.client);
            tokio::pin!(scenario_fut);

            let outcome = tokio::select! {
                result = &mut scenario_fut => {
                    match result {
                        Ok(()) => {
                            let duration = scenario_start.elapsed();
                            info!(id = meta.id, ?duration, "scenario passed");
                            passed += 1;
                            ScenarioOutcome::Passed { duration }
                        }
                        Err(error) => {
                            let duration = scenario_start.elapsed();
                            warn!(id = meta.id, %error, "scenario failed");
                            failed += 1;
                            ScenarioOutcome::Failed { duration, error }
                        }
                    }
                }
                () = tokio::time::sleep(timeout) => {
                    // NOTE: scenario_fut goes out of scope here, cancelling any in-flight work
                    let duration = scenario_start.elapsed();
                    warn!(
                        id = meta.id,
                        timeout_secs = self.config.timeout_secs,
                        "scenario timed out, task cancelled"
                    );
                    failed += 1;
                    ScenarioOutcome::Failed {
                        duration,
                        error: crate::error::TimeoutSnafu {
                            elapsed_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
                        }
                        .build(),
                    }
                }
            };

            results.push(ScenarioResult { meta, outcome });

            if self.config.fail_fast && failed > 0 {
                fail_fast_idx = Some(i + 1);
                break;
            }
        }

        // WHY: when fail_fast triggers, mark remaining as skipped so passed + failed + skipped == total
        if let Some(remaining_start) = fail_fast_idx {
            #[expect(
                clippy::indexing_slicing,
                reason = "remaining_start is i+1 where i < scenarios.len(); slice is empty when i is last"
            )]
            for scenario in &scenarios[remaining_start..] {
                results.push(ScenarioResult {
                    meta: scenario.meta(),
                    outcome: ScenarioOutcome::Skipped {
                        reason: "fail_fast: earlier scenario failed".to_owned(),
                    },
                });
                skipped += 1;
            }
        }

        RunReport {
            passed,
            failed,
            skipped,
            total_duration: start.elapsed(),
            results,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_report_counts() {
        let report = RunReport {
            passed: 3,
            failed: 1,
            skipped: 2,
            total_duration: Duration::from_millis(500),
            results: vec![],
        };
        assert_eq!(report.passed, 3);
        assert_eq!(report.failed, 1);
        assert_eq!(report.skipped, 2);
    }

    #[test]
    fn scenario_outcome_is_passed() {
        let outcome = ScenarioOutcome::Passed {
            duration: Duration::from_millis(100),
        };
        assert!(outcome.is_passed());
        assert!(!outcome.is_failed());
    }

    #[test]
    fn scenario_outcome_is_failed() {
        let outcome = ScenarioOutcome::Failed {
            duration: Duration::from_millis(100),
            error: crate::error::AssertionSnafu {
                message: "test failure",
            }
            .build(),
        };
        assert!(outcome.is_failed());
        assert!(!outcome.is_passed());
    }

    #[test]
    fn scenario_outcome_skipped_not_passed_or_failed() {
        let outcome = ScenarioOutcome::Skipped {
            reason: "not applicable".to_owned(),
        };
        assert!(!outcome.is_passed());
        assert!(!outcome.is_failed());
    }

    #[test]
    fn run_report_total_count() {
        let report = RunReport {
            passed: 5,
            failed: 2,
            skipped: 3,
            total_duration: Duration::from_secs(1),
            results: vec![],
        };
        assert_eq!(report.passed + report.failed + report.skipped, 10);
    }

    #[test]
    fn scenario_meta_fields() {
        let meta = crate::scenario::ScenarioMeta {
            id: "test-scenario",
            description: "a test scenario",
            category: "unit",
            requires_auth: true,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
        };
        assert_eq!(meta.id, "test-scenario");
        assert_eq!(meta.description, "a test scenario");
        assert_eq!(meta.category, "unit");
        assert!(meta.requires_auth);
        assert!(!meta.requires_nous);
    }
}
