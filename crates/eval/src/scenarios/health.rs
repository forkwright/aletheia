//! Health endpoint scenarios.

use tracing::Instrument;

use crate::client::{EvalClient, InstanceStatus};
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eval};

#[tracing::instrument(skip_all)]
pub(crate) fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![
        Box::new(HealthReturnsOk),
        Box::new(HealthContainsVersion),
        Box::new(HealthReportsChecks),
    ]
}

struct HealthReturnsOk;
impl Scenario for HealthReturnsOk {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "health-returns-ok",
            description: "Health endpoint returns healthy or degraded status",
            category: "health",
            requires_auth: false,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let health = client.health().await?;
                assert_eval(
                    matches!(
                        health.status,
                        InstanceStatus::Healthy | InstanceStatus::Degraded
                    ),
                    format!("expected healthy or degraded, got {:?}", health.status),
                )
            }
            .instrument(tracing::info_span!("scenario", id = "health-returns-ok")),
        )
    }
}

struct HealthContainsVersion;
impl Scenario for HealthContainsVersion {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "health-contains-version",
            description: "Health response includes non-empty version",
            category: "health",
            requires_auth: false,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let health = client.health().await?;
                assert_eval(!health.version.is_empty(), "version field is empty")
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "health-contains-version"
            )),
        )
    }
}

struct HealthReportsChecks;
impl Scenario for HealthReportsChecks {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "health-reports-checks",
            description: "Health response includes at least one check",
            category: "health",
            requires_auth: false,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let health = client.health().await?;
                assert_eval(!health.checks.is_empty(), "checks array is empty")
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "health-reports-checks"
            )),
        )
    }
}
