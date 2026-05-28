use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, validate_response};
use crate::sse;

// ---------------------------------------------------------------------------

/// Insert fact → query it back → verify exact match
pub(super) struct RecallInsertQueryRoundtrip;

impl Scenario for RecallInsertQueryRoundtrip {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-insert-query-roundtrip",
            description: "Insert a specific fact and verify exact recall on query",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: Some(" eval-canary-test-fact "),
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
                let key = crate::scenarios::unique_key("canary", "recall-roundtrip");
                let session = client.create_session(nous_id, &key).await?;

                let _ = client
                    .send_message(
                        &session.id,
                        "Remember this exact fact: eval-canary-test-fact-42",
                    )
                    .await?;
                let events = client
                    .send_message(&session.id, "What fact did I just ask you to remember?")
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-insert-query-roundtrip"
            )),
        )
    }
}

/// Insert 3 related facts → search by semantic query → verify recall
pub(super) struct RecallSemanticSearch;

impl Scenario for RecallSemanticSearch {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-semantic-search",
            description: "Insert related facts and verify semantic search recall",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: Some(r"(?i)(redwood|sequoia|pine|oak|maple)"),
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
                let key = crate::scenarios::unique_key("canary", "recall-semantic");
                let session = client.create_session(nous_id, &key).await?;

                let _ = client
                    .send_message(
                        &session.id,
                        "Remember: Redwoods are the tallest trees. \
                         Sequoias are the most massive. \
                         Bristlecone pines are the oldest.",
                    )
                    .await?;
                let events = client
                    .send_message(&session.id, "Tell me about the tallest trees.")
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-semantic-search"
            )),
        )
    }
}

/// Insert fact with high confidence → insert contradicting fact → verify conflict detection
pub(super) struct RecallConflictDetection;

impl Scenario for RecallConflictDetection {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-conflict-detection",
            description: "Insert conflicting facts and verify conflict detection",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: Some(r"(?i)(conflict|contradict|inconsistent|discrepancy)"),
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
                let key = crate::scenarios::unique_key("canary", "recall-conflict");
                let session = client.create_session(nous_id, &key).await?;

                let _ = client
                    .send_message(
                        &session.id,
                        "The capital of France is Paris. This is certain.",
                    )
                    .await?;
                let _ = client
                    .send_message(
                        &session.id,
                        "Actually, the capital of France is Lyon. \
                         I was wrong before.",
                    )
                    .await?;
                let events = client
                    .send_message(
                        &session.id,
                        "What is the capital of France? Is there any conflict in what I told you?",
                    )
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-conflict-detection"
            )),
        )
    }
}

/// Insert temporal facts → query by time range → verify ordering
pub(super) struct RecallTemporalOrdering;

impl Scenario for RecallTemporalOrdering {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-temporal-ordering",
            description: "Insert temporal facts and verify chronological ordering",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: Some(r"(?i)(1969.*1989|Apollo.*Berlin|moon.*wall)"),
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
                let key = crate::scenarios::unique_key("canary", "recall-temporal");
                let session = client.create_session(nous_id, &key).await?;

                let _ = client
                    .send_message(
                        &session.id,
                        "In 1989, the Berlin Wall fell. \
                         In 1969, humans first landed on the moon. \
                         In 1991, the Soviet Union dissolved.",
                    )
                    .await?;
                let events = client
                    .send_message(
                        &session.id,
                        "List the events I mentioned in chronological order.",
                    )
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-temporal-ordering"
            )),
        )
    }
}

/// Search with empty knowledge → verify graceful empty result
pub(super) struct RecallEmptyKnowledgeGraceful;

impl Scenario for RecallEmptyKnowledgeGraceful {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-empty-knowledge-graceful",
            description: "Query with no relevant knowledge returns graceful response",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: Some(
                r"(?i)(don't know|not sure|no information|haven't been told|unclear)",
            ),
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
                let key = crate::scenarios::unique_key("canary", "recall-empty");
                let session = client.create_session(nous_id, &key).await?;

                let events = client
                    .send_message(
                        &session.id,
                        "What is my favorite color? \
                         (Note: I have not told you this information)",
                    )
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-empty-knowledge-graceful"
            )),
        )
    }
}

// ---------------------------------------------------------------------------
