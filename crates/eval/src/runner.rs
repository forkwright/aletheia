//! Scenario runner: orchestrates evaluation execution.

use std::time::{Duration, Instant};

use mneme::meta::{ArtefactMeta, Stamped};
use tracing::{info, warn};

use koina::secret::SecretString;

use crate::client::EvalClient;
use crate::coverage::{
    SKIP_REASON_FAIL_FAST, SKIP_REASON_INSTANCE_UNREACHABLE, SKIP_REASON_NO_AUTH_TOKEN,
    SKIP_REASON_NO_NOUS_AGENTS,
};
use crate::persistence::now_iso8601;
use crate::provenance::EvalProvenance;
use crate::provider::{BuiltinProvider, EvalProvider};
use crate::scenario::{Scenario, ScenarioOutcome, ScenarioResult, ScenarioRunOutcome};

/// Configuration for a scenario run.
pub struct RunConfig {
    /// Base URL of the target instance.
    pub base_url: String,
    /// Bearer token for authenticated endpoints.
    pub token: Option<SecretString>,
    /// Substring filter on scenario IDs.
    pub filter: Option<String>,
    /// Exact-match filter on scenario category. When set, only scenarios with
    /// `meta().category == this` are run. Useful for tests that want to run
    /// "session" CRUD scenarios without also pulling in `canary-session`
    /// scenarios that share a substring with the id-based filter.
    pub category_filter: Option<String>,
    /// Stop on first failure.
    pub fail_fast: bool,
    /// Per-scenario timeout in seconds.
    pub timeout_secs: u64,
    /// Emit JSON instead of formatted output.
    pub json_output: bool,
    /// Durable provenance envelope for this run.
    pub provenance: EvalProvenance,
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
    /// Durable provenance envelope for this run.
    pub provenance: EvalProvenance,
}

impl Stamped for RunReport {
    /// Returns provenance metadata for this eval run report.
    ///
    /// `row_counts` carries `"passed"`, `"failed"`, `"skipped"`, and
    /// `"total"` scenario counts. `generated_at` is the stamp time, not the
    /// run start time (use `total_duration` for timing).
    fn stamp(&self) -> ArtefactMeta {
        let total = u64::try_from(self.passed + self.failed + self.skipped).unwrap_or(u64::MAX);
        ArtefactMeta::new(
            concat!("dokimion@", env!("CARGO_PKG_VERSION")),
            1,
            now_iso8601(),
        )
        .with_count("passed", u64::try_from(self.passed).unwrap_or(u64::MAX))
        .with_count("failed", u64::try_from(self.failed).unwrap_or(u64::MAX))
        .with_count("skipped", u64::try_from(self.skipped).unwrap_or(u64::MAX))
        .with_count("total", total)
        .with_evidence(self.provenance.tool_ref.iter().map(String::as_str))
    }
}

/// Runs behavioral scenarios against a live Aletheia instance.
pub struct ScenarioRunner {
    config: RunConfig,
    client: EvalClient,
    provider: Box<dyn EvalProvider>,
}

impl ScenarioRunner {
    /// Create a new runner with the built-in scenario provider.
    #[must_use]
    pub fn new(config: RunConfig) -> Self {
        Self::with_provider(config, Box::new(BuiltinProvider))
    }

    /// Create a runner with a custom scenario provider.
    ///
    /// Use this to supply canary suites, phase gate checks, or composed
    /// scenario sets from multiple providers.
    #[must_use]
    pub fn with_provider(config: RunConfig, provider: Box<dyn EvalProvider>) -> Self {
        let client = EvalClient::new(
            &config.base_url,
            config.token.as_ref().map(|t| t.expose_secret().to_owned()),
        );
        Self {
            config,
            client,
            provider,
        }
    }

    /// Run all scenarios matching the configured filter.
    #[expect(clippy::too_many_lines, reason = "sequential scenario orchestration")]
    #[tracing::instrument(skip(self), fields(provider = self.provider.name()))]
    pub async fn run(&self) -> RunReport {
        let start = Instant::now();
        let all_scenarios = self.provider.provide();

        let scenarios: Vec<Box<dyn Scenario>> = all_scenarios
            .into_iter()
            .filter(|s| {
                let meta = s.meta();
                self.config
                    .filter
                    .as_deref()
                    .is_none_or(|f| meta.id.contains(f))
                    && self
                        .config
                        .category_filter
                        .as_deref()
                        .is_none_or(|c| meta.category == c)
            })
            .collect();

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
                        reason: SKIP_REASON_INSTANCE_UNREACHABLE.to_owned(),
                    },
                    sub_results: Vec::new(),
                });
                skipped += 1;
                continue;
            }

            if meta.requires_auth && !has_token {
                results.push(ScenarioResult {
                    meta: meta.clone(),
                    outcome: ScenarioOutcome::Skipped {
                        reason: SKIP_REASON_NO_AUTH_TOKEN.to_owned(),
                    },
                    sub_results: Vec::new(),
                });
                skipped += 1;
                continue;
            }

            if meta.requires_nous && !has_nous {
                results.push(ScenarioResult {
                    meta: meta.clone(),
                    outcome: ScenarioOutcome::Skipped {
                        reason: SKIP_REASON_NO_NOUS_AGENTS.to_owned(),
                    },
                    sub_results: Vec::new(),
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

            let ScenarioRunOutcome {
                result,
                sub_results,
            } = tokio::select! {
                outcome = &mut scenario_fut => outcome,
                () = tokio::time::sleep(timeout) => {
                    // NOTE: scenario_fut goes out of scope here, cancelling any in-flight work
                    let duration = scenario_start.elapsed();
                    warn!(
                        id = meta.id,
                        timeout_secs = self.config.timeout_secs,
                        "scenario timed out, task cancelled"
                    );
                    ScenarioRunOutcome {
                        result: Err(crate::error::TimeoutSnafu {
                            elapsed_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
                        }
                        .build()),
                        sub_results: Vec::new(),
                    }
                }
            };

            let outcome = match result {
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
            };

            results.push(ScenarioResult {
                meta,
                outcome,
                sub_results,
            });

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
                        reason: SKIP_REASON_FAIL_FAST.to_owned(),
                    },
                    sub_results: Vec::new(),
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
            provenance: self.config.provenance.clone().finished(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_provenance() -> EvalProvenance {
        EvalProvenance::new("er-test", "http://localhost")
    }

    #[test]
    fn run_report_counts() {
        let report = RunReport {
            passed: 3,
            failed: 1,
            skipped: 2,
            total_duration: Duration::from_millis(500),
            results: vec![],
            provenance: empty_provenance(),
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
            provenance: empty_provenance(),
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
            classification: crate::scenario::ScenarioClassification::Assertive,
        };
        assert_eq!(meta.id, "test-scenario");
        assert_eq!(meta.description, "a test scenario");
        assert_eq!(meta.category, "unit");
        assert!(meta.requires_auth);
        assert!(!meta.requires_nous);
    }

    fn sample_report() -> RunReport {
        RunReport {
            passed: 3,
            failed: 1,
            skipped: 2,
            total_duration: Duration::from_millis(500),
            results: vec![],
            provenance: empty_provenance(),
        }
    }

    #[test]
    fn run_report_stamp_producer_prefix() {
        let report = sample_report();
        let meta = report.stamp();
        assert!(
            meta.producer.starts_with("dokimion@"),
            "producer must start with 'dokimion@', got: {}",
            meta.producer
        );
    }

    #[test]
    fn run_report_stamp_schema_version() {
        let report = sample_report();
        let meta = report.stamp();
        assert_eq!(meta.schema_version, 1, "schema_version must be 1");
    }

    #[test]
    fn run_report_stamp_row_counts() {
        let report = sample_report();
        let meta = report.stamp();
        assert_eq!(
            meta.row_counts.get("passed").copied(),
            Some(3),
            "passed count should match"
        );
        assert_eq!(
            meta.row_counts.get("failed").copied(),
            Some(1),
            "failed count should match"
        );
        assert_eq!(
            meta.row_counts.get("skipped").copied(),
            Some(2),
            "skipped count should match"
        );
        assert_eq!(
            meta.row_counts.get("total").copied(),
            Some(6),
            "total should be passed + failed + skipped"
        );
    }

    #[test]
    fn run_report_stamp_carries_tool_ref_evidence() {
        let mut report = sample_report();
        report.provenance =
            report
                .provenance
                .with_audit_refs(None, None, None, Some("ts1:test".to_owned()), None);
        let meta = report.stamp();
        assert_eq!(meta.evidence_refs, vec!["ts1:test"]);
    }

    #[test]
    fn run_report_carries_provenance() {
        let provenance = EvalProvenance::new("er-123", "http://example.com");
        let report = RunReport {
            passed: 0,
            failed: 0,
            skipped: 0,
            total_duration: Duration::from_secs(0),
            results: vec![],
            provenance: provenance.clone(),
        };
        assert_eq!(report.provenance.eval_run_id, "er-123");
        assert!(report.provenance.finished_at.is_none());
    }

    #[test]
    fn run_report_finished_sets_finished_at() {
        let provenance = EvalProvenance::new("er-123", "http://example.com");
        let finished = provenance.finished();
        assert!(finished.finished_at.is_some());
    }
}
