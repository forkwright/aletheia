//! Nous list/status scenarios.

use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eq_eval, assert_eval};

#[tracing::instrument(skip_all)]
pub(crate) fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![
        Box::new(NousListReturnsArray),
        Box::new(NousUnknownReturns404),
    ]
}

struct NousListReturnsArray;
impl Scenario for NousListReturnsArray {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "nous-list-returns-array",
            description: "GET /api/v1/nous returns agent list",
            category: "nous",
            requires_auth: true,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let list = client.list_nous().await?;
                for nous in &list {
                    assert_eval(!nous.id.is_empty(), "nous id should not be empty")?;
                }
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "nous-list-returns-array"
            )),
        )
    }
}

struct NousUnknownReturns404;
impl Scenario for NousUnknownReturns404 {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "nous-unknown-returns-404",
            description: "GET /api/v1/nous/{unknown} returns 404",
            category: "nous",
            requires_auth: true,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                match client.get_nous("nonexistent-eval-test-id").await {
                    Err(crate::error::Error::UnexpectedStatus { status, .. }) => {
                        assert_eq_eval(&status, &404, "expected 404 for unknown nous")
                    }
                    Err(e) => Err(e),
                    Ok(_) => crate::error::AssertionSnafu {
                        message: "expected 404 but got success",
                    }
                    .fail(),
                }
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "nous-unknown-returns-404"
            )),
        )
    }
}
