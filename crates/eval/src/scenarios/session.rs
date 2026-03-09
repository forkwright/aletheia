//! Session CRUD lifecycle scenarios.

use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eq_eval, assert_eval};

#[tracing::instrument(skip_all)]
pub fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![
        Box::new(SessionCreateAndGet),
        Box::new(SessionCloseArchives),
        Box::new(SessionUnknown404),
    ]
}

fn unique_key(suffix: &str) -> String {
    format!(
        "eval-{}-{}",
        suffix,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}

struct SessionCreateAndGet;
impl Scenario for SessionCreateAndGet {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "session-create-and-get",
            description: "Create a session and retrieve it by ID",
            category: "session",
            requires_auth: true,
            requires_nous: true,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous_id = &nous_list[0].id;
                let key = unique_key("create");
                let session = client.create_session(nous_id, &key).await?;
                assert_eval(!session.id.is_empty(), "session id should not be empty")?;
                assert_eq_eval(&session.nous_id, nous_id, "nous_id should match")?;
                assert_eq_eval(&session.session_key, &key, "session_key should match")?;
                assert_eq_eval(
                    &session.status,
                    &"active".to_owned(),
                    "status should be active",
                )?;
                let fetched = client.get_session(&session.id).await?;
                assert_eq_eval(&fetched.id, &session.id, "fetched session id should match")?;
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "session-create-and-get"
            )),
        )
    }
}

struct SessionCloseArchives;
impl Scenario for SessionCloseArchives {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "session-close-archives",
            description: "Closing a session sets status to archived",
            category: "session",
            requires_auth: true,
            requires_nous: true,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous_id = &nous_list[0].id;
                let key = unique_key("close");
                let session = client.create_session(nous_id, &key).await?;
                client.close_session(&session.id).await?;
                let fetched = client.get_session(&session.id).await?;
                assert_eq_eval(
                    &fetched.status,
                    &"archived".to_owned(),
                    "closed session should be archived",
                )
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "session-close-archives"
            )),
        )
    }
}

struct SessionUnknown404;
impl Scenario for SessionUnknown404 {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "session-unknown-404",
            description: "GET for nonexistent session returns 404",
            category: "session",
            requires_auth: true,
            requires_nous: false,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                match client.get_session("nonexistent-eval-session-id").await {
                    Err(crate::error::Error::UnexpectedStatus { status, .. }) => {
                        assert_eq_eval(&status, &404, "expected 404 for unknown session")
                    }
                    Err(e) => Err(e),
                    Ok(_) => crate::error::AssertionSnafu {
                        message: "expected 404 but got success",
                    }
                    .fail(),
                }
            }
            .instrument(tracing::info_span!("scenario", id = "session-unknown-404")),
        )
    }
}
