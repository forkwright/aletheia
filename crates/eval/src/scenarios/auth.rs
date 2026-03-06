//! Auth rejection scenarios.

use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eq_eval};

pub fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![Box::new(AuthRejectsNoToken), Box::new(AuthRejectsBadToken)]
}

struct AuthRejectsNoToken;
impl Scenario for AuthRejectsNoToken {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "auth-rejects-no-token",
            description: "Protected endpoint returns 401 without auth token",
            category: "auth",
            requires_auth: false,
            requires_nous: false,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(async move {
            let body =
                serde_json::json!({ "nous_id": "nonexistent", "session_key": "eval-auth-test" });
            let resp = client.raw_post("/api/v1/sessions", &body).await?;
            let status = resp.status().as_u16();
            assert_eq_eval(&status, &401, "expected 401 for unauthenticated request")
        }.instrument(tracing::info_span!("scenario", id = "auth-rejects-no-token")))
    }
}

struct AuthRejectsBadToken;
impl Scenario for AuthRejectsBadToken {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "auth-rejects-bad-token",
            description: "Protected endpoint returns 401 for invalid token",
            category: "auth",
            requires_auth: false,
            requires_nous: false,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(async move {
            let resp = client
                .raw_get_with_token("/api/v1/nous", "invalid-garbage-token")
                .await?;
            let status = resp.status().as_u16();
            assert_eq_eval(&status, &401, "expected 401 for invalid token")
        }.instrument(tracing::info_span!("scenario", id = "auth-rejects-bad-token")))
    }
}
