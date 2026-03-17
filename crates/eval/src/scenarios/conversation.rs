//! Conversation scenarios: message flow, SSE, history.

use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::{EvalClient, MessageRole};
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eval, validate_response};
use crate::sse;

#[tracing::instrument(skip_all)]
pub(crate) fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![
        Box::new(ConversationSendSse),
        Box::new(ConversationHistoryReflects),
        Box::new(ConversationMultiTurn),
        Box::new(ConversationEmptyRejected),
    ]
}

struct ConversationSendSse;
impl Scenario for ConversationSendSse {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "conversation-send-sse",
            description: "Send message returns SSE stream with text_delta and message_complete",
            category: "conversation",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;
                let nous_id = &nous.id;
                let key = super::unique_key("conv", "sse");
                let session = client.create_session(nous_id, &key).await?;
                let events = client
                    .send_message(&session.id, "Hello, this is an eval test.")
                    .await?;
                assert_eval(!events.is_empty(), "SSE stream should not be empty")?;
                assert_eval(
                    sse::is_complete(&events),
                    "SSE stream should contain message_complete event",
                )?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "conversation-send-sse"
            )),
        )
    }
}

struct ConversationHistoryReflects;
impl Scenario for ConversationHistoryReflects {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "conversation-history-reflects",
            description: "After sending a message, history contains user + assistant messages",
            category: "conversation",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;
                let nous_id = &nous.id;
                let key = super::unique_key("conv", "history");
                let session = client.create_session(nous_id, &key).await?;
                let _ = client
                    .send_message(&session.id, "Eval history test message.")
                    .await?;
                let history = client.get_history(&session.id).await?;
                assert_eval(
                    history.messages.len() >= 2,
                    format!(
                        "history should have at least 2 messages, got {}",
                        history.messages.len()
                    ),
                )?;
                assert_eval(
                    history.messages[0].role == MessageRole::User,
                    format!(
                        "first message should be user, got {:?}",
                        history.messages[0].role
                    ),
                )?;
                assert_eval(
                    history.messages[1].role == MessageRole::Assistant,
                    format!(
                        "second message should be assistant, got {:?}",
                        history.messages[1].role
                    ),
                )?;
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "conversation-history-reflects"
            )),
        )
    }
}

struct ConversationMultiTurn;
impl Scenario for ConversationMultiTurn {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "conversation-multi-turn",
            description: "Two consecutive messages produce 4+ messages in history",
            category: "conversation",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;
                let nous_id = &nous.id;
                let key = super::unique_key("conv", "multi");
                let session = client.create_session(nous_id, &key).await?;
                let _ = client
                    .send_message(&session.id, "First eval message.")
                    .await?;
                let _ = client
                    .send_message(&session.id, "Second eval message.")
                    .await?;
                let history = client.get_history(&session.id).await?;
                assert_eval(
                    history.messages.len() >= 4,
                    format!(
                        "should have at least 4 messages after 2 turns, got {}",
                        history.messages.len()
                    ),
                )?;
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "conversation-multi-turn"
            )),
        )
    }
}

struct ConversationEmptyRejected;
impl Scenario for ConversationEmptyRejected {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "conversation-empty-rejected",
            description: "Sending empty content returns 400",
            category: "conversation",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,
        }
    }
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;
                let nous_id = &nous.id;
                let key = super::unique_key("conv", "empty");
                let session = client.create_session(nous_id, &key).await?;
                match client.send_message(&session.id, "").await {
                    Err(crate::error::Error::UnexpectedStatus { status, .. }) => {
                        assert_eval(status == 400, format!("expected 400, got {status}"))
                    }
                    Err(e) => Err(e),
                    Ok(_) => crate::error::AssertionSnafu {
                        message: "expected 400 for empty content but got success",
                    }
                    .fail(),
                }
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "conversation-empty-rejected"
            )),
        )
    }
}
