//! Coverage policy gates for eval scenario reports.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::runner::RunReport;
use crate::scenario::{ScenarioClassification, ScenarioMeta, ScenarioOutcome};

/// Skip reason emitted when the target instance cannot be reached.
pub const SKIP_REASON_INSTANCE_UNREACHABLE: &str = "instance unreachable";
/// Skip reason emitted when a scenario needs auth but no token was supplied.
pub const SKIP_REASON_NO_AUTH_TOKEN: &str = "no auth token provided";
/// Skip reason emitted when a scenario needs a nous but none is configured.
pub const SKIP_REASON_NO_NOUS_AGENTS: &str = "no nous agents configured";
/// Skip reason emitted for scenarios not run after fail-fast trips.
pub const SKIP_REASON_FAIL_FAST: &str = "fail_fast: earlier scenario failed";

/// Named eval coverage gate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Policy {
    /// Local exploratory policy: report coverage but allow skipped scenarios.
    SmokeDev,
    /// CI policy: every required selected scenario must run.
    #[default]
    Ci,
    /// Release policy: every selected scenario must run.
    Release,
    /// Publishable benchmark policy: every selected scenario must run.
    PublishableBenchmark,
}

impl Policy {
    /// Evaluate a run report against this policy.
    #[must_use]
    pub fn evaluate(self, report: &RunReport) -> Summary {
        Summary::from_report(self, report)
    }

    /// Canonical CLI/config spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SmokeDev => "smoke-dev",
            Self::Ci => "ci",
            Self::Release => "release",
            Self::PublishableBenchmark => "publishable-benchmark",
        }
    }

    const fn max_required_skip_ratio_bps(self) -> u32 {
        match self {
            Self::SmokeDev => 10_000,
            Self::Ci | Self::Release | Self::PublishableBenchmark => 0,
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Policy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "smoke-dev" | "smoke" | "dev" => Ok(Self::SmokeDev),
            "ci" => Ok(Self::Ci),
            "release" => Ok(Self::Release),
            "publishable-benchmark" | "publishable" | "benchmark" => Ok(Self::PublishableBenchmark),
            other => Err(format!(
                "unknown coverage policy {other:?}; expected smoke-dev, ci, release, or publishable-benchmark"
            )),
        }
    }
}

/// Machine-readable skip class for skipped scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SkipClass {
    /// Explicitly planned skip, such as an intentionally disabled scenario.
    Intentional,
    /// The environment was not ready: server, auth, or agent prerequisites.
    Environmental,
    /// The runner stopped early for control-flow reasons.
    RunControl,
    /// Unknown skip reason; treated conservatively by strict policies.
    Unknown,
}

/// Machine-readable skip reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SkipKind {
    /// Target instance was unreachable.
    InstanceUnreachable,
    /// Auth token was required but absent.
    MissingAuthToken,
    /// Nous agent was required but absent.
    MissingNousAgents,
    /// Remaining scenario was skipped after fail-fast.
    FailFast,
    /// Scenario was skipped intentionally by the suite or operator.
    Intentional,
    /// Unknown skip reason.
    Unknown,
}

impl SkipKind {
    /// Return the broader class for this skip reason.
    #[must_use]
    pub const fn class(self) -> SkipClass {
        match self {
            Self::InstanceUnreachable | Self::MissingAuthToken | Self::MissingNousAgents => {
                SkipClass::Environmental
            }
            Self::FailFast => SkipClass::RunControl,
            Self::Intentional => SkipClass::Intentional,
            Self::Unknown => SkipClass::Unknown,
        }
    }
}

/// Classify a human-readable skip reason into a stable machine value.
#[must_use]
pub fn classify_skip(reason: &str) -> SkipKind {
    match reason {
        SKIP_REASON_INSTANCE_UNREACHABLE => SkipKind::InstanceUnreachable,
        SKIP_REASON_NO_AUTH_TOKEN => SkipKind::MissingAuthToken,
        SKIP_REASON_NO_NOUS_AGENTS => SkipKind::MissingNousAgents,
        SKIP_REASON_FAIL_FAST => SkipKind::FailFast,
        _ if reason.starts_with("intentional:") => SkipKind::Intentional,
        _ => SkipKind::Unknown,
    }
}

/// Coverage policy violation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Violation {
    /// No required scenarios were selected.
    NoRequiredScenarios,
    /// At least one required scenario was skipped.
    RequiredScenariosSkipped {
        /// Required scenarios skipped.
        skipped: usize,
        /// Required scenarios selected.
        required: usize,
    },
    /// Required skip ratio exceeded the policy threshold.
    RequiredSkipRatioExceeded {
        /// Required scenarios skipped.
        skipped: usize,
        /// Required scenarios selected.
        required: usize,
        /// Observed skip ratio, in basis points.
        ratio_bps: u32,
        /// Maximum allowed skip ratio, in basis points.
        max_bps: u32,
    },
    /// Server/auth/nous prerequisite skip was present.
    EnvironmentalPrerequisiteMissing {
        /// Scenarios skipped because the instance was unreachable.
        instance_unreachable: usize,
        /// Scenarios skipped because auth was missing.
        missing_auth: usize,
        /// Scenarios skipped because nous agents were missing.
        missing_nous: usize,
    },
    /// Fail-fast caused required scenarios to remain unrun.
    RunControlSkippedRequired {
        /// Scenarios skipped by fail-fast.
        skipped: usize,
    },
    /// Release-grade policy forbids any selected scenario skip.
    AnySkipForbidden {
        /// Total skipped scenarios.
        skipped: usize,
    },
}

impl Violation {
    /// Human-readable explanation for CLI output.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NoRequiredScenarios => "no required scenarios were selected".to_owned(),
            Self::RequiredScenariosSkipped { skipped, required } => {
                format!("{skipped} of {required} required scenario(s) were skipped")
            }
            Self::RequiredSkipRatioExceeded {
                skipped,
                required,
                ratio_bps,
                max_bps,
            } => format!(
                "required skip ratio {} exceeded policy maximum {} ({skipped}/{required} skipped)",
                format_bps(*ratio_bps),
                format_bps(*max_bps)
            ),
            Self::EnvironmentalPrerequisiteMissing {
                instance_unreachable,
                missing_auth,
                missing_nous,
            } => format!(
                "environmental prerequisite skips present: server={instance_unreachable}, auth={missing_auth}, nous={missing_nous}"
            ),
            Self::RunControlSkippedRequired { skipped } => {
                format!("fail-fast skipped {skipped} required scenario(s)")
            }
            Self::AnySkipForbidden { skipped } => {
                format!("policy forbids skipped scenarios, but {skipped} scenario(s) were skipped")
            }
        }
    }
}

/// Aggregated coverage denominator and policy result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Policy used to evaluate the report.
    pub policy: Policy,
    /// Total selected scenarios.
    pub total_scenarios: usize,
    /// Required selected scenarios.
    pub required_scenarios: usize,
    /// Required scenarios that passed.
    pub passed_required: usize,
    /// Required scenarios that failed.
    pub failed_required: usize,
    /// Required scenarios that were skipped.
    pub skipped_required: usize,
    /// Total intentional skips.
    pub intentional_skips: usize,
    /// Total environmental skips.
    pub environmental_skips: usize,
    /// Total run-control skips.
    pub run_control_skips: usize,
    /// Total unknown skips.
    pub unknown_skips: usize,
    /// Skips caused by an unreachable instance.
    pub instance_unreachable_skips: usize,
    /// Skips caused by a missing auth token.
    pub missing_auth_skips: usize,
    /// Skips caused by missing nous agents.
    pub missing_nous_skips: usize,
    /// Skips caused by fail-fast.
    pub fail_fast_skips: usize,
    /// Required scenario pass rate in basis points.
    pub required_pass_rate_bps: u32,
    /// Required scenario skip ratio in basis points.
    pub required_skip_ratio_bps: u32,
    /// Whether the coverage policy passed.
    pub passed: bool,
    /// Policy violations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub violations: Vec<Violation>,
}

impl Summary {
    #[must_use]
    fn from_report(policy: Policy, report: &RunReport) -> Self {
        let mut summary = Self {
            policy,
            total_scenarios: report.results.len(),
            required_scenarios: 0,
            passed_required: 0,
            failed_required: 0,
            skipped_required: 0,
            intentional_skips: 0,
            environmental_skips: 0,
            run_control_skips: 0,
            unknown_skips: 0,
            instance_unreachable_skips: 0,
            missing_auth_skips: 0,
            missing_nous_skips: 0,
            fail_fast_skips: 0,
            required_pass_rate_bps: 0,
            required_skip_ratio_bps: 0,
            passed: true,
            violations: Vec::new(),
        };

        for result in &report.results {
            let required = required_for_coverage(&result.meta);
            if required {
                summary.required_scenarios += 1;
            }

            match &result.outcome {
                ScenarioOutcome::Passed { .. } if required => summary.passed_required += 1,
                ScenarioOutcome::Failed { .. } if required => summary.failed_required += 1,
                ScenarioOutcome::Skipped { reason } => {
                    let kind = classify_skip(reason);
                    summary.record_skip(kind);
                    if required {
                        summary.skipped_required += 1;
                    }
                }
                ScenarioOutcome::Passed { .. } | ScenarioOutcome::Failed { .. } => {}
            }
        }

        summary.required_pass_rate_bps =
            ratio_bps(summary.passed_required, summary.required_scenarios);
        summary.required_skip_ratio_bps =
            ratio_bps(summary.skipped_required, summary.required_scenarios);
        summary.violations = summary.evaluate_violations();
        summary.passed = summary.violations.is_empty();
        summary
    }

    /// Return each policy violation as a human-readable string.
    #[must_use]
    pub fn violation_messages(&self) -> Vec<String> {
        self.violations.iter().map(Violation::message).collect()
    }

    /// Return a single CLI-friendly failure message when the policy failed.
    #[must_use]
    pub fn failure_message(&self) -> Option<String> {
        if self.passed {
            return None;
        }

        let mut lines = vec![format!("eval coverage policy '{}' failed:", self.policy)];
        for message in self.violation_messages() {
            lines.push(format!("  - {message}"));
        }
        Some(lines.join("\n"))
    }

    fn record_skip(&mut self, kind: SkipKind) {
        match kind {
            SkipKind::InstanceUnreachable => {
                self.environmental_skips += 1;
                self.instance_unreachable_skips += 1;
            }
            SkipKind::MissingAuthToken => {
                self.environmental_skips += 1;
                self.missing_auth_skips += 1;
            }
            SkipKind::MissingNousAgents => {
                self.environmental_skips += 1;
                self.missing_nous_skips += 1;
            }
            SkipKind::FailFast => {
                self.run_control_skips += 1;
                self.fail_fast_skips += 1;
            }
            SkipKind::Intentional => self.intentional_skips += 1,
            SkipKind::Unknown => self.unknown_skips += 1,
        }
    }

    fn evaluate_violations(&self) -> Vec<Violation> {
        if self.policy == Policy::SmokeDev {
            return Vec::new();
        }

        let mut violations = Vec::new();
        if self.required_scenarios == 0 {
            violations.push(Violation::NoRequiredScenarios);
        }

        if self.skipped_required > 0 {
            violations.push(Violation::RequiredScenariosSkipped {
                skipped: self.skipped_required,
                required: self.required_scenarios,
            });
        }

        let max_bps = self.policy.max_required_skip_ratio_bps();
        if self.required_skip_ratio_bps > max_bps {
            violations.push(Violation::RequiredSkipRatioExceeded {
                skipped: self.skipped_required,
                required: self.required_scenarios,
                ratio_bps: self.required_skip_ratio_bps,
                max_bps,
            });
        }

        if self.environmental_skips > 0 {
            violations.push(Violation::EnvironmentalPrerequisiteMissing {
                instance_unreachable: self.instance_unreachable_skips,
                missing_auth: self.missing_auth_skips,
                missing_nous: self.missing_nous_skips,
            });
        }

        if self.fail_fast_skips > 0 {
            violations.push(Violation::RunControlSkippedRequired {
                skipped: self.fail_fast_skips,
            });
        }

        if matches!(self.policy, Policy::Release | Policy::PublishableBenchmark) {
            let skipped = self.intentional_skips
                + self.environmental_skips
                + self.run_control_skips
                + self.unknown_skips;
            if skipped > 0 {
                violations.push(Violation::AnySkipForbidden { skipped });
            }
        }

        violations
    }
}

/// Whether a scenario is part of the required coverage denominator.
#[must_use]
pub const fn required_for_coverage(meta: &ScenarioMeta) -> bool {
    !matches!(meta.classification, ScenarioClassification::Informational)
}

/// Format a basis-point ratio as a percentage with two decimals.
#[must_use]
pub fn format_bps(bps: u32) -> String {
    format!("{}.{:02}%", bps / 100, bps % 100)
}

fn ratio_bps(numerator: usize, denominator: usize) -> u32 {
    if denominator == 0 {
        return 0;
    }
    let bps = numerator.saturating_mul(10_000) / denominator;
    u32::try_from(bps).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::provenance::EvalProvenance;
    use crate::scenario::{ScenarioMeta, ScenarioResult};

    use super::*;

    fn meta(id: &'static str, classification: ScenarioClassification) -> ScenarioMeta {
        ScenarioMeta {
            id,
            description: "test scenario",
            category: "test",
            requires_auth: false,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
            classification,
        }
    }

    fn result(meta: ScenarioMeta, outcome: ScenarioOutcome) -> ScenarioResult {
        ScenarioResult {
            meta,
            outcome,
            sub_results: Vec::new(),
        }
    }

    fn report(results: Vec<ScenarioResult>) -> RunReport {
        let passed = results.iter().filter(|r| r.outcome.is_passed()).count();
        let failed = results.iter().filter(|r| r.outcome.is_failed()).count();
        let skipped = results
            .iter()
            .filter(|r| matches!(r.outcome, ScenarioOutcome::Skipped { .. }))
            .count();
        RunReport {
            passed,
            failed,
            skipped,
            total_duration: Duration::from_millis(10),
            results,
            provenance: EvalProvenance::new("er-test", "http://localhost"),
        }
    }

    #[test]
    fn ci_policy_fails_required_environmental_skip() {
        let report = report(vec![
            result(
                meta("health-ok", ScenarioClassification::Assertive),
                ScenarioOutcome::Passed {
                    duration: Duration::from_millis(5),
                },
            ),
            result(
                meta("session-create", ScenarioClassification::Assertive),
                ScenarioOutcome::Skipped {
                    reason: SKIP_REASON_NO_AUTH_TOKEN.to_owned(),
                },
            ),
        ]);

        let summary = Policy::Ci.evaluate(&report);

        assert!(!summary.passed);
        assert_eq!(summary.required_scenarios, 2);
        assert_eq!(summary.passed_required, 1);
        assert_eq!(summary.skipped_required, 1);
        assert_eq!(summary.missing_auth_skips, 1);
        assert!(
            summary
                .violations
                .iter()
                .any(|v| matches!(v, Violation::EnvironmentalPrerequisiteMissing { .. }))
        );
    }

    #[test]
    fn smoke_dev_policy_reports_but_allows_skips() {
        let report = report(vec![result(
            meta("session-create", ScenarioClassification::Assertive),
            ScenarioOutcome::Skipped {
                reason: SKIP_REASON_NO_NOUS_AGENTS.to_owned(),
            },
        )]);

        let summary = Policy::SmokeDev.evaluate(&report);

        assert!(summary.passed);
        assert_eq!(summary.skipped_required, 1);
        assert_eq!(summary.missing_nous_skips, 1);
        assert_eq!(summary.required_skip_ratio_bps, 10_000);
    }

    #[test]
    fn informational_scenarios_are_not_required() {
        let report = report(vec![result(
            meta("passive-probe", ScenarioClassification::Informational),
            ScenarioOutcome::Skipped {
                reason: "intentional: optional probe".to_owned(),
            },
        )]);

        let summary = Policy::Ci.evaluate(&report);

        assert_eq!(summary.required_scenarios, 0);
        assert_eq!(summary.intentional_skips, 1);
        assert!(
            summary
                .violations
                .iter()
                .any(|v| matches!(v, Violation::NoRequiredScenarios))
        );
    }

    #[test]
    fn release_policy_forbids_any_skip() {
        let report = report(vec![result(
            meta("passive-probe", ScenarioClassification::Informational),
            ScenarioOutcome::Skipped {
                reason: "intentional: optional probe".to_owned(),
            },
        )]);

        let summary = Policy::Release.evaluate(&report);

        assert!(
            summary
                .violations
                .iter()
                .any(|v| matches!(v, Violation::AnySkipForbidden { skipped: 1 }))
        );
    }

    #[test]
    fn policy_from_str_accepts_aliases() {
        assert_eq!("smoke".parse::<Policy>(), Ok(Policy::SmokeDev));
        assert_eq!(
            "benchmark".parse::<Policy>(),
            Ok(Policy::PublishableBenchmark)
        );
        assert!("unknown".parse::<Policy>().is_err());
    }

    #[test]
    fn skip_classification_is_stable() {
        assert_eq!(
            classify_skip(SKIP_REASON_INSTANCE_UNREACHABLE),
            SkipKind::InstanceUnreachable
        );
        assert_eq!(
            classify_skip("intentional: not on this platform"),
            SkipKind::Intentional
        );
        assert_eq!(SkipKind::MissingAuthToken.class(), SkipClass::Environmental);
    }

    #[test]
    fn basis_points_format_as_percentage() {
        assert_eq!(format_bps(12), "0.12%");
        assert_eq!(format_bps(9_876), "98.76%");
    }
}
