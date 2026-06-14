//! Health endpoint scenarios.

use tracing::Instrument;

use crate::client::{EvalClient, InstanceStatus};
use crate::scenario::{
    Scenario, ScenarioClassification, ScenarioFuture, ScenarioMeta, assert_eval,
};

#[tracing::instrument(skip_all)]
pub(crate) fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![
        Box::new(HealthReturnsOk),
        Box::new(HealthOmitsVersion),
        Box::new(HealthOmitsDiagnostics),
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

            classification: ScenarioClassification::Assertive,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let health = client.health().await?;
                    assert_eval(
                        matches!(
                            health.status,
                            InstanceStatus::Healthy | InstanceStatus::Degraded
                        ),
                        format!("expected healthy or degraded, got {:?}", health.status),
                    )
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!("scenario", id = "health-returns-ok")),
        )
    }
}

struct HealthOmitsVersion;
impl Scenario for HealthOmitsVersion {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "health-omits-version",
            description: "Public health response does not expose build metadata",
            category: "health",
            requires_auth: false,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,

            classification: ScenarioClassification::Assertive,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let health = client.health().await?;
                    assert_eval(
                        health.version.is_none(),
                        "public health response exposed version",
                    )?;
                    assert_eval(
                        health.uptime_seconds.is_none(),
                        "public health response exposed uptime",
                    )
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!("scenario", id = "health-omits-version")),
        )
    }
}

struct HealthOmitsDiagnostics;
impl Scenario for HealthOmitsDiagnostics {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "health-omits-diagnostics",
            description: "Public health response does not expose diagnostics",
            category: "health",
            requires_auth: false,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,

            classification: ScenarioClassification::Assertive,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let health = client.health().await?;
                    assert_eval(
                        health.checks.is_empty(),
                        "public health response exposed diagnostic checks",
                    )?;
                    assert_eval(
                        health.data_dir.is_none(),
                        "public health response exposed data directory",
                    )
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "health-omits-diagnostics"
            )),
        )
    }
}
