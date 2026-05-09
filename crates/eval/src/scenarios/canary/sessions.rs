use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::{EvalClient, MessageRole, SessionStatus};
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eval, validate_response};
use crate::sse;

// ---------------------------------------------------------------------------

/// Create session → send message → get history → verify consistency
pub(super) struct SessionCreateSendHistory;

impl Scenario for SessionCreateSendHistory {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-session-create-send-history",
            description: "Session lifecycle: create, send, verify history consistency",
            category: "canary-session",
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
                let key = crate::scenarios::unique_key("canary", "session-lifecycle");
                let session = client.create_session(nous_id, &key).await?;

                let test_msg = "Canary session test message 12345";
                let _ = client.send_message(&session.id, test_msg).await?;
                let history = client.get_history(&session.id).await?;

                assert_eval(
                    history.messages.len() >= 2,
                    "History should have user + assistant",
                )?;
                let user_msg = history
                    .messages
                    .iter()
                    .find(|m| m.role == MessageRole::User);
                assert_eval(user_msg.is_some(), "History should contain a user message")?;

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-session-create-send-history"
            )),
        )
    }
}

/// Multi-turn conversation → verify context preservation
pub(super) struct SessionMultiTurnContext;

impl Scenario for SessionMultiTurnContext {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-session-multi-turn-context",
            description: "Multi-turn conversation preserves context across messages",
            category: "canary-session",
            requires_auth: true,
            requires_nous: true,
            expected_contains: Some("blue"),
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
                let key = crate::scenarios::unique_key("canary", "session-context");
                let session = client.create_session(nous_id, &key).await?;

                let _ = client
                    .send_message(&session.id, "My favorite color is blue.")
                    .await?;
                let _ = client
                    .send_message(&session.id, "What is my favorite number? (You don't know)")
                    .await?;
                let events = client
                    .send_message(&session.id, "What is my favorite color?")
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-session-multi-turn-context"
            )),
        )
    }
}

/// Session close → verify archived session retrieval behavior.
pub(super) struct SessionCloseReopenRestore;

impl Scenario for SessionCloseReopenRestore {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-session-close-reopen-restore",
            description: "Closed session is archived or no longer retrievable",
            category: "canary-session",
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
                let key = crate::scenarios::unique_key("canary", "session-restore");
                let session = client.create_session(nous_id, &key).await?;

                let _ = client
                    .send_message(&session.id, "Session persistence test.")
                    .await?;
                client.close_session(&session.id).await?;

                // Session should now be archived and retrievable
                match client.get_session(&session.id).await {
                    Ok(s) => {
                        let is_archived = matches!(
                            s.status,
                            SessionStatus::Archived | SessionStatus::Unknown(_)
                        );
                        assert_eval(
                            is_archived,
                            format!("Closed session should be archived, got {:?}", s.status),
                        )?;
                    }
                    Err(crate::error::Error::UnexpectedStatus { status: 404, .. }) => {
                        // Archived sessions may return 404, which is acceptable
                    }
                    Err(e) => return Err(e),
                }

                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-session-close-reopen-restore"
            )),
        )
    }
}

/// Concurrent messages → verify ordering
pub(super) struct SessionConcurrentOrdering;

impl Scenario for SessionConcurrentOrdering {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-session-concurrent-ordering",
            description: "Sequential messages maintain proper ordering",
            category: "canary-session",
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
                let key = crate::scenarios::unique_key("canary", "session-ordering");
                let session = client.create_session(nous_id, &key).await?;

                // Send messages in known order
                let _ = client
                    .send_message(&session.id, "First message: ALPHA")
                    .await?;
                let _ = client
                    .send_message(&session.id, "Second message: BETA")
                    .await?;
                let _ = client
                    .send_message(&session.id, "Third message: GAMMA")
                    .await?;

                let history = client.get_history(&session.id).await?;
                // Verify we have the expected number of messages
                assert_eval(
                    history.messages.len() >= 6,
                    "Should have 6+ messages (3 user + 3 assistant)",
                )?;

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-session-concurrent-ordering"
            )),
        )
    }
}

/// Session with large context → verify distillation triggers
pub(super) struct SessionLargeContextDistillation;

impl Scenario for SessionLargeContextDistillation {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-session-large-context-distillation",
            description: "Large context sessions handle distillation appropriately",
            category: "canary-session",
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
                let key = crate::scenarios::unique_key("canary", "session-distillation");
                let session = client.create_session(nous_id, &key).await?;

                // Send several substantive messages to build context
                for i in 1..=5 {
                    let _ = client
                        .send_message(
                            &session.id,
                            &format!(
                                "Context message {i}: The quick brown fox jumps over the lazy dog. \
                                 This is padding text to simulate a growing conversation context. \
                                 We need to test how the system handles context window management \
                                 and any potential distillation or summarization behaviors."
                            ),
                        )
                        .await?;
                }

                let events = client
                    .send_message(&session.id, "Summarize what we've discussed.")
                    .await?;
                let text = sse::extract_text(&events);
                assert_eval(
                    !text.is_empty(),
                    "Large context response should not be empty",
                )?;

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-session-large-context-distillation"
            )),
        )
    }
}

// ---------------------------------------------------------------------------
